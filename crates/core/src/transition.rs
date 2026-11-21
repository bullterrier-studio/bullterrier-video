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
    let (w, h) = (width as f64, height as f64);
    let long = w.max(h);
    let (dy, dx) = angle.to_radians().sin_cos();
    let feather = feather.max(1e-6);
    // the half extent of the frame along the direction, in long sides: the edge starts and ends just outside it
    let reach = (w * dx.abs() + h * dy.abs()) / 2.0 / long;
    let edge = -reach - feather / 2.0 + progress * (2.0 * reach + feather);
    let mut shown = Vec::with_capacity(width * height * 4);
    let mut bar = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        #[expect(clippy::cast_precision_loss, reason = "a side is far below 2^52")]
        let y = (row as f64 + 0.5 - h / 2.0) * dy;
        for column in 0..width {
            #[expect(clippy::cast_precision_loss, reason = "a side is far below 2^52")]
            let s = ((column as f64 + 0.5 - w / 2.0) * dx + y) / long;
            let u = ((edge - s) / feather + 0.5).clamp(0.0, 1.0);
            let m = u * u * (3.0 - 2.0 * u);
            #[expect(clippy::cast_possible_truncation, reason = "a mask value is 0 to 1")]
            let (m, b) = (m as f32, (1.0 - (2.0 * m - 1.0).abs()) as f32);
            shown.extend_from_slice(&[m, m, m, 1.0]);
            bar.extend_from_slice(&[b, b, b, 1.0]);
        }
    }
    Ok((Frame::new(width, height, shown)?, Frame::new(width, height, bar)?))
}
