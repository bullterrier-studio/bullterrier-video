//! The sound port: what video does to a sound it will put in the soundtrack.

use crate::error::Error;
use crate::types::Seconds;

/// One sound, edited one at a time (cut, fade, level). Putting several on the timeline and mixing them is video's own work,
/// done on the samples this port hands over.
pub trait Sound: Send + Sync {
    /// Samples a second.
    fn rate(&self) -> u32;

    /// Channels.
    fn channels(&self) -> u16;

    /// How long it lasts.
    fn duration(&self) -> Seconds;

    /// The part from `start` to `end`.
    ///
    /// # Errors
    /// [`Error::Port`] when the span is empty or outside the sound.
    fn cut(&self, start: Seconds, end: Seconds) -> Result<Box<dyn Sound>, Error>;

    /// The sound with a fade in and a fade out, each in seconds (zero for none).
    ///
    /// # Errors
    /// [`Error::Port`] when the fades are longer than the sound.
    fn fade(&self, fade_in: Seconds, fade_out: Seconds) -> Result<Box<dyn Sound>, Error>;

    /// The sound at `lufs` integrated loudness.
    ///
    /// # Errors
    /// [`Error::Port`] when the loudness cannot be measured (a silent sound).
    fn normalised(&self, lufs: f64) -> Result<Box<dyn Sound>, Error>;

    /// The samples, interleaved by channel, between minus one and one.
    ///
    /// # Errors
    /// [`Error::Port`] when the sound cannot be read.
    fn samples(&self) -> Result<Vec<f32>, Error>;
}
