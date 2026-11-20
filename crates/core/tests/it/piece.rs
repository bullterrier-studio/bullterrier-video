//! `Piece`: the timeline, the frame loop and what reaches the encoder, through fakes of every port.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use bullterrier_video::ports::{Encode, Job, Picture};
use bullterrier_video::{Error, FRAMES_AHEAD_PER_WORKER, Frame, Parameter, Piece, Profile, Span, Value, Values};

use crate::common::fakes::{FakeMusic, FakeSound, FnSignal, number, parameter};

/// A picture of one flat grey whose level is its `level`. Early (low) levels take longer to draw, so frames finish out of order.
/// It can fail from a level on, or draw one pixel too small.
struct Flat {
    drawn: Arc<AtomicUsize>,
    fail_from: Option<f64>,
    shrink: bool,
}

impl Flat {
    fn new() -> (Arc<Self>, Arc<AtomicUsize>) {
        let drawn = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(Self {
                drawn: drawn.clone(),
                fail_from: None,
                shrink: false,
            }),
            drawn,
        )
    }

    fn failing_from(level: f64) -> Arc<Self> {
        Arc::new(Self {
            drawn: Arc::new(AtomicUsize::new(0)),
            fail_from: Some(level),
            shrink: false,
        })
    }

    fn too_small() -> Arc<Self> {
        Arc::new(Self {
            drawn: Arc::new(AtomicUsize::new(0)),
            fail_from: None,
            shrink: true,
        })
    }
}

impl Picture for Flat {
    fn parameters(&self) -> Result<Vec<Parameter>, Error> {
        Ok(vec![parameter("level", number(0.0, 1.0, false, false))])
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the fake's levels are small"
    )]
    fn render(&self, values: &Values, long_side: u32) -> Result<Frame, Error> {
        let Some(Value::Number(level)) = values.get("level") else {
            return Err(Error::Port {
                port: "picture",
                message: "no level".into(),
            });
        };
        for spin in 0..((1.0 - level) * 20_000.0) as u64 {
            std::hint::black_box(spin);
        }
        self.drawn.fetch_add(1, Ordering::SeqCst);
        if self.fail_from.is_some_and(|from| *level >= from) {
            return Err(Error::Port {
                port: "picture",
                message: "too bright".into(),
            });
        }
        let side = (long_side - u32::from(self.shrink)) as usize;
        Frame::new(side, side, [*level as f32, 0.0, 0.0, 1.0].repeat(side * side))
    }
}

/// What an encoder was asked to write: the rate, depth, codec, and the soundtrack's rate, channels and samples.
type Wrote = (f64, u8, String, Option<(u32, u16, Vec<f32>)>);

/// An encoder that keeps the first value of every frame and what it was asked to write.
#[derive(Default)]
struct Collect {
    levels: Mutex<Vec<f32>>,
    wrote: Mutex<Option<Wrote>>,
    drawn: Option<Arc<AtomicUsize>>,
    most_ahead: AtomicUsize,
}

impl Encode for Collect {
    fn encode(&self, job: &Job<'_>, frames: &mut dyn Iterator<Item = Result<Frame, Error>>) -> Result<(), Error> {
        let audio = job.audio.as_ref().map(|a| (a.rate, a.channels, a.samples.to_vec()));
        *self.wrote.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((job.fps, job.depth, job.codec.to_string(), audio));
        for (k, frame) in frames.enumerate() {
            let frame = frame?;
            self.levels
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(frame.pixels()[0]);
            if let Some(drawn) = &self.drawn {
                self.most_ahead
                    .fetch_max(drawn.load(Ordering::SeqCst).saturating_sub(k + 1), Ordering::SeqCst);
            }
        }
        Ok(())
    }
}

