//! The camera and transitions through a counting fake of the compose port: what is composed, when, and with what.
#![expect(
    clippy::float_cmp,
    reason = "the fakes' levels and masks are exact: 0, 1 and the values written"
)]

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use bullterrier_video::ports::{Blend, Compose, Encode, Job, Layer, Picture, Signal};
use bullterrier_video::{
    Edge, Error, Frame, Parameter, Piece, Profile, RenderStats, Seconds, TransitionKind, Value, Values, View,
};

use crate::common::fakes::{FnSignal, number, parameter};

/// A picture of one flat colour: its `level` in red.
struct Flat;

impl Picture for Flat {
    fn parameters(&self) -> Result<Vec<Parameter>, Error> {
        Ok(vec![parameter("level", number(0.0, 1.0, false, false))])
    }

    #[expect(clippy::cast_possible_truncation, reason = "a test level")]
    fn render(&self, values: &Values, long_side: u32) -> Result<Frame, Error> {
        let Some(Value::Number(level)) = values.get("level") else {
            return Err(Error::Port {
                port: "picture",
                message: "no level".into(),
            });
        };
        let side = long_side as usize;
        Frame::new(side, side, [*level as f32, 0.0, 0.0, 1.0].repeat(side * side))
    }
}

/// What one layer of a call looked like.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    level: f32,
    view: View,
    opacity: f64,
    blend: Blend,
    /// The mask's red values, if the layer had one.
    mask: Option<Vec<f32>>,
}

/// A compose that keeps every call's layers and answers with its first layer's frame.
#[derive(Default)]
struct Tape(Mutex<Vec<Vec<Seen>>>);

