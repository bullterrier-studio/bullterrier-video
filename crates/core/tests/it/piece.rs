//! `Piece`: the timeline, the frame loop and what reaches the encoder, through fakes of every port.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use bullterrier_video::ports::{Encode, Job, Picture};
use bullterrier_video::{
    Error, FRAMES_AHEAD_PER_WORKER, Frame, Parameter, Piece, Profile, RenderStats, Span, Value, Values,
};

use crate::common::fakes::{FakeMusic, FakeSound, FnSignal, number, parameter};

/// A picture of one flat grey whose level is its `level`. Early (low) levels take longer to draw, so frames finish out of order.
/// It can fail from a level on, or draw one pixel too small.
struct Flat {
    drawn: Arc<AtomicUsize>,
    fail_from: Option<f64>,
    panic_from: Option<f64>,
    shrink: bool,
    steps_down_from: Option<f64>,
}

impl Flat {
    fn with(fail_from: Option<f64>, panic_from: Option<f64>, shrink: bool) -> Arc<Self> {
        Arc::new(Self {
            drawn: Arc::new(AtomicUsize::new(0)),
            fail_from,
            panic_from,
            shrink,
            steps_down_from: None,
        })
    }

    fn new() -> (Arc<Self>, Arc<AtomicUsize>) {
        let picture = Self::with(None, None, false);
        let drawn = picture.drawn.clone();
        (picture, drawn)
    }

    fn failing_from(level: f64) -> Arc<Self> {
        Self::with(Some(level), None, false)
    }

    fn too_small() -> Arc<Self> {
        Self::with(None, None, true)
    }

    /// A picture one pixel smaller from `level` on.
    fn steps_down_at(level: f64) -> Arc<Self> {
        Arc::new(Self {
            drawn: Arc::new(AtomicUsize::new(0)),
            fail_from: None,
            panic_from: None,
            shrink: false,
            steps_down_from: Some(level),
        })
    }

    fn panicking_from(level: f64) -> Arc<Self> {
        Self::with(None, Some(level), false)
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
        assert!(!self.panic_from.is_some_and(|from| *level >= from), "the picture broke");
        if self.fail_from.is_some_and(|from| *level >= from) {
            return Err(Error::Port {
                port: "picture",
                message: "too bright".into(),
            });
        }
        let side = (long_side
            - u32::from(self.shrink)
            - u32::from(self.steps_down_from.is_some_and(|from| *level >= from))) as usize;
        Frame::new(side, side, [*level as f32, 0.0, 0.0, 1.0].repeat(side * side))
    }
}

/// What an encoder was asked to write: the rate, depth, codec, and the soundtrack's rate, channels and samples.
type Wrote = (f64, u8, String, Option<(u32, u16, Vec<f32>)>);

/// An encoder that keeps the one frame it is given whole, and the file it was asked to write.
#[derive(Default)]
struct Capture {
    frame: Mutex<Option<Frame>>,
    path: Mutex<Option<std::path::PathBuf>>,
}

impl Encode for Capture {
    fn encode(&self, job: &Job<'_>, frames: &mut dyn Iterator<Item = Result<Frame, Error>>) -> Result<(), Error> {
        *self.path.lock().unwrap_or_else(PoisonError::into_inner) = Some(job.path.to_path_buf());
        let mut kept = None;
        for frame in frames {
            assert!(kept.is_none(), "a sheet is one frame");
            kept = Some(frame?);
        }
        *self.frame.lock().unwrap_or_else(PoisonError::into_inner) = kept;
        Ok(())
    }
}

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

