//! A piece: clips on a timeline, a song under them, rendered frame by frame into a file.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use crate::clip::{Clip, SEAM_TOLERANCE, Seam};
use crate::error::Error;
use crate::ports::{Audio, Blend, Compose, Encode, Job, Layer, Music, Picture, Signal, Sound};
use crate::profile::Profile;
use crate::song::Song;
use crate::span::Span;
use crate::transition::{self, Edge, TransitionKind, Window, sweep_masks};
use crate::types::{Frame, Seconds, Values, View};

/// How many frames a worker may be ahead of the one the encoder wants next, so memory stays flat: a frame in flight is
/// 16 bytes a pixel, 15 MB at 720 by 1280.
pub const FRAMES_AHEAD_PER_WORKER: usize = 2;

/// The width in pixels of the gap between the cells of a contact sheet (a dark grey, so that a black cell shows against it).
pub const SHEET_GAP: usize = 6;

/// The grey of a sheet's gaps.
const SHEET_BACKGROUND: [f32; 4] = [0.12, 0.12, 0.12, 1.0];

/// A video being made: a [`Profile`], clips, and at most one [`Song`].
///
/// Built by calls that check what they are given; every later error is a refusal of the picture or the encoder. A piece is
/// cheap to clone: pictures, signals and the song are shared, so a variant never copies what it does not change.
#[derive(Clone)]
pub struct Piece {
    profile: Profile,
    clips: Vec<Clip>,
    song: Option<Song>,
    compose: Option<Arc<dyn Compose>>,
    camera: Option<Camera>,
    transitions: Vec<Window>,
    looped: bool,
}

/// The signals that move the finished frame; one that is not given leaves its part of the view at rest.
#[derive(Clone)]
struct Camera {
    dx: Option<Arc<dyn Signal>>,
    dy: Option<Arc<dyn Signal>>,
    zoom: Option<Arc<dyn Signal>>,
    rotate: Option<Arc<dyn Signal>>,
}

/// What a render did: the frames it wrote, how many of them it had to draw (a frame equal to the one before it is that frame
/// again) and how many of those it put together with the compose port (a camera at rest and a cut cost none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderStats {
    /// Frames written.
    pub frames: usize,
    /// Frames drawn.
    pub drawn: usize,
    /// Drawn frames that went through the compose port.
    pub composed: usize,
}

/// What one frame is made of: the clips that draw it (none: black) with the values to draw each with, how the second of two
/// is mixed in, and the camera's view of the result.
#[derive(PartialEq)]
struct Step {
    layers: Vec<(usize, Values)>,
    mix: Option<Mix>,
    view: View,
}

/// A transition under way: the kind (never a cut) and how far along it is.
#[derive(Clone, Copy, PartialEq)]
struct Mix {
    kind: TransitionKind,
    progress: f64,
}

impl Step {
    /// Whether the frame needs the compose port: two layers, or a view that moves the picture.
    fn composes(&self) -> bool {
        !self.layers.is_empty() && (self.layers.len() > 1 || self.view != View::IDENTITY)
    }
}

impl Piece {
    /// An empty piece with `profile`.
    #[must_use]
    pub fn new(profile: Profile) -> Self {
        Self {
            profile,
            clips: Vec::new(),
            song: None,
            compose: None,
            camera: None,
            transitions: Vec::new(),
            looped: false,
        }
    }

