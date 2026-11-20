//! The compose port: frames put together into one frame.

use crate::error::Error;
use crate::types::{Frame, View};

/// How a layer joins what is under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Blend {
    /// Over, by its alpha.
    Normal,
    /// Lightens: a light that adds to what is under it without darkening it.
    Screen,
    /// Darkens.
    Multiply,
}

/// One frame placed over the others.
#[derive(Debug, Clone, Copy)]
pub struct Layer<'a> {
    /// The picture.
    pub frame: &'a Frame,
    /// Where it sits over the ground (a camera is the identity view of the ground plus one layer with a view).
    pub view: View,
    /// How much of it shows, `0.0..=1.0`.
    pub opacity: f64,
    /// How it joins what is under it.
    pub blend: Blend,
    /// Where it shows: the red channel of this frame, `0.0..=1.0`, the same size as `frame`; `None` shows it everywhere.
    pub mask: Option<&'a Frame>,
}

/// Puts frames together: tracks (a title over a clip), transitions (two clips and a moving mask, and a light) and the camera
/// (one frame moved). Video decides what and when; this does the pixels.
pub trait Compose: Send + Sync {
    /// The layers over one another, first at the bottom. The result has the size of the first layer's frame; a later layer is
    /// placed over it by its `view`.
    ///
    /// # Errors
    /// [`Error::Port`] when there is no layer or a mask is not the size of its frame.
    fn compose(&self, layers: &[Layer<'_>]) -> Result<Frame, Error>;
}
