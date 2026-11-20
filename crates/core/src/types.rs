//! Video's own small types: what crosses a port. They name no sibling's type, so a sibling that changes or is replaced
//! touches its adapter only.

use std::collections::BTreeMap;

use crate::error::Error;

/// Time in seconds, everywhere. Frames exist only in the piece's own clock; a port never sees a frame number.
pub type Seconds = f64;

/// One rendered picture: float32 RGBA, straight alpha, rows from the top, `width * height * 4` values.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    width: usize,
    height: usize,
    pixels: Vec<f32>,
}

impl Frame {
    /// A frame from its pixels.
    ///
    /// # Errors
    /// [`Error::FrameShape`] when `pixels` is not `width * height * 4` values or a side is zero.
    pub fn new(width: usize, height: usize, pixels: Vec<f32>) -> Result<Self, Error> {
        let expected = width.checked_mul(height).and_then(|n| n.checked_mul(4));
        if width == 0 || height == 0 || expected != Some(pixels.len()) {
            return Err(Error::FrameShape {
                width,
                height,
                len: pixels.len(),
            });
        }
        Ok(Self { width, height, pixels })
    }

    /// Columns.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Rows.
    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// The values, row by row, four to a pixel.
    #[must_use]
    pub fn pixels(&self) -> &[f32] {
        &self.pixels
    }
}

/// What a parameter takes. Only a number has a range, so a range on a text cannot be written.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Kind {
    /// A number in `min..=max`, in `unit` (`px`, `degrees`, `fraction`, ...; empty for none). A cyclic number wraps into the
    /// range instead of being held to it.
    Number {
        /// The least value.
        min: f64,
        /// The greatest value.
        max: f64,
        /// The unit's name; empty for none.
        unit: String,
        /// Whether a value outside the range wraps round (an angle) instead of stopping at the end.
        cyclic: bool,
    },
    /// On or off.
    Bool,
    /// A piece of text.
    Text,
    /// A picture filled in by a caller (a hole of a template).
    Picture,
}

/// One value a picture takes, by the address of a [`Parameter`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// A number, in the parameter's unit.
    Number(f64),
    /// On or off.
    Bool(bool),
    /// Text.
    Text(String),
    /// A picture.
    Picture(Frame),
}

/// The values to set for one frame, by address.
pub type Values = BTreeMap<String, Value>;

/// One thing a picture lets a caller set: its address, what it means, and what it takes.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    /// Where it is set: the picture's own dotted address (`title.opacity`).
    pub address: String,
    /// What it does, in the picture's terms; may be empty.
    pub meaning: String,
    /// What it takes.
    pub kind: Kind,
}

/// One transform over a whole picture: a camera. All zero and a zoom of one leave the picture as it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Move right, as a fraction of the picture's long side, so a preview and a final render agree.
    pub dx: f64,
    /// Move down, as a fraction of the picture's long side.
    pub dy: f64,
    /// Scale about the centre; one is none.
    pub zoom: f64,
    /// Turn about the centre, in degrees.
    pub rotate: f64,
}

impl View {
    /// The picture as it is.
    pub const IDENTITY: Self = Self {
        dx: 0.0,
        dy: 0.0,
        zoom: 1.0,
        rotate: 0.0,
    };
}

/// A kind of sound event the music port reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Hit {
    /// A kick drum.
    Kick,
    /// A snare.
    Snare,
    /// A hi-hat.
    Hat,
}

/// A band of the spectrum, low to high (the adapter maps it to the library's own bands).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Band {
    /// Lowest rumble.
    Sub,
    /// Bass.
    Bass,
    /// Low middle.
    LowMid,
    /// Middle.
    Mid,
    /// High.
    High,
    /// Highest.
    Air,
}

/// A moment with a strength between zero and one (a kick, a drop).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moment {
    /// When, in seconds from the start of the sound.
    pub t: Seconds,
    /// How strong, `0.0..=1.0`.
    pub strength: f64,
}

/// A named stretch of a song.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// Start, in seconds.
    pub start: Seconds,
    /// End, in seconds.
    pub end: Seconds,
    /// A label shared by similar sections.
    pub label: String,
}