    /// Makes the piece's clock a circle (or not): the frame after the last would be the first. Checked when it renders.
    pub fn set_looped(&mut self, looped: bool) {
        self.looped = looped;
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

    /// Gives the piece the port that puts frames together; a camera and a transition need it.
    pub fn compose(&mut self, compose: Arc<dyn Compose>) {
        self.compose = Some(compose);
    }

    /// Moves the finished frame: `dx` and `dy` as fractions of its long side, `zoom` (one is none) and `rotate` in degrees,
    /// each a signal read at the frame's time; one that is `None` stays at rest. Replaces an earlier camera. A frame whose
    /// view is at rest is not composed at all.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the piece has no compose port.
    pub fn camera(
        &mut self,
        dx: Option<Arc<dyn Signal>>,
        dy: Option<Arc<dyn Signal>>,
        zoom: Option<Arc<dyn Signal>>,
        rotate: Option<Arc<dyn Signal>>,
    ) -> Result<(), Error> {
        self.needs_compose("a camera")?;
        self.camera = Some(Camera { dx, dy, zoom, rotate });
        Ok(())
    }

    /// A transition from clip `a` to clip `b`, centred on `at` and `length` seconds long. Both clips are drawn during the
    /// window (a clip is drawn outside its own span); clips may overlap only inside a window.
    ///
    /// # Errors
    /// [`Error::Invalid`] when a clip is unknown or the same twice, `at` or `length` is not a finite number (or `length` is not
    /// above zero), the kind is not usable, the window overlaps another, or the piece has no compose port.
    pub fn transition(
        &mut self,
        a: &str,
        b: &str,
        at: Seconds,
        length: Seconds,
        kind: TransitionKind,
    ) -> Result<(), Error> {
        let index = |name: &str| {
            self.clips
                .iter()
                .position(|c| c.name() == name)
                .ok_or_else(|| self.no_clip(name))
        };
        let (a, b) = (index(a)?, index(b)?);
        if a == b {
            return Err(invalid(
                "a transition goes from one clip to another, not to itself".into(),
            ));
        }
        if !(at.is_finite() && length.is_finite() && length > 0.0) {
            return Err(invalid(format!(
                "a transition needs a finite moment and a length above zero, got {at} and {length}"
            )));
        }
        transition::check(kind)?;
        if !matches!(kind, TransitionKind::Cut) {
            self.needs_compose("a dissolve or a sweep")?;
        }
        let window = Window { a, b, at, length, kind };
        if let Some(other) = self
            .transitions
            .iter()
            .find(|w| window.start() < w.end() && w.start() < window.end())
        {
            return Err(invalid(format!(
                "the transition from `{}` to `{}` overlaps the one from `{}` to `{}`: two run one after the other, not together",
                self.clips[a].name(),
                self.clips[b].name(),
                self.clips[other.a].name(),
                self.clips[other.b].name()
            )));
        }
        self.transitions.push(window);
        Ok(())
    }

    fn needs_compose(&self, what: &str) -> Result<(), Error> {
        if self.compose.is_some() {
            Ok(())
        } else {
            Err(invalid(format!(
                "{what} needs a compose port to put frames together: give the piece one with `compose`"
            )))
        }
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
    /// in order and the file does not depend on the number of workers. A frame whose clip and values equal the frame before it is
    /// not drawn again: that frame is handed over once more (a held pose, a still, a gap costs one drawing).
    ///
    /// # Errors
    /// [`Error::Invalid`] when `workers` is zero or the piece cannot be laid out; the picture's, the signals' and the encoder's.
    pub fn render(&self, encoder: &dyn Encode, path: &Path, workers: usize) -> Result<RenderStats, Error> {
        if workers == 0 {
            return Err(invalid("workers must be at least one".into()));
        }
        if self.looped {
            self.check_loop()?;
        }
        let layout = self.layout()?;
        let frames = self.frame_count()?;
        let fps = self.profile.fps();
        #[expect(clippy::cast_precision_loss, reason = "a frame index is far below 2^52")]
        let times: Vec<Seconds> = (0..frames).map(|i| i as f64 / fps).collect();
        let steps = self.steps(&layout, &times)?;
        // the frames that need drawing, and for every frame which of them it is
        let mut drawn_at: Vec<usize> = Vec::new();
        let mut slot_of: Vec<usize> = Vec::with_capacity(frames);
        for index in 0..frames {
            if index == 0 || steps[index] != steps[index - 1] {
                drawn_at.push(index);
            }
            slot_of.push(drawn_at.len() - 1);
        }
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
        let gate = Gate::new(drawn_at.len(), workers * FRAMES_AHEAD_PER_WORKER);
        let (sender, receiver) = mpsc::channel();
        std::thread::scope(|scope| {
            for _ in 0..workers {
                let sender = sender.clone();
                let (gate, steps, drawn_at) = (&gate, &steps, &drawn_at);
                scope.spawn(move || {
                    while let Some(slot) = gate.claim() {
                        if sender
                            .send((slot, self.draw_or_report(&steps[drawn_at[slot]])))
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            }
            drop(sender);
            let mut ordered = Ordered {
                receiver,
                pending: BTreeMap::new(),
                slot_of: &slot_of,
                next: 0,
                current: None,
                gate: &gate,
            };
            let result = encoder.encode(&job, &mut ordered);
            gate.stop();
            drop(ordered);
            result
        })?;
        Ok(RenderStats {
            frames,
            drawn: drawn_at.len(),
            composed: drawn_at.iter().filter(|&&at| steps[at].composes()).count(),
        })
    }

    /// The same piece as a draft: the frame `scale` times the size (each side to the nearest even number) and, if `fps` is given, at
    /// that rate. Everything else is shared with this piece, which is left as it was. A picture draws at any size, so a draft costs
    /// roughly `scale` squared of a full render; a picture whose shape does not scale to whole pixels is refused when it draws.
    ///
    /// # Errors
    /// [`Error::Invalid`] when `scale` or `fps` is not a positive finite number.
    pub fn draft(&self, scale: f64, fps: Option<f64>) -> Result<Self, Error> {
        let scaled = self.profile.scaled(scale)?;
        let profile = match fps {
            Some(fps) => scaled.with_fps(fps)?,
            None => scaled,
        };
        Ok(Self {
            profile,
            ..self.clone()
        })
    }

    /// Whether the clock's circle closes: one clip (so no transition), no fade, and every mapping and camera signal the same at the
    /// start and at the end (within [`SEAM_TOLERANCE`]).
    fn check_loop(&self) -> Result<(), Error> {
        let refuse = |why: &str| {
            invalid(format!(
                "a looped piece cannot have {why}: build it without `loop`, or remove that"
            ))
        };
        if self.clips.len() > 1 {
            return Err(refuse(
                "more than one clip or any transition (a transition joins two clips), which would show at the seam",
            ));
        }
        if self.song.as_ref().is_some_and(Song::faded) {
            return Err(refuse("a song with a fade in or out, which would be heard at the seam"));
        }
        let end = self.duration()?;
        for clip in &self.clips {
            if let Some(Seam { address, first, last }) = clip.seam(end)? {
                return Err(invalid(format!(
                    "the loop does not close: `{address}` of clip `{}` is {first:?} at 0 s and {last:?} at {end} s; make that mapping periodic over the piece (a whole number of turns), or drop `loop`",
                    clip.name()
                )));
            }
        }
        let views = self.views(&[0.0, end])?;
        let (a, b) = (views[0], views[1]);
        // a turn of the picture is the same one 360 degrees on
        let turn = (a.rotate - b.rotate).abs() % 360.0;
        for (name, first, last, gap) in [
            ("dx", a.dx, b.dx, (a.dx - b.dx).abs()),
            ("dy", a.dy, b.dy, (a.dy - b.dy).abs()),
            ("zoom", a.zoom, b.zoom, (a.zoom - b.zoom).abs()),
            ("rotate", a.rotate, b.rotate, turn.min(360.0 - turn)),
        ] {
            if gap > SEAM_TOLERANCE {
                return Err(invalid(format!(
                    "the loop does not close: the camera's {name} is {first} at 0 s and {last} at {end} s; make that signal periodic over the piece, or drop `loop`"
                )));
            }
        }
        Ok(())
    }

    /// Which clip draws each moment: clips by start, an end that is not given reaching the next clip or the piece's end. An
    /// overlap is allowed only where a transition between the two covers it.
    fn layout(&self) -> Result<Vec<(usize, Seconds, Seconds)>, Error> {
        let duration = self.duration()?;
        let mut order: Vec<usize> = (0..self.clips.len()).collect();
        order.sort_by(|&a, &b| self.clips[a].start().total_cmp(&self.clips[b].start()));
        let mut laid = Vec::with_capacity(order.len());
        for (place, &index) in order.iter().enumerate() {
            let clip = &self.clips[index];
            let following = order.get(place + 1).copied();
            let end = clip
                .end()
                .unwrap_or_else(|| following.map_or(duration, |n| self.clips[n].start()));
            if let Some(n) = following.filter(|&n| end > self.clips[n].start()) {
                let next = &self.clips[n];
                let covered = self
                    .transitions
                    .iter()
                    .any(|w| w.a == index && w.b == n && w.start() <= next.start() && end <= w.end());
                if !covered {
                    return Err(invalid(format!(
                        "clips `{0}` and `{1}` overlap from {2} s to {end} s: cover it with a transition (`transition(\"{0}\", \"{1}\", at, length)`) or end `{0}` where `{1}` starts",
                        clip.name(),
                        next.name(),
                        next.start(),
                    )));
                }
            }
            laid.push((index, clip.start(), end));
        }
        Ok(laid)
    }

    /// The view at each of `times`: the camera's signals read once for all of them.
    fn views(&self, times: &[Seconds]) -> Result<Vec<View>, Error> {
        let Some(camera) = &self.camera else {
            return Ok(vec![View::IDENTITY; times.len()]);
        };
        let read = |name: &str, signal: &Option<Arc<dyn Signal>>, rest: f64| -> Result<Vec<f64>, Error> {
            let Some(signal) = signal else {
                return Ok(vec![rest; times.len()]);
            };
            let raw = signal.sample(times).map_err(|e| match e {
                Error::Port { port, message } => Error::Port {
                    port,
                    message: format!("camera {name}: {message}"),
                },
                other => other,
            })?;
            if raw.len() != times.len() {
                return Err(Error::Port {
                    port: "signal",
                    message: format!("camera {name}: gave {} values for {} times", raw.len(), times.len()),
                });
            }
            match raw.iter().zip(times).find(|(v, _)| !v.is_finite()) {
                Some((v, t)) => Err(invalid(format!("camera {name} at {t} s: the value is {v}"))),
                None => Ok(raw),
            }
        };
        let (dx, dy) = (read("dx", &camera.dx, 0.0)?, read("dy", &camera.dy, 0.0)?);
        let (zoom, rotate) = (read("zoom", &camera.zoom, 1.0)?, read("rotate", &camera.rotate, 0.0)?);
        if let Some((z, t)) = zoom.iter().zip(times).find(|(z, _)| **z <= 0.0) {
            return Err(invalid(format!(
                "camera zoom at {t} s is {z}: a zoom above zero is wanted (one is none)"
            )));
        }
        Ok((0..times.len())
            .map(|i| View {
                dx: dx[i],
                dy: dy[i],
                zoom: zoom[i],
                rotate: rotate[i],
            })
            .collect())
    }

    /// What each of `times` is made of, every clip's signals read once for all its times.
    fn steps(&self, layout: &[(usize, Seconds, Seconds)], times: &[Seconds]) -> Result<Vec<Step>, Error> {
        let mut steps: Vec<Step> = self
            .views(times)?
            .into_iter()
            .map(|view| Step {
                layers: Vec::new(),
                mix: None,
                view,
            })
            .collect();
        for &(index, start, end) in layout {
            let mine: Vec<usize> = (0..times.len())
                .filter(|&i| start <= times[i] && times[i] < end)
                .collect();
            let at: Vec<Seconds> = mine.iter().map(|&i| times[i]).collect();
            for (&i, values) in mine.iter().zip(self.clips[index].values(&at)?) {
                steps[i].layers = vec![(index, values)];
            }
        }
        for window in &self.transitions {
            let mine: Vec<usize> = (0..times.len())
                .filter(|&i| window.start() <= times[i] && times[i] < window.end())
                .collect();
            let at: Vec<Seconds> = mine.iter().map(|&i| times[i]).collect();
            let (first, second) = (self.clips[window.a].values(&at)?, self.clips[window.b].values(&at)?);
            for ((&i, a), b) in mine.iter().zip(first).zip(second) {
                let progress = window.progress(times[i]);
                match window.kind {
                    TransitionKind::Cut => {
                        steps[i].layers = vec![if progress < 0.5 { (window.a, a) } else { (window.b, b) }];
                    }
                    kind => {
                        steps[i].layers = vec![(window.a, a), (window.b, b)];
                        steps[i].mix = Some(Mix { kind, progress });
                    }
                }
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

    /// One frame: the clips' pictures with their values put together, or black where no clip is.
    fn draw(&self, step: &Step) -> Result<Frame, Error> {
        let (width, height) = (self.profile.width() as usize, self.profile.height() as usize);
        if step.layers.is_empty() {
            return Frame::new(width, height, [0.0, 0.0, 0.0, 1.0].repeat(width * height));
        }
        let mut frames = Vec::with_capacity(step.layers.len());
        for (index, values) in &step.layers {
            let frame = self.clips[*index].picture().render(values, self.profile.long_side())?;
            frames.push(self.in_profile_shape(*index, frame)?);
        }
        self.put_together(step, frames)
    }

    /// The frames of a step's layers as one: the only one as it is when the view is at rest, else through the compose port.
    fn put_together(&self, step: &Step, mut frames: Vec<Frame>) -> Result<Frame, Error> {
        let view = step.view;
        let at_rest = |frame| Layer::new(frame, view);
        let compose = |layers: &[Layer<'_>]| match &self.compose {
            Some(compose) => compose.compose(layers),
            None => Err(invalid(
                "a camera or a transition needs a compose port: give the piece one with `compose`".into(),
            )),
        };
        let Some(Mix { kind, progress }) = step.mix else {
            return match frames.pop() {
                Some(only) if step.view == View::IDENTITY => Ok(only),
                Some(only) => compose(&[at_rest(&only)]),
                None => Err(invalid("a frame with no layer".into())),
            };
        };
        let [first, second] = &frames[..] else {
            return Err(invalid("a transition mixes two frames".into()));
        };
        match kind {
            TransitionKind::Sweep { edge, angle, feather } => {
                let (shown, bar) = sweep_masks(first.width(), first.height(), angle, feather, progress)?;
                let level = if edge == Edge::Light { 1.0 } else { 0.0 };
                let shade = Frame::new(
                    first.width(),
                    first.height(),
                    [level, level, level, 1.0].repeat(first.width() * first.height()),
                )?;
                compose(&[
                    at_rest(first),
                    Layer {
                        mask: Some(&shown),
                        ..at_rest(second)
                    },
                    Layer {
                        mask: Some(&bar),
                        blend: if edge == Edge::Light {
                            Blend::Screen
                        } else {
                            Blend::Normal
                        },
                        ..at_rest(&shade)
                    },
                ])
            }
            _ => compose(&[
                at_rest(first),
                Layer {
                    opacity: progress,
                    ..at_rest(second)
                },
            ]),
        }
    }

    /// `frame`, refused if it is not the shape of the profile.
    fn in_profile_shape(&self, index: usize, frame: Frame) -> Result<Frame, Error> {
        let (width, height) = (self.profile.width() as usize, self.profile.height() as usize);
        if (frame.width(), frame.height()) != (width, height) {
            return Err(Error::Port {
                port: "picture",
                message: format!(
                    "clip `{}` drew {} by {} but the profile is {width} by {height}: make the picture the shape of the profile",
                    self.clips[index].name(),
                    frame.width(),
                    frame.height()
                ),
            });
        }
        Ok(frame)
    }

    /// A contact sheet: the piece at each of the moments `at` (seconds), every one drawn `width` pixels along its long side, side by
    /// side with [`SHEET_GAP`] between (or, with `columns`, that many to a row, the last row padded with the gap's colour),
    /// written as one picture to `path` through `encoder`. A moment no clip draws is a black cell.
    ///
    /// # Errors
    /// [`Error::Invalid`] when `at` is empty, a moment is not inside the piece, `width` is under two pixels or `columns` is 0;
    /// [`Error::Port`] when the pictures draw cells of different sizes, and the pictures' and the encoder's errors.
    pub fn sheet(
        &self,
        encoder: &dyn Encode,
        path: &Path,
        at: &[Seconds],
        width: u32,
        columns: Option<usize>,
    ) -> Result<(), Error> {
        if at.is_empty() || width < 2 || columns == Some(0) {
            return Err(invalid(format!(
                "a sheet needs at least one moment, cells of two pixels or more and one column or more, got {} moments, {width} pixels and {columns:?} columns",
                at.len()
            )));
        }
        let duration = self.duration()?;
        if let Some(t) = at.iter().find(|t| !(t.is_finite() && **t >= 0.0 && **t < duration)) {
            return Err(invalid(format!(
                "the moment {t} s is not inside the piece, which lasts {duration} s: its last frame is at {} s",
                duration - 1.0 / self.profile.fps()
            )));
        }
        let layout = self.layout()?;
        let steps = self.steps(&layout, at)?;
        let drawn = steps
            .iter()
            .map(|step| {
                if step.layers.is_empty() {
                    return Ok(None);
                }
                let frames = step
                    .layers
                    .iter()
                    .map(|(index, values)| self.clips[*index].picture().render(values, width))
                    .collect::<Result<Vec<Frame>, Error>>()?;
                self.put_together(step, frames).map(Some)
            })
            .collect::<Result<Vec<Option<Frame>>, Error>>()?;
        let (cell_width, cell_height) = drawn.iter().flatten().next().map_or_else(
            || {
                // nothing draws at any moment: black cells in the profile's shape at this size
                let long = f64::from(self.profile.long_side());
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a cell side is a small positive number of pixels"
                )]
                let scale = |side: u32| ((f64::from(side) * f64::from(width) / long).round() as usize).max(1);
                (scale(self.profile.width()), scale(self.profile.height()))
            },
            |frame| (frame.width(), frame.height()),
        );
        let cells = drawn
            .into_iter()
            .map(|cell| {
                cell.map_or_else(
                    || {
                        Frame::new(
                            cell_width,
                            cell_height,
                            [0.0, 0.0, 0.0, 1.0].repeat(cell_width * cell_height),
                        )
                    },
                    Ok,
                )
            })
            .collect::<Result<Vec<Frame>, Error>>()?;
        let sheet = tile(&cells, columns.unwrap_or(cells.len()))?;
        let job = Job {
            path,
            fps: 1.0,
            depth: 8,
            codec: "h264",
            audio: None,
        };
        encoder.encode(&job, &mut std::iter::once(Ok(sheet)))
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

/// The frames in order, however the workers finish them, a frame that was not drawn again being the one drawn before.
struct Ordered<'a> {
    receiver: Receiver<(usize, Result<Frame, Error>)>,
    pending: BTreeMap<usize, Result<Frame, Error>>,
    /// For every frame, the drawing that makes it.
    slot_of: &'a [usize],
    next: usize,
    /// The latest drawing and what it came to, for the frames that repeat it.
    current: Option<(usize, Result<Frame, Error>)>,
    gate: &'a Gate,
}

impl Iterator for Ordered<'_> {
    type Item = Result<Frame, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let slot = *self.slot_of.get(self.next)?;
        self.next += 1;
        if let Some((held, frame)) = &self.current
            && *held == slot
        {
            return Some(frame.clone());
        }
        let frame = loop {
            if let Some(frame) = self.pending.remove(&slot) {
                break frame;
            }
            match self.receiver.recv() {
                Ok((index, frame)) => {
                    self.pending.insert(index, frame);
                }
                Err(_) => {
                    break Err(Error::Port {
                        port: "picture",
                        message: "a render worker stopped before it drew every frame".into(),
                    });
                }
            }
        };
        self.gate.consumed(slot + 1);
        self.current = Some((slot, frame.clone()));
        Some(frame)
    }
}

fn invalid(message: String) -> Error {
    Error::Invalid { message }
}

/// The cells in rows of `columns`, `SHEET_GAP` apart; the last row is padded with the background.
fn tile(cells: &[Frame], columns: usize) -> Result<Frame, Error> {
    let columns = columns.min(cells.len());
    let rows = cells.len().div_ceil(columns);
    let (cell_width, height) = (cells[0].width(), cells[0].height());
    if let Some(odd) = cells.iter().find(|c| (c.width(), c.height()) != (cell_width, height)) {
        return Err(Error::Port {
            port: "picture",
            message: format!(
                "the cells of a sheet must be one size: {cell_width} by {height} and {} by {}",
                odd.width(),
                odd.height()
            ),
        });
    }
    let width = columns * cell_width + (columns - 1) * SHEET_GAP;
    let total = rows * height + (rows - 1) * SHEET_GAP;
    let mut pixels = SHEET_BACKGROUND.repeat(width * total);
    for (n, cell) in cells.iter().enumerate() {
        let left = (n % columns) * (cell_width + SHEET_GAP);
        let top = (n / columns) * (height + SHEET_GAP);
        for row in 0..height {
            let from = row * cell_width * 4;
            let to = ((top + row) * width + left) * 4;
            pixels[to..to + cell_width * 4].copy_from_slice(&cell.pixels()[from..from + cell_width * 4]);
        }
    }
    Frame::new(width, total, pixels)
}
