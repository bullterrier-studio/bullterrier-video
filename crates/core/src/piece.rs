//! A piece: clips on a timeline, a song under them, rendered frame by frame into a file.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use crate::clip::Clip;
use crate::error::Error;
use crate::ports::{Audio, Encode, Job, Music, Picture, Sound};
use crate::profile::Profile;
use crate::song::Song;
use crate::span::Span;
use crate::types::{Frame, Seconds, Values};

/// How many frames a worker may be ahead of the one the encoder wants next, so memory stays flat: a frame in flight is
/// 16 bytes a pixel, 15 MB at 720 by 1280.
pub const FRAMES_AHEAD_PER_WORKER: usize = 2;

/// A video being made: a [`Profile`], clips, and at most one [`Song`].
///
/// Built by calls that check what they are given; every later error is a refusal of the picture or the encoder. A piece is
/// cheap to clone: pictures, signals and the song are shared, so a variant never copies what it does not change.
#[derive(Clone)]
pub struct Piece {
    profile: Profile,
    clips: Vec<Clip>,
    song: Option<Song>,
}

/// What one frame is made of: the clip that draws it (`None`: nothing, so black) and the values to draw it with.
struct Step {
    clip: Option<usize>,
    values: Values,
}

impl Piece {
    /// An empty piece with `profile`.
    #[must_use]
    pub fn new(profile: Profile) -> Self {
        Self {
            profile,
            clips: Vec::new(),
            song: None,
        }
    }

    /// The output profile.
    #[must_use]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Adds a clip of `picture` named `name`, from `start` to `end` (`None`: until the next clip or the piece's end).
    ///
    /// # Errors
    /// [`Error::Invalid`] when the name is taken or the clip is refused (see [`Clip::new`]); [`Error::Port`] when the picture
    /// cannot describe itself.
    pub fn clip(
        &mut self,
        name: &str,
        picture: Arc<dyn Picture>,
        start: Seconds,
        end: Option<Seconds>,
    ) -> Result<(), Error> {
        if self.clips.iter().any(|c| c.name() == name) {
            return Err(invalid(format!("the piece already has a clip named `{name}`")));
        }
        self.clips.push(Clip::new(name, picture, start, end)?);
        Ok(())
    }

    /// The clip named `name`.
    ///
    /// # Errors
    /// [`Error::Invalid`] when there is none (the names it has are listed).
    pub fn clip_named(&self, name: &str) -> Result<&Clip, Error> {
        self.clips
            .iter()
            .find(|c| c.name() == name)
            .ok_or_else(|| self.no_clip(name))
    }

    /// The clip named `name`, to map, set or swap.
    ///
    /// # Errors
    /// [`Error::Invalid`] when there is none.
    pub fn clip_mut(&mut self, name: &str) -> Result<&mut Clip, Error> {
        let known = self.clips.iter().map(Clip::name).collect::<Vec<_>>().join(", ");
        self.clips
            .iter_mut()
            .find(|c| c.name() == name)
            .ok_or_else(|| invalid(format!("the piece has no clip `{name}`; it has {known}")))
    }

    /// Puts the soundtrack under the piece: `sound` cut to `span` (found with `music`), faded, and set to the profile's loudness.
    /// A piece has one song: a timing variant builds the piece again, because its mappings hold the song's moments.
    ///
    /// # Errors
    /// [`Error::Invalid`] when there is a song already or the span cannot be placed; [`Error::Port`] from the music or the sound.
    pub fn song(
        &mut self,
        music: Arc<dyn Music>,
        sound: &dyn Sound,
        span: &Span,
        fade_in: Seconds,
        fade_out: Seconds,
    ) -> Result<Song, Error> {
        if self.song.is_some() {
            return Err(invalid(
                "the piece has a song already; build the piece again for a different span".into(),
            ));
        }
        let song = Song::new(music, sound, span, fade_in, fade_out, self.profile.loudness())?;
        self.song = Some(song.clone());
        Ok(song)
    }

    /// How long the piece lasts: the song's span, else the end of its last clip.
    ///
    /// # Errors
    /// [`Error::Invalid`] when there is no song and a clip has no end.
    pub fn duration(&self) -> Result<Seconds, Error> {
        if let Some(song) = &self.song {
            return Ok(song.duration());
        }
        let ends: Option<Vec<Seconds>> = self.clips.iter().map(Clip::end).collect();
        match ends {
            Some(ends) if !ends.is_empty() => Ok(ends.into_iter().fold(0.0, f64::max)),
            _ => Err(invalid(
                "a piece without a song needs every clip to have an end, so that it has a length".into(),
            )),
        }
    }