impl Tape {
    fn calls(&self) -> Vec<Vec<Seen>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Compose for Tape {
    fn compose(&self, layers: &[Layer<'_>]) -> Result<Frame, Error> {
        let seen = layers
            .iter()
            .map(|l| Seen {
                level: l.frame.pixels()[0],
                view: l.view,
                opacity: l.opacity,
                blend: l.blend,
                mask: l.mask.map(|m| m.pixels().iter().step_by(4).copied().collect()),
            })
            .collect();
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(seen);
        Ok(layers[0].frame.clone())
    }
}

/// An encoder that keeps the first value of every frame.
#[derive(Default)]
struct Reds(Mutex<Vec<f32>>);

impl Encode for Reds {
    fn encode(&self, _job: &Job<'_>, frames: &mut dyn Iterator<Item = Result<Frame, Error>>) -> Result<(), Error> {
        for frame in frames {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(frame?.pixels()[0]);
        }
        Ok(())
    }
}

fn constant(piece: &mut Piece, clip: &str, level: f64) {
    piece
        .clip_mut(clip)
        .and_then(|c| c.set(Values::from([("level".to_string(), Value::Number(level))])))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn square() -> Piece {
    Piece::new(Profile::square(4, 4.0).unwrap_or_else(|e| unreachable!("{e}")))
}

/// Four frames a second on a 4 by 4 frame: clip `a` 0 to 1.5 s at level 0.2, clip `b` 1 to 3 s at level 0.8, and a tape.
fn two_clips() -> (Piece, Arc<Tape>) {
    let mut piece = square();
    piece
        .clip("a", Arc::new(Flat), 0.0, Some(1.5))
        .and_then(|()| piece.clip("b", Arc::new(Flat), 1.0, Some(3.0)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    constant(&mut piece, "a", 0.2);
    constant(&mut piece, "b", 0.8);
    let tape = Arc::new(Tape::default());
    piece.compose(tape.clone());
    (piece, tape)
}

/// One clip of three seconds at level 0.5 and a tape.
fn one_clip() -> (Piece, Arc<Tape>) {
    let mut piece = square();
    piece
        .clip("a", Arc::new(Flat), 0.0, Some(3.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    constant(&mut piece, "a", 0.5);
    let tape = Arc::new(Tape::default());
    piece.compose(tape.clone());
    (piece, tape)
}

fn signal(function: impl Fn(Seconds) -> f64 + Send + Sync + 'static) -> Arc<dyn Signal> {
    Arc::new(FnSignal(Box::new(function)))
}

fn render(piece: &Piece) -> (RenderStats, Vec<f32>) {
    let reds = Reds::default();
    let stats = piece
        .render(&reds, Path::new("out.mp4"), 1)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let seen = reds.0.lock().unwrap_or_else(PoisonError::into_inner).clone();
    (stats, seen)
}

fn sweep(edge: Edge, angle: f64, feather: f64) -> TransitionKind {
    TransitionKind::Sweep { edge, angle, feather }
}

fn mask(layer: &Seen) -> Vec<f32> {
    layer.mask.clone().unwrap_or_else(|| unreachable!("no mask"))
}

#[test]
fn no_camera_and_an_identity_camera_never_call_compose() {
    let (piece, tape) = one_clip();
    let (stats, _) = render(&piece);
    assert_eq!((stats.frames, stats.composed, tape.calls().len()), (12, 0, 0));

    let (mut piece, tape) = one_clip();
    piece
        .camera(
            Some(signal(|_| 0.0)),
            None,
            Some(signal(|_| 1.0)),
            Some(signal(|_| 0.0)),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (stats, _) = render(&piece);
    assert_eq!(
        (stats.composed, tape.calls().len()),
        (0, 0),
        "a camera at rest costs nothing"
    );
}

#[test]
fn a_moving_camera_composes_every_frame_with_its_view_and_a_static_one_costs_one() {
    let (mut piece, tape) = one_clip();
    piece
        .camera(
            Some(signal(|t| t / 10.0)),
            Some(signal(|_| 0.25)),
            Some(signal(|t| 1.0 + t)),
            Some(signal(|_| 5.0)),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (stats, _) = render(&piece);
    let calls = tape.calls();
    assert_eq!(
        (stats.frames, stats.drawn, stats.composed, calls.len()),
        (12, 12, 12, 12)
    );
    // frame 4 is at one second
    assert_eq!(
        calls[4][0].view,
        View {
            dx: 0.1,
            dy: 0.25,
            zoom: 2.0,
            rotate: 5.0
        }
    );

    let (mut piece, tape) = one_clip();
    piece
        .camera(None, None, Some(signal(|_| 1.5)), None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (stats, _) = render(&piece);
    assert_eq!(
        (stats.frames, stats.drawn, stats.composed, tape.calls().len()),
        (12, 1, 1, 1)
    );
}

#[test]
fn a_camera_needs_a_compose_and_a_zoom_above_zero() {
    let mut bare = square();
    bare.clip("a", Arc::new(Flat), 0.0, Some(1.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let Err(Error::Invalid { message }) = bare.camera(None, None, Some(signal(|_| 2.0)), None) else {
        unreachable!("accepted")
    };
    assert!(message.contains("compose"), "{message}");

    let (mut piece, _) = one_clip();
    piece
        .camera(None, None, Some(signal(|_| 0.0)), None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let Err(Error::Invalid { message }) = piece.frame(0.0) else {
        unreachable!("drawn")
    };
    assert!(message.contains("zoom"), "{message}");
}

#[test]
fn clips_overlap_only_inside_a_transition_and_the_message_names_the_fix() {
    let (piece, _) = two_clips();
    let Err(Error::Invalid { message }) = piece.frame(0.0) else {
        unreachable!("accepted")
    };
    assert!(
        message.contains("transition") && message.contains("`a`") && message.contains("`b`"),
        "{message}"
    );

    // a window shorter than the overlap does not cover it
    let (mut short, _) = two_clips();
    short
        .transition("a", "b", 1.25, 0.25, TransitionKind::Dissolve)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(short.frame(0.0), Err(Error::Invalid { .. })));

    let (mut covered, _) = two_clips();
    covered
        .transition("a", "b", 1.25, 0.5, TransitionKind::Dissolve)
        .unwrap_or_else(|e| unreachable!("{e}"));
    covered.frame(0.0).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_dissolve_fades_the_second_clip_in_by_its_progress() {
    let (mut piece, tape) = two_clips();
    piece
        .transition("a", "b", 1.25, 0.5, TransitionKind::Dissolve)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (stats, reds) = render(&piece);
    let calls = tape.calls();
    // the frames at 1.0 s and 1.25 s are inside the window [1.0, 1.5)
    assert_eq!((stats.composed, calls.len()), (2, 2));
    let opacities: Vec<f64> = calls.iter().map(|c| c[1].opacity).collect();
    assert_eq!(opacities, vec![0.0, 0.5]);
    assert!(
        calls
            .iter()
            .all(|c| c.len() == 2 && c[0].opacity == 1.0 && c[0].level == 0.2 && c[1].level == 0.8)
    );
    // before the window the first clip alone, after it the second, none of them composed
    assert_eq!((reds[3], reds[6]), (0.2, 0.8));
}

#[test]
fn a_cut_switches_at_the_middle_of_its_window_without_composing() {
    let (mut piece, tape) = two_clips();
    piece
        .transition("a", "b", 1.25, 0.5, TransitionKind::Cut)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (stats, reds) = render(&piece);
    assert_eq!((stats.composed, tape.calls().len()), (0, 0));
    assert_eq!(&reds[3..8], &[0.2, 0.2, 0.8, 0.8, 0.8]);
}

#[test]
fn a_sweep_starts_with_the_first_clip_alone_and_moves_a_soft_edge_across() {
    let (mut piece, tape) = two_clips();
    piece
        .transition("a", "b", 1.25, 0.5, sweep(Edge::Light, 0.0, 0.5))
        .unwrap_or_else(|e| unreachable!("{e}"));
    render(&piece);
    let calls = tape.calls();
    let [start, middle] = &calls[..] else {
        unreachable!("{calls:?}")
    };
    // first clip, second clip masked, and the light under screen
    assert_eq!(start.len(), 3);
    assert!(
        mask(&start[1]).iter().all(|&m| m == 0.0),
        "the second clip shows nowhere"
    );
    assert!(mask(&start[2]).iter().all(|&m| m == 0.0), "the light is off");
    assert_eq!((start[2].blend, start[1].blend), (Blend::Screen, Blend::Normal));
    // half way the edge is in the middle: by hand, a column's distance from the middle is -0.375, -0.125, 0.125, 0.375 long sides,
    // u = 0.5 - distance / 0.5 = 1, 0.75, 0.25, 0 (held to 0..1) and the mask is the smoothstep of u: 1, 0.84375, 0.15625, 0
    let close = |got: &[f32], want: [f32; 4]| got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-6);
    assert!(
        close(&mask(&middle[1])[..4], [1.0, 0.84375, 0.15625, 0.0]),
        "{:?}",
        mask(&middle[1])
    );
    // the light is brightest on the edge: 1 - |2 m - 1|
    assert!(
        close(&mask(&middle[2])[..4], [0.0, 0.3125, 0.3125, 0.0]),
        "{:?}",
        mask(&middle[2])
    );
}

#[test]
fn a_dark_sweep_is_an_ordinary_layer_and_the_mask_ends_covering_everything() {
    let (mut piece, tape) = two_clips();
    piece
        .transition("a", "b", 1.25, 0.5, sweep(Edge::Dark, 0.0, 0.5))
        .unwrap_or_else(|e| unreachable!("{e}"));
    // the last instant before the window's end
    piece.frame(1.5 - 1e-9).unwrap_or_else(|e| unreachable!("{e}"));
    let calls = tape.calls();
    assert_eq!(calls[0][2].blend, Blend::Normal);
    assert!(
        mask(&calls[0][1]).iter().all(|&m| m > 0.999),
        "{:?}",
        mask(&calls[0][1])
    );
}

#[test]
fn a_sweep_at_an_angle_spreads_along_it() {
    let (mut piece, tape) = two_clips();
    piece
        .transition("a", "b", 1.25, 0.5, sweep(Edge::Light, 90.0, 0.25))
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece.frame(1.25).unwrap_or_else(|e| unreachable!("{e}"));
    // downwards: the top row is shown, the bottom row is not
    let m = mask(&tape.calls()[0][1]);
    assert!(
        m[..4].iter().all(|&v| v == 1.0) && m[12..].iter().all(|&v| v == 0.0),
        "{m:?}"
    );
}

#[test]
fn a_transition_refuses_what_cannot_work() {
    let (mut piece, _) = two_clips();
    for (a, b, at, length) in [
        ("a", "zz", 1.0, 0.5),
        ("a", "a", 1.0, 0.5),
        ("a", "b", 1.0, 0.0),
        ("a", "b", f64::NAN, 0.5),
    ] {
        assert!(
            matches!(
                piece.transition(a, b, at, length, TransitionKind::Dissolve),
                Err(Error::Invalid { .. })
            ),
            "{a} {b} {at} {length}"
        );
    }
    piece
        .transition("a", "b", 1.25, 0.5, TransitionKind::Dissolve)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let Err(Error::Invalid { message }) = piece.transition("b", "a", 1.3, 0.5, TransitionKind::Cut) else {
        unreachable!("two windows at once")
    };
    assert!(message.contains("overlap"), "{message}");
    assert!(matches!(
        piece.transition("a", "b", 2.5, 0.2, sweep(Edge::Light, f64::NAN, 0.1)),
        Err(Error::Invalid { .. })
    ));
}

#[test]
fn a_camera_moves_both_clips_of_a_transition_by_the_same_view() {
    let (mut piece, tape) = two_clips();
    piece
        .transition("a", "b", 1.25, 0.5, TransitionKind::Dissolve)
        .unwrap_or_else(|e| unreachable!("{e}"));
    piece
        .camera(None, None, Some(signal(|_| 2.0)), None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (stats, _) = render(&piece);
    // four different drawings (the first clip held, two in the window, the second held), one call each
    assert_eq!((stats.drawn, stats.composed, tape.calls().len()), (4, 4, 4));
    assert!(tape.calls().iter().all(|c| c.iter().all(|l| l.view.zoom == 2.0)));
}