fn levels(encoder: &Collect) -> Vec<f32> {
    encoder.levels.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// A 4 by 4 piece at `fps` with a two-second song of nothing and one clip `a` whose level is the time.
fn piece(fps: f64, picture: Arc<dyn Picture>) -> Piece {
    let mut piece = Piece::new(Profile::square(4, fps).unwrap_or_else(|e| unreachable!("{e}")));
    piece
        .song(
            Arc::new(FakeMusic::of(2.0)),
            &FakeSound::of(2.0),
            &Span::Whole,
            0.0,
            0.0,
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip("a", picture, 0.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip_mut("a")
        .and_then(|c| c.map("level", Arc::new(FnSignal(Box::new(|t| t))), 0.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
}

fn render(piece: &Piece, encoder: &Collect, workers: usize) -> Result<(), Error> {
    piece.render(encoder, Path::new("out.mp4"), workers)
}

#[test]
fn frames_are_the_mapped_values_at_each_moment_in_order() {
    let encoder = Collect::default();
    render(&piece(4.0, Flat::new().0), &encoder, 1).unwrap_or_else(|e| unreachable!("{e}"));
    // two seconds at four a second: the level is the time, held to 1
    assert_eq!(levels(&encoder), vec![0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0]);
}

#[test]
fn the_file_does_not_depend_on_the_number_of_workers() {
    let serial = Collect::default();
    render(&piece(60.0, Flat::new().0), &serial, 1).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(levels(&serial).len(), 120);
    for workers in [2, 4, 8] {
        let parallel = Collect::default();
        render(&piece(60.0, Flat::new().0), &parallel, workers).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(levels(&parallel), levels(&serial), "{workers} workers");
    }
}

#[test]
fn workers_never_run_further_ahead_than_the_window() {
    for workers in [1, 4] {
        let (picture, drawn) = Flat::new();
        let encoder = Collect {
            drawn: Some(drawn),
            ..Collect::default()
        };
        render(&piece(100.0, picture), &encoder, workers).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(levels(&encoder).len(), 200);
        let ahead = encoder.most_ahead.load(Ordering::SeqCst);
        assert!(
            ahead <= workers * FRAMES_AHEAD_PER_WORKER,
            "{workers} workers drew {ahead} frames ahead"
        );
    }
}

#[test]
fn the_encoder_is_told_the_rate_the_depth_the_codec_and_the_soundtrack() {
    let mut piece = Piece::new(
        Profile::square(4, 4.0)
            .and_then(|p| p.with_depth(10))
            .map(|p| p.with_codec("hevc"))
            .and_then(|p| p.with_loudness(-14.0))
            .unwrap_or_else(|e| unreachable!("{e}")),
    );
    piece
        .song(
            Arc::new(FakeMusic::of(2.0)),
            &FakeSound::of(2.0),
            &Span::Whole,
            0.25,
            0.5,
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip("a", Flat::new().0, 0.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip_mut("a")
        .and_then(|c| c.set(Values::from([("level".to_string(), Value::Number(0.5))])))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let encoder = Collect::default();
    render(&piece, &encoder, 2).unwrap_or_else(|e| unreachable!("{e}"));
    let wrote = encoder.wrote.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(
        wrote,
        Some((4.0, 10, "hevc".to_string(), Some((4, 1, vec![2.0, 0.25, 0.5, -14.0])))),
        "duration, fades and loudness reach the sound"
    );
}

#[test]
fn a_piece_without_a_song_has_no_soundtrack() {
    let mut piece = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
    piece
        .clip("a", Flat::new().0, 0.0, Some(1.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip_mut("a")
        .and_then(|c| c.set(Values::from([("level".to_string(), Value::Number(0.5))])))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let encoder = Collect::default();
    render(&piece, &encoder, 1).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        levels(&encoder),
        vec![0.5; 4],
        "the clip's end is the piece's end: one second at four a second"
    );
    assert!(
        encoder
            .wrote
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|w| w.3.is_none())
    );
}

#[test]
fn a_moment_no_clip_covers_is_black_and_clips_cut_from_one_to_the_next() {
    let level = |v: f64| Values::from([("level".to_string(), Value::Number(v))]);
    let mut gap = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
    gap.song(
        Arc::new(FakeMusic::of(2.0)),
        &FakeSound::of(2.0),
        &Span::Whole,
        0.0,
        0.0,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    gap.clip("late", Flat::new().0, 1.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    gap.clip_mut("late")
        .and_then(|c| c.set(level(0.5)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let encoder = Collect::default();
    render(&gap, &encoder, 2).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        levels(&encoder),
        vec![0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5],
        "black until the clip starts"
    );

    let mut cut = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
    cut.song(
        Arc::new(FakeMusic::of(2.0)),
        &FakeSound::of(2.0),
        &Span::Whole,
        0.0,
        0.0,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    cut.clip("b", Flat::new().0, 1.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    cut.clip("a", Flat::new().0, 0.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    cut.clip_mut("a")
        .and_then(|c| c.set(level(0.2)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    cut.clip_mut("b")
        .and_then(|c| c.set(level(0.8)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let encoder = Collect::default();
    render(&cut, &encoder, 1).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        levels(&encoder),
        vec![0.2, 0.2, 0.2, 0.2, 0.8, 0.8, 0.8, 0.8],
        "a clip with no end runs to the next, whatever the order they were added in"
    );
}

#[test]
fn clips_that_overlap_are_refused_naming_both() {
    let mut piece = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
    piece
        .clip("a", Flat::new().0, 0.0, Some(1.5))
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip("b", Flat::new().0, 1.0, Some(2.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let Err(Error::Invalid { message }) = render(&piece, &Collect::default(), 1) else {
        unreachable!("accepted")
    };
    assert!(
        message.contains("`a`") && message.contains("`b`") && message.contains("overlap"),
        "{message}"
    );
}

#[test]
fn a_failing_frame_stops_the_encoder_and_the_error_comes_back() {
    for workers in [1, 4] {
        let encoder = Collect::default();
        let result = render(&piece(4.0, Flat::failing_from(0.5)), &encoder, workers);
        assert_eq!(
            result,
            Err(Error::Port {
                port: "picture",
                message: "too bright".into()
            }),
            "{workers} workers"
        );
        assert_eq!(
            levels(&encoder),
            vec![0.0, 0.25],
            "the frames before the first failure, and no more"
        );
    }
}

#[test]
fn a_picture_of_the_wrong_shape_is_refused_naming_the_sizes() {
    let mut piece = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
    piece
        .clip("a", Flat::too_small(), 0.0, Some(0.25))
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip_mut("a")
        .and_then(|c| c.set(Values::from([("level".to_string(), Value::Number(0.25))])))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let Err(Error::Port { message, .. }) = piece.frame(0.0) else {
        unreachable!("accepted")
    };
    assert!(
        message.contains("3 by 3") && message.contains("4 by 4") && message.contains("`a`"),
        "{message}"
    );
}

#[test]
fn one_frame_at_a_time_is_the_same_frame_the_render_makes() {
    let piece = piece(4.0, Flat::new().0);
    let frame = piece.frame(0.5).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((frame.width(), frame.height(), frame.pixels()[0]), (4, 4, 0.5));
    let gap = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
    let mut gap = gap;
    gap.clip("late", Flat::new().0, 1.0, Some(2.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        gap.frame(0.2).map(|f| f.pixels()[..4].to_vec()),
        Ok(vec![0.0, 0.0, 0.0, 1.0]),
        "black before the clip"
    );
}

#[test]
fn a_piece_that_cannot_be_laid_out_is_refused() {
    let mut empty = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
    assert!(
        matches!(empty.duration(), Err(Error::Invalid { .. })),
        "nothing gives it a length"
    );
    empty
        .clip("a", Flat::new().0, 0.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(empty.duration(), Err(Error::Invalid { .. })),
        "a clip with no end and no song"
    );
    let mut short = Piece::new(Profile::square(4, 1.0).unwrap_or_else(|e| unreachable!("{e}")));
    short
        .clip("a", Flat::new().0, 0.0, Some(0.1))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(short.frame_count(), Err(Error::Invalid { .. })),
        "shorter than a frame"
    );
    assert!(
        matches!(
            render(&piece(4.0, Flat::new().0), &Collect::default(), 0),
            Err(Error::Invalid { .. })
        ),
        "no workers"
    );
}

#[test]
fn names_and_songs_are_checked_when_they_are_given() {
    let mut piece = piece(4.0, Flat::new().0);
    let Err(Error::Invalid { message }) = piece.clip("a", Flat::new().0, 0.0, None) else {
        unreachable!("a name taken twice")
    };
    assert!(message.contains("`a`"), "{message}");
    let Err(Error::Invalid { message }) = piece.clip_mut("zz").map(|_| ()) else {
        unreachable!("an unknown clip")
    };
    assert!(message.contains("`zz`") && message.contains('a'), "{message}");
    assert!(
        matches!(
            piece.song(
                Arc::new(FakeMusic::of(2.0)),
                &FakeSound::of(2.0),
                &Span::Whole,
                0.0,
                0.0
            ),
            Err(Error::Invalid { .. })
        ),
        "a second song"
    );
    assert!(piece.clip_named("a").is_ok());
}
