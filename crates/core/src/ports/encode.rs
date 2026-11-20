//! The encode port: frames and a soundtrack into a file.

use std::path::Path;

use crate::error::Error;
use crate::types::Frame;

/// What to write: the file, its rate and codec, and the soundtrack. The soundtrack's loudness is set by video before it gets here.
#[derive(Debug, Clone, PartialEq)]
pub struct Job<'a> {
    /// Where the file goes; its extension picks the container (`mp4`, `gif`, ...).
    pub path: &'a Path,
    /// Frames a second.
    pub fps: f64,
    /// Bits a colour channel is written with, 8 or 10.
    pub depth: u8,
    /// The codec by name (`h264`, ...); the encoder refuses one it does not have.
    pub codec: &'a str,
    /// Samples of the soundtrack, interleaved, with its rate and channel count; `None` for a silent file.
    pub audio: Option<Audio<'a>>,
}

/// The soundtrack handed to an encoder.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio<'a> {
    /// Samples a second.
    pub rate: u32,
    /// Channels.
    pub channels: u16,
    /// Interleaved samples between minus one and one.
    pub samples: &'a [f32],
}

/// Writes a video file. One call per file: the frames arrive as a stream, so a clip never has to fit in memory.
pub trait Encode: Send + Sync {
    /// Writes every frame `frames` yields, in order, and the soundtrack in sync with them; stops at the first frame that is an
    /// error and returns it, leaving no half-written file behind.
    ///
    /// # Errors
    /// [`Error::Port`] when the encoder cannot run or the file cannot be written; the frame's own error when a frame fails.
    fn encode(&self, job: &Job<'_>, frames: &mut dyn Iterator<Item = Result<Frame, Error>>) -> Result<(), Error>;
}