    /// The number of frames: the duration times the rate, rounded.
    ///
    /// # Errors
    /// [`Error::Invalid`] as [`Piece::duration`], or when the piece is shorter than a frame.
    pub fn frame_count(&self) -> Result<usize, Error> {
        let seconds = self.duration()?;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a frame count is a small positive number"
        )]
        let frames = (seconds * self.profile.fps()).round() as usize;
        if frames == 0 {
            return Err(invalid(format!(
                "a piece of {seconds} s has no frame at {} a second",
                self.profile.fps()
            )));
        }
        Ok(frames)
    }

    /// The frame at `t` seconds.
    ///
    /// # Errors
    /// [`Error::Invalid`] for a piece that cannot be laid out; the picture's errors.
    pub fn frame(&self, t: Seconds) -> Result<Frame, Error> {
        let layout = self.layout()?;
        let steps = self.steps(&layout, &[t])?;
        self.draw(&steps[0])
    }

    /// Renders the piece into the file `path` through `encoder`, drawing frames on `workers` threads. The frames reach the encoder
    /// in order and the file does not depend on the number of workers.
    ///
    /// # Errors
    /// [`Error::Invalid`] when `workers` is zero or the piece cannot be laid out; the picture's, the signals' and the encoder's.
    pub fn render(&self, encoder: &dyn Encode, path: &Path, workers: usize) -> Result<(), Error> {
        if workers == 0 {
            return Err(invalid("workers must be at least one".into()));
        }
        let layout = self.layout()?;
        let frames = self.frame_count()?;
        let fps = self.profile.fps();
        #[expect(clippy::cast_precision_loss, reason = "a frame index is far below 2^52")]
        let times: Vec<Seconds> = (0..frames).map(|i| i as f64 / fps).collect();
        let steps = self.steps(&layout, &times)?;
        let samples = self.song.as_ref().map(|s| s.soundtrack().samples()).transpose()?;
        let audio = match (&self.song, &samples) {
            (Some(song), Some(samples)) => Some(Audio {
                rate: song.soundtrack().rate(),
                channels: song.soundtrack().channels(),
                samples,
            }),
            _ => None,
        };
        let job = Job {
            path,
            fps,
            depth: self.profile.depth(),
            codec: self.profile.codec(),
            audio,
        };
        let gate = Gate::new(frames, workers * FRAMES_AHEAD_PER_WORKER);
        let (sender, receiver) = mpsc::channel();
        std::thread::scope(|scope| {
            for _ in 0..workers {
                let sender = sender.clone();
                let (gate, steps) = (&gate, &steps);
                scope.spawn(move || {
                    while let Some(index) = gate.claim() {
                        if sender.send((index, self.draw_or_report(&steps[index]))).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(sender);
            let mut ordered = Ordered {
                receiver,
                pending: BTreeMap::new(),
                next: 0,
                frames,
                gate: &gate,
            };
            let result = encoder.encode(&job, &mut ordered);
            gate.stop();
            drop(ordered);
            result
        })
    }

    /// Which clip draws each moment: clips by start, an end that is not given reaching the next clip or the piece's end.
    fn layout(&self) -> Result<Vec<(usize, Seconds, Seconds)>, Error> {
        let duration = self.duration()?;
        let mut order: Vec<usize> = (0..self.clips.len()).collect();
        order.sort_by(|&a, &b| self.clips[a].start().total_cmp(&self.clips[b].start()));
        let mut laid = Vec::with_capacity(order.len());
        for (place, &index) in order.iter().enumerate() {
            let clip = &self.clips[index];
            let next = order.get(place + 1).map(|&n| &self.clips[n]);
            let end = clip.end().unwrap_or_else(|| next.map_or(duration, Clip::start));
            if let Some(next) = next.filter(|n| end > n.start()) {
                return Err(invalid(format!(
                    "clips `{}` and `{}` overlap: layers on top of each other need the compose adapter, which is not built",
                    clip.name(),
                    next.name()
                )));
            }
            laid.push((index, clip.start(), end));
        }
        Ok(laid)
    }

    /// The values for each of `times`, every clip's signals read once for all its times.
    fn steps(&self, layout: &[(usize, Seconds, Seconds)], times: &[Seconds]) -> Result<Vec<Step>, Error> {
        let mut steps: Vec<Step> = times
            .iter()
            .map(|_| Step {
                clip: None,
                values: Values::new(),
            })
            .collect();
        for &(index, start, end) in layout {
            let mine: Vec<usize> = (0..times.len())
                .filter(|&i| start <= times[i] && times[i] < end)
                .collect();
            let at: Vec<Seconds> = mine.iter().map(|&i| times[i]).collect();
            for (&i, values) in mine.iter().zip(self.clips[index].values(&at)?) {
                steps[i] = Step {
                    clip: Some(index),
                    values,
                };
            }
        }
        Ok(steps)
    }

    /// [`Piece::draw`], with a picture that panics turned into the error it is: a worker that unwound would leave the others
    /// waiting for a frame that never comes.
    fn draw_or_report(&self, step: &Step) -> Result<Frame, Error> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.draw(step))).unwrap_or_else(|payload| {
            let said = payload
                .downcast_ref::<&str>()
                .map(ToString::to_string)
                .or_else(|| payload.downcast_ref::<String>().cloned());
            Err(Error::Port {
                port: "picture",
                message: format!(
                    "the picture panicked: {}",
                    said.unwrap_or_else(|| "without a message".into())
                ),
            })
        })
    }

    /// One frame: the clip's picture with the values, or black where no clip is.
    fn draw(&self, step: &Step) -> Result<Frame, Error> {
        let (width, height) = (self.profile.width() as usize, self.profile.height() as usize);
        let Some(index) = step.clip else {
            return Frame::new(width, height, [0.0, 0.0, 0.0, 1.0].repeat(width * height));
        };
        let clip = &self.clips[index];
        let frame = clip.picture().render(&step.values, self.profile.long_side())?;
        if (frame.width(), frame.height()) != (width, height) {
            return Err(Error::Port {
                port: "picture",
                message: format!(
                    "clip `{}` drew {} by {} but the profile is {width} by {height}: make the picture the shape of the profile",
                    clip.name(),
                    frame.width(),
                    frame.height()
                ),
            });
        }
        Ok(frame)
    }

    fn no_clip(&self, name: &str) -> Error {
        let known = self.clips.iter().map(Clip::name).collect::<Vec<_>>().join(", ");
        invalid(format!("the piece has no clip `{name}`; it has {known}"))
    }
}

