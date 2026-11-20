//! The music port: what is known about a song, as times and levels.

use crate::error::Error;
use crate::types::{Band, Hit, Moment, Seconds, Section};

/// Measures of one piece of music. Every time is in seconds from the start of the sound; video moves them into the piece's own
/// clock when it chooses a span.
pub trait Music: Send + Sync {
    /// How long the music lasts.
    fn duration(&self) -> Seconds;

    /// Every beat, rising.
    ///
    /// # Errors
    /// [`Error::Port`] when the music cannot be measured.
    fn beats(&self) -> Result<Vec<Seconds>, Error>;

    /// The first beat of each bar, rising.
    ///
    /// # Errors
    /// [`Error::Port`] when the music cannot be measured or its meter is not known: never an empty list that looks like a song
    /// without bars.
    fn downbeats(&self) -> Result<Vec<Seconds>, Error>;

    /// Where the music pays off (a drop after a build), strongest first.
    ///
    /// # Errors
    /// [`Error::Port`] when the music cannot be measured.
    fn drops(&self) -> Result<Vec<Moment>, Error>;

    /// The hits of one kind, rising. A hit needs the drum part found, which may need a model: a source that has none returns an
    /// error naming what is missing, never an empty list that looks like silence.
    ///
    /// # Errors
    /// [`Error::Port`] when the hits cannot be found.
    fn hits(&self, hit: Hit) -> Result<Vec<Moment>, Error>;

    /// The sections, in order.
    ///
    /// # Errors
    /// [`Error::Port`] when the music cannot be measured.
    fn sections(&self) -> Result<Vec<Section>, Error>;

    /// The level of `band` as samples between zero and one at `rate` samples a second, scaled to the song's own range.
    ///
    /// # Errors
    /// [`Error::Port`] when the music cannot be measured.
    fn band(&self, band: Band) -> Result<(f64, Vec<f64>), Error>;
}
