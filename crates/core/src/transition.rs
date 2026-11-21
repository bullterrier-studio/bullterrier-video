//! Transitions between two clips: when they run, how far along they are, and the masks a sweep needs.

use crate::error::Error;
use crate::types::{Frame, Seconds};

/// The bar that rides the edge of a sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// A white light on the edge (screen blend).
    Light,
    /// A black shadow on the edge.
    Dark,
}

/// How one clip gives way to the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransitionKind {
    /// The first clip until the middle of the window, the second after it: no blend, no compose call.
    Cut,
    /// The second clip fades in over the first.
    Dissolve,
    /// A soft edge crosses the frame at `angle` degrees (0: left to right, 90: downwards) and shows the second clip behind it.
    Sweep {
        /// The bar on the edge.
        edge: Edge,
        /// Direction of travel in degrees, clockwise.
        angle: f64,
        /// Width of the soft edge as a fraction of the frame's long side.
        feather: f64,
    },
}

/// A transition from clip `a` to clip `b`, centred on `at` and `length` seconds long.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Window {
    pub(crate) a: usize,
    pub(crate) b: usize,
    pub(crate) at: Seconds,
    pub(crate) length: Seconds,
    pub(crate) kind: TransitionKind,
}

impl Window {
    pub(crate) fn start(&self) -> Seconds {
        self.at - self.length / 2.0
    }

    pub(crate) fn end(&self) -> Seconds {
        self.at + self.length / 2.0
    }

    /// 0 at the start of the window, 1 at its end.
    pub(crate) fn progress(&self, t: Seconds) -> f64 {
        ((t - self.start()) / self.length).clamp(0.0, 1.0)
    }
}

/// What the kind must satisfy to be drawn.
pub(crate) fn check(kind: TransitionKind) -> Result<(), Error> {
    if let TransitionKind::Sweep { angle, feather, .. } = kind
        && !(angle.is_finite() && feather.is_finite() && feather >= 0.0)
    {
        return Err(Error::Invalid {
            message: format!("a sweep needs a finite angle and a feather of 0 or more, got {angle} and {feather}"),
        });
    }
    Ok(())
}

/// The masks of a sweep at `progress` over a frame of `width` by `height`: where the second clip shows (0 everywhere at
/// progress 0, 1 everywhere at 1), and the bar on the edge (0 away from it, 1 on it). Both in the red channel.
pub(crate) fn sweep_masks(
    width: usize,
    height: usize,
    angle: f64,
    feather: f64,
    progress: f64,
) -> Result<(Frame, Frame), Error> {
    #[expect(clippy::cast_precision_loss, reason = "a side is far below 2^52")]
    let (wide, high) = (width as f64, height as f64);
    let long = wide.max(high);
    let (dy, dx) = angle.to_radians().sin_cos();
    let feather = feather.max(1e-6);
    // the half extent of the frame along the direction, in long sides: the edge starts and ends just outside it
    let reach = f64::midpoint(wide * dx.abs(), high * dy.abs()) / long;
    let edge = -reach - feather / 2.0 + progress * (2.0 * reach + feather);
    // a pixel's distance along the direction is the sum of its column's and its row's share: both made once, in single
    // precision for the loop over every pixel
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "a side is far below 2^24"
    )]
    let (columns, rows): (Vec<f32>, Vec<f32>) = (
        (0..width)
            .map(|c| ((c as f64 + 0.5 - wide / 2.0) * dx / long) as f32)
            .collect(),
        (0..height)
            .map(|r| ((r as f64 + 0.5 - high / 2.0) * dy / long) as f32)
            .collect(),
    );
    #[expect(clippy::cast_possible_truncation, reason = "single precision is enough for a mask")]
    let (edge, feather) = (edge as f32, feather as f32);
    let mut shown = Vec::with_capacity(width * height * 4);
    let mut bar = Vec::with_capacity(width * height * 4);
    for row in &rows {
        for column in &columns {
            let along = ((edge - (column + row)) / feather + 0.5).clamp(0.0, 1.0);
            let seen = along * along * (3.0 - 2.0 * along);
            let lit = 1.0 - (2.0 * seen - 1.0).abs();
            shown.extend_from_slice(&[seen, seen, seen, 1.0]);
            bar.extend_from_slice(&[lit, lit, lit, 1.0]);
        }
    }
    Ok((Frame::new(width, height, shown)?, Frame::new(width, height, bar)?))
}
