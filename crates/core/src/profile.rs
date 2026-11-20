//! The output profile: the shape, rate and encoding of the finished video.

use crate::error::Error;

/// What a finished video is: frame size, rate, bit depth, codec, delivery loudness and where titles are safe.
///
/// Sizes are even (the common 4:2:0 formats need it), so a profile that could not be encoded cannot be made.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    width: u32,
    height: u32,
    fps: f64,
    depth: u8,
    codec: String,
    loudness: Option<f64>,
    safe: (f64, f64),
}

impl Profile {
    /// A profile of `width` by `height` at `fps` frames a second: 8 bit, `h264`, no loudness change, the whole height safe.
    ///
    /// # Errors
    /// [`Error::Invalid`] when a side is zero or odd, or `fps` is not a positive finite number.
    pub fn new(width: u32, height: u32, fps: f64) -> Result<Self, Error> {
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(invalid(format!(
                "a profile needs even, non-zero sides, got {width} by {height}"
            )));
        }
        if !(fps.is_finite() && fps > 0.0) {
            return Err(invalid(format!("fps must be a positive number, got {fps}")));
        }
        Ok(Self {
            width,
            height,
            fps,
            depth: 8,
            codec: "h264".into(),
            loudness: None,
            safe: (0.0, 1.0),
        })
    }

    /// 9:16 (a phone held upright): `width` wide, titles safe between 12 and 80 percent of the height.
    ///
    /// # Errors
    /// [`Error::Invalid`] as [`Profile::new`].
    pub fn vertical(width: u32, fps: f64) -> Result<Self, Error> {
        let mut profile = Self::new(width, even(f64::from(width) * 16.0 / 9.0), fps)?;
        profile.safe = (0.12, 0.80);
        Ok(profile)
    }

    /// 16:9, `width` wide.
    ///
    /// # Errors
    /// [`Error::Invalid`] as [`Profile::new`].
    pub fn landscape(width: u32, fps: f64) -> Result<Self, Error> {
        Self::new(width, even(f64::from(width) * 9.0 / 16.0), fps)
    }

    /// Square, `width` on a side.
    ///
    /// # Errors
    /// [`Error::Invalid`] as [`Profile::new`].
    pub fn square(width: u32, fps: f64) -> Result<Self, Error> {
        Self::new(width, width, fps)
    }

    /// The same profile written with `depth` bits a channel.
    ///
    /// # Errors
    /// [`Error::Invalid`] unless `depth` is 8 or 10.
    pub fn with_depth(mut self, depth: u8) -> Result<Self, Error> {
        if depth != 8 && depth != 10 {
            return Err(invalid(format!("depth is 8 or 10 bits, got {depth}")));
        }
        self.depth = depth;
        Ok(self)
    }

    /// The same profile written with `codec` (`h264` or `hevc`; the encoder refuses one it does not have).
    #[must_use]
    pub fn with_codec(mut self, codec: &str) -> Self {
        self.codec = codec.into();
        self
    }

    /// The same profile with the soundtrack set to `lufs` integrated loudness.
    ///
    /// # Errors
    /// [`Error::Invalid`] when `lufs` is not finite or is above zero.
    pub fn with_loudness(mut self, lufs: f64) -> Result<Self, Error> {
        if !(lufs.is_finite() && lufs <= 0.0) {
            return Err(invalid(format!("loudness is LUFS at or below zero, got {lufs}")));
        }
        self.loudness = Some(lufs);
        Ok(self)
    }

    /// Columns.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Rows.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Frames a second.
    #[must_use]
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// Bits a colour channel is written with.
    #[must_use]
    pub fn depth(&self) -> u8 {
        self.depth
    }

    /// The codec by name.
    #[must_use]
    pub fn codec(&self) -> &str {
        &self.codec
    }

    /// The soundtrack's loudness in LUFS, `None` to leave it as it is.
    #[must_use]
    pub fn loudness(&self) -> Option<f64> {
        self.loudness
    }

    /// Where titles are safe, as fractions of the height from the top: `(top, bottom)`.
    #[must_use]
    pub fn safe(&self) -> (f64, f64) {
        self.safe
    }

    /// The long side in pixels: what a picture is asked to draw at so that it fills the frame.
    #[must_use]
    pub fn long_side(&self) -> u32 {
        self.width.max(self.height)
    }
}

/// The nearest even whole number to `x`, at least 2.
fn even(x: f64) -> u32 {
    // constant: halving, rounding and doubling gives the nearest even number
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a side of a video fits u32"
    )]
    let half = (x / 2.0).round().max(1.0) as u32;
    half * 2
}

fn invalid(message: String) -> Error {
    Error::Invalid { message }
}
