//! The picture port: what video needs of whatever draws one frame.

use crate::error::Error;
use crate::types::{Frame, Parameter, Values, View};

/// Something that draws one picture for given values: a scene, a drawing, a caller's function.
///
/// A picture is **one long-lived instance, rendered again with new values each frame**. Whatever it caches (scene keeps the
/// layers that did not change: 0.19 s a frame against 0.95 s without) lives inside it between calls, so a caller never
/// rebuilds it per frame. `render` takes `&self` and the trait is `Send + Sync`: frames may be drawn from several threads at
/// once, and the picture decides how to share its cache between them.
pub trait Picture: Send + Sync {
    /// Everything a caller may set, with unit, range and whether it is cyclic. The same list for the picture's whole life.
    ///
    /// # Errors
    /// [`Error::Port`] when the picture cannot describe itself.
    fn parameters(&self) -> Result<Vec<Parameter>, Error>;

    /// The frame for `values` (an address not set keeps the picture's own value), seen through `view`, whose long side is
    /// `long_side` pixels. Values are in each parameter's own unit; a pixel unit is read at the picture's own design size, so
    /// the same values at another `long_side` give the same picture, only sharper.
    ///
    /// # Errors
    /// [`Error::Port`] when the picture refuses a value or cannot draw.
    fn render(&self, values: &Values, view: View, long_side: u32) -> Result<Frame, Error>;
}