fn render(piece: &Piece, encoder: &Collect, workers: usize) -> Result<RenderStats, Error> {
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

#[test]
fn a_picture_that_panics_is_an_error_naming_it_and_never_a_hang() {
    for workers in [1, 2, 4] {
        let encoder = Collect::default();
        let result = render(&piece(100.0, Flat::panicking_from(0.5)), &encoder, workers);
        let Err(Error::Port {
            port: "picture",
            message,
        }) = result
        else {
            unreachable!("{workers} workers: {result:?}")
        };
        assert!(
            message.contains("panicked") && message.contains("the picture broke"),
            "{message}"
        );
    }
}

/// The grey of the sheet's gaps and of a moment nothing draws: `SHEET_GAP` pixels of it between cells.
const GAP: usize = bullterrier_video::SHEET_GAP;

fn sheet_of(piece: &Piece, at: &[f64], width: u32) -> Result<Frame, Error> {
    sheet_in(piece, at, width, None)
}

fn sheet_in(piece: &Piece, at: &[f64], width: u32, columns: Option<usize>) -> Result<Frame, Error> {
    let capture = Capture::default();
    piece.sheet(&capture, Path::new("sheet.png"), at, width, columns)?;
    assert_eq!(
        *capture.path.lock().unwrap_or_else(PoisonError::into_inner),
        Some(Path::new("sheet.png").to_path_buf())
    );
    Ok(capture
        .frame
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .unwrap_or_else(|| unreachable!("no sheet")))
}

#[test]
fn a_sheet_is_the_chosen_moments_side_by_side_at_a_small_size() {
    let piece = piece(4.0, Flat::new().0);
    let sheet = sheet_of(&piece, &[0.25, 0.5, 1.0], 6).unwrap_or_else(|e| unreachable!("{e}"));
    // the flat picture draws long_side by long_side squares: three cells of 6 and two gaps
    assert_eq!((sheet.width(), sheet.height()), (3 * 6 + 2 * GAP, 6));
    let red = |column: usize| sheet.pixels()[column * 4];
    assert_eq!(
        (red(0), red(6 + GAP), red(2 * (6 + GAP))),
        (0.25, 0.5, 1.0),
        "the level is the time, held to one"
    );
    assert_eq!(red(6), red(6 + GAP - 1), "the gap is one colour");
    assert_ne!(red(6), 0.25, "and not a cell's");
}

#[test]
fn a_moment_no_clip_draws_is_a_black_cell_of_the_others_size() {
    let mut piece = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
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
        .clip("late", Flat::new().0, 1.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip_mut("late")
        .and_then(|c| c.set(Values::from([("level".to_string(), Value::Number(0.5))])))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let sheet = sheet_of(&piece, &[0.0, 1.5], 6).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((sheet.width(), sheet.height()), (2 * 6 + GAP, 6));
    assert_eq!(
        &sheet.pixels()[..4],
        &[0.0, 0.0, 0.0, 1.0],
        "black before the clip starts"
    );
    assert_eq!(sheet.pixels()[(6 + GAP) * 4], 0.5);
}

#[test]
fn a_sheet_that_cannot_be_made_is_refused_naming_why() {
    let piece = piece(4.0, Flat::new().0);
    let invalid = |r: Result<Frame, Error>| matches!(r, Err(Error::Invalid { .. }));
    assert!(invalid(sheet_of(&piece, &[], 6)), "no moments");
    assert!(invalid(sheet_of(&piece, &[0.5], 1)), "a cell of one pixel");
    for t in [-0.1, 2.0, 3.0, f64::NAN] {
        assert!(
            invalid(sheet_of(&piece, &[0.5, t], 6)),
            "{t} s is outside the two-second piece"
        );
    }
}

#[test]
fn a_sheet_in_columns_wraps_into_rows_and_pads_the_last_row_with_the_background() {
    let piece = piece(4.0, Flat::new().0);
    let at = [0.0, 0.25, 0.5, 0.75, 1.0];
    let sheet = sheet_in(&piece, &at, 6, Some(2)).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        (sheet.width(), sheet.height()),
        (2 * 6 + GAP, 3 * 6 + 2 * GAP),
        "3 rows of 2 columns"
    );
    let last_row = (2 * (6 + GAP)) * sheet.width() * 4;
    let second_cell = last_row + (6 + GAP) * 4;
    assert_eq!(
        sheet.pixels()[second_cell..second_cell + 4],
        sheet.pixels()[4 * 6..4 * 6 + 4],
        "the missing sixth cell is the gap's colour"
    );
    assert!(sheet_in(&piece, &at, 6, Some(0)).is_err(), "no columns");
}

#[test]
fn a_moment_at_the_end_is_refused_naming_the_last_frame_which_is_accepted() {
    let piece = piece(4.0, Flat::new().0);
    assert!(
        sheet_of(&piece, &[1.75], 6).is_ok(),
        "the last frame of the two-second piece"
    );
    let Err(Error::Invalid { message }) = sheet_of(&piece, &[2.0], 6) else {
        unreachable!("accepted")
    };
    assert!(message.contains("1.75"), "{message}");
}

#[test]
fn cells_of_different_sizes_are_refused_naming_both() {
    let mut piece = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
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
        .clip("a", Flat::steps_down_at(0.5), 0.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip_mut("a")
        .and_then(|c| c.map("level", Arc::new(FnSignal(Box::new(|t| t))), 0.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let Err(Error::Port { message, .. }) = sheet_of(&piece, &[0.25, 1.0], 6) else {
        unreachable!("accepted")
    };
    assert!(message.contains("6 by 6") && message.contains("5 by 5"), "{message}");
}

#[test]
fn a_frame_equal_to_the_one_before_is_drawn_once_and_handed_over_again() {
    for workers in [1, 4] {
        // a clip with one constant value: eight frames, one drawing
        let (picture, drawn) = Flat::new();
        let mut held = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
        held.song(
            Arc::new(FakeMusic::of(2.0)),
            &FakeSound::of(2.0),
            &Span::Whole,
            0.0,
            0.0,
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        held.clip("a", picture, 0.0, None)
            .unwrap_or_else(|e| unreachable!("{e}"));
        held.clip_mut("a")
            .and_then(|c| c.set(Values::from([("level".to_string(), Value::Number(0.5))])))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let encoder = Collect::default();
        let stats = render(&held, &encoder, workers).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(levels(&encoder), vec![0.5; 8], "{workers} workers");
        assert_eq!(
            (stats, drawn.load(Ordering::SeqCst)),
            (
                RenderStats {
                    frames: 8,
                    drawn: 1,
                    composed: 0
                },
                1
            ),
            "{workers} workers"
        );

        // the level is the time, held at one from one second on: four different frames, then four the same
        let (picture, drawn) = Flat::new();
        let encoder = Collect::default();
        let stats = render(&piece(4.0, picture), &encoder, workers).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(levels(&encoder), vec![0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(
            (stats.frames, stats.drawn, drawn.load(Ordering::SeqCst)),
            (8, 5, 5),
            "{workers} workers"
        );
    }
}

#[test]
fn a_draft_is_the_same_piece_smaller_and_at_a_lower_rate_and_leaves_the_original_alone() {
    let mut full = Piece::new(Profile::square(8, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
    full.song(
        Arc::new(FakeMusic::of(2.0)),
        &FakeSound::of(2.0),
        &Span::Whole,
        0.0,
        0.0,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    full.clip("a", Flat::new().0, 0.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    full.clip_mut("a")
        .and_then(|c| c.map("level", Arc::new(FnSignal(Box::new(|t| t))), 0.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let draft = full.draft(0.5, Some(2.0)).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        (draft.profile().width(), draft.profile().height(), draft.profile().fps()),
        (4, 4, 2.0)
    );
    assert_eq!(
        (draft.frame_count(), full.frame_count()),
        (Ok(4), Ok(8)),
        "two seconds at two a second and at four a second"
    );
    let encoder = Collect::default();
    render(&draft, &encoder, 2).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        levels(&encoder),
        vec![0.0, 0.5, 1.0, 1.0],
        "the same mapping, read at the draft's moments"
    );
    assert_eq!(
        full.frame(0.5).map(|f| (f.width(), f.height())),
        Ok((8, 8)),
        "the original is as it was"
    );
    assert_eq!(
        full.draft(0.5, None).map(|d| d.profile().fps()),
        Ok(4.0),
        "no rate given: the rate is kept"
    );
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            matches!(full.draft(scale, None), Err(Error::Invalid { .. })),
            "scale {scale}"
        );
    }
    assert!(
        matches!(full.draft(0.5, Some(0.0)), Err(Error::Invalid { .. })),
        "a rate of zero"
    );
}

/// A picture of one grey whose `angle` is a cyclic turn of 0 to 360: the level is the turn's fraction.
struct Turn;

impl Picture for Turn {
    fn parameters(&self) -> Result<Vec<Parameter>, Error> {
        Ok(vec![parameter("angle", number(0.0, 360.0, true, false))])
    }

    #[expect(clippy::cast_possible_truncation, reason = "a test level")]
    fn render(&self, values: &Values, long_side: u32) -> Result<Frame, Error> {
        let Some(Value::Number(angle)) = values.get("angle") else {
            return Err(Error::Port {
                port: "picture",
                message: "no angle".into(),
            });
        };
        let side = long_side as usize;
        Frame::new(side, side, [(*angle / 360.0) as f32, 0.0, 0.0, 1.0].repeat(side * side))
    }
}

/// A looped two-second piece with one clip `a` of `picture`, `address` driven by `signal` of the time.
fn looped(picture: Arc<dyn Picture>, address: &str, signal: impl Fn(f64) -> f64 + Send + Sync + 'static) -> Piece {
    let mut piece = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
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
        .and_then(|c| c.map(address, Arc::new(FnSignal(Box::new(signal))), 0.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece.set_looped(true).unwrap_or_else(|e| unreachable!("{e}"));
    piece
}

fn refusal(piece: &Piece) -> String {
    match render(piece, &Collect::default(), 1) {
        Err(Error::Invalid { message }) => message,
        other => unreachable!("expected a refusal, got {:?}", other.map(|s| s.frames)),
    }
}

#[test]
fn a_looped_piece_whose_mappings_close_renders() {
    // a sine of one whole turn over the two seconds: the same at both ends
    let piece = looped(Flat::new().0, "level", |t| 0.5 + 0.5 * (std::f64::consts::PI * t).sin());
    let encoder = Collect::default();
    let stats = render(&piece, &encoder, 2).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(stats.frames, 8, "a loop has as many frames as the same piece unlooped");
}

#[test]
fn a_seam_that_differs_names_the_clip_the_address_and_both_values_before_any_frame_is_drawn() {
    let (picture, drawn) = Flat::new();
    let piece = looped(picture, "level", |t| t / 4.0);
    let message = refusal(&piece);
    assert!(
        message.contains("`level` of clip `a` is 0 at 0 s and 0.5 at 2 s"),
        "{message}"
    );
    assert_eq!(drawn.load(Ordering::SeqCst), 0, "the check runs before the first frame");
}

#[test]
fn a_cyclic_address_that_turns_a_whole_circle_closes() {
    let piece = looped(Arc::new(Turn), "angle", |t| t * 180.0);
    render(&piece, &Collect::default(), 1).unwrap_or_else(|e| unreachable!("{e}"));
    let half = looped(Arc::new(Turn), "angle", |t| t * 90.0);
    assert!(refusal(&half).contains("`angle`"));
}

#[test]
fn a_cyclic_value_a_hair_short_of_the_wrap_closes() {
    let piece = looped(Arc::new(Turn), "angle", |t| t * 180.0 - 1e-12);
    render(&piece, &Collect::default(), 1).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_looped_piece_with_two_clips_or_a_fade_is_refused_naming_the_fix() {
    let periodic = |t: f64| (std::f64::consts::PI * t).sin();
    let mut two = looped(Flat::new().0, "level", periodic);
    two.clip("b", Flat::new().0, 1.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(refusal(&two).contains("one clip"), "two clips");
    for (fade_in, fade_out) in [(0.5, 0.0), (0.0, 0.5)] {
        let mut faded = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
        faded
            .song(
                Arc::new(FakeMusic::of(2.0)),
                &FakeSound::of(2.0),
                &Span::Whole,
                fade_in,
                fade_out,
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
        faded
            .clip("a", Flat::new().0, 0.0, None)
            .unwrap_or_else(|e| unreachable!("{e}"));
        faded.set_looped(true).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(refusal(&faded).contains("fade"), "fades {fade_in} {fade_out}");
    }
}

#[test]
fn a_looped_draft_keeps_the_loop() {
    let piece = looped(Flat::new().0, "level", |t| t / 4.0);
    let draft = piece.draft(0.5, None).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(refusal(&draft).contains("`level`"));
}

#[test]
fn a_camera_signal_that_differs_at_the_seam_is_named() {
    struct Passthrough;
    impl bullterrier_video::ports::Compose for Passthrough {
        fn compose(&self, layers: &[bullterrier_video::ports::Layer<'_>]) -> Result<Frame, Error> {
            Ok(layers[0].frame.clone())
        }
    }
    let mut piece = looped(Flat::new().0, "level", |_| 0.5);
    piece.compose(Arc::new(Passthrough));
    piece
        .camera(Some(Arc::new(FnSignal(Box::new(|t| t / 10.0)))), None, None, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let message = refusal(&piece);
    assert!(message.contains("camera") && message.contains("dx"), "{message}");
    piece
        .camera(None, None, None, Some(Arc::new(FnSignal(Box::new(|t| t * 180.0)))))
        .unwrap_or_else(|e| unreachable!("{e}"));
    render(&piece, &Collect::default(), 1).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_looped_clip_that_does_not_cover_the_piece_is_refused() {
    let mut piece = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
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
        .clip("a", Flat::new().0, 0.0, Some(1.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip_mut("a")
        .and_then(|c| c.set(Values::from([("level".to_string(), Value::Number(0.5))])))
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece.set_looped(true).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(refusal(&piece).contains("cover"));
}

/// A looped piece of a 2.1 s song at 4 frames a second (8.4 frames), `angle` turning 360 degrees over `turn` seconds.
fn song_of_whole_and_a_bit(turn: f64, looped_piece: bool) -> Piece {
    let mut piece = Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")));
    piece.set_looped(looped_piece).unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .song(
            Arc::new(FakeMusic::of(2.1)),
            &FakeSound::of(2.1),
            &Span::Whole,
            0.0,
            0.0,
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip("a", Arc::new(Turn), 0.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .clip_mut("a")
        .and_then(|c| c.map("angle", Arc::new(FnSignal(Box::new(move |t| 360.0 * t / turn))), 0.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
}

#[test]
fn a_looped_piece_is_a_whole_number_of_frames_long_and_its_sound_is_cut_to_it() {
    let piece = song_of_whole_and_a_bit(2.0, true);
    assert_eq!(piece.duration().unwrap_or_else(|e| unreachable!("{e}")), 2.0);
    assert_eq!(piece.frame_count().unwrap_or_else(|e| unreachable!("{e}")), 8);
    let encoder = Collect::default();
    render(&piece, &encoder, 1).unwrap_or_else(|e| unreachable!("{e}"));
    let wrote = encoder.wrote.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let sound = wrote.and_then(|w| w.3).map(|a| a.2[0]);
    assert_eq!(sound, Some(2.0), "the soundtrack is as long as the frames");
    // the last frame is one step short of the turn, not a fraction of one
    let last = levels(&encoder)[7];
    assert!((f64::from(last) - 0.875).abs() < 1e-6, "{last}");
}

#[test]
fn an_unlooped_piece_keeps_the_songs_own_length() {
    let piece = song_of_whole_and_a_bit(2.0, false);
    assert_eq!(piece.duration().unwrap_or_else(|e| unreachable!("{e}")), 2.1);
}

#[test]
fn a_looped_draft_at_another_rate_snaps_to_that_rates_frames() {
    let draft = song_of_whole_and_a_bit(2.0, true)
        .draft(1.0, Some(2.6))
        .unwrap_or_else(|e| unreachable!("{e}"));
    // 2.0 s is 5.2 frames at 2.6 a second: 5 whole ones
    assert_eq!(draft.frame_count().unwrap_or_else(|e| unreachable!("{e}")), 5);
    assert!((draft.duration().unwrap_or_else(|e| unreachable!("{e}")) - 5.0 / 2.6).abs() < 1e-12);
}