/// Hands out frame numbers to workers, never more than `window` ahead of the one the consumer wants next.
struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
    frames: usize,
    window: usize,
}

struct GateState {
    claimed: usize,
    consumed: usize,
    stopped: bool,
}

impl Gate {
    fn new(frames: usize, window: usize) -> Self {
        Self {
            state: Mutex::new(GateState {
                claimed: 0,
                consumed: 0,
                stopped: false,
            }),
            changed: Condvar::new(),
            frames,
            window,
        }
    }

    /// The next frame number to draw, waiting while the workers are a window ahead; `None` when all are claimed or stopped.
    fn claim(&self) -> Option<usize> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if state.stopped || state.claimed >= self.frames {
                return None;
            }
            if state.claimed < state.consumed + self.window {
                state.claimed += 1;
                return Some(state.claimed - 1);
            }
            state = self.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn consumed(&self, count: usize) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).consumed = count;
        self.changed.notify_all();
    }

    fn stop(&self) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).stopped = true;
        self.changed.notify_all();
    }
}

/// The frames in order, however the workers finish them.
struct Ordered<'a> {
    receiver: Receiver<(usize, Result<Frame, Error>)>,
    pending: BTreeMap<usize, Result<Frame, Error>>,
    next: usize,
    frames: usize,
    gate: &'a Gate,
}

impl Iterator for Ordered<'_> {
    type Item = Result<Frame, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.frames {
            return None;
        }
        loop {
            if let Some(frame) = self.pending.remove(&self.next) {
                self.next += 1;
                self.gate.consumed(self.next);
                return Some(frame);
            }
            match self.receiver.recv() {
                Ok((index, frame)) => {
                    self.pending.insert(index, frame);
                }
                Err(_) => {
                    return Some(Err(Error::Port {
                        port: "picture",
                        message: "a render worker stopped before it drew every frame".into(),
                    }));
                }
            }
        }
    }
}

fn invalid(message: String) -> Error {
    Error::Invalid { message }
}
