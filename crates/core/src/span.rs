//! Choosing the stretch of a song a piece uses.

use crate::error::Error;
use crate::ports::Music;
use crate::types::{Band, Seconds};

/// Which part of a song becomes the piece.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Span {
    /// `length` seconds starting `before` seconds ahead of the song's strongest drop.
    AroundDrop {
        /// Seconds of build before the drop.
        before: Seconds,
        /// Seconds in all.
        length: Seconds,
    },
    /// `bars` whole bars, starting on the first downbeat, so a loop closes on the beat.
    Bars(u32),
    /// The whole song.
    Whole,
    /// From `start` to `end`.
    Between {
        /// Start, in seconds.
        start: Seconds,
        /// End, in seconds.
        end: Seconds,
    },
    /// The `length` seconds in which `band` is strongest.
    Loudest {
        /// The band to measure.
        band: Band,
        /// Seconds in all.
        length: Seconds,
    },
}

impl Span {
    /// The start and end, in seconds from the start of the song.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the span cannot be placed in this song (no drop, fewer bars than asked, past the end, a
    /// non-positive length); [`Error::Port`] when the music cannot be measured.
    pub fn resolve(&self, music: &dyn Music) -> Result<(Seconds, Seconds), Error> {
        let duration = music.duration();
        match *self {
            Self::AroundDrop { before, length } => {
                positive("length", length)?;
                finite("before", before)?;
                let drop = music
                    .drops()?
                    .first()
                    .map(|d| d.t)
                    .ok_or_else(|| invalid("the song has no drop".into()))?;
                let start = (drop - before).max(0.0);
                inside(start, start + length, duration)
            }
            Self::Bars(bars) => {
                let downbeats = music.downbeats()?;
                let bars = bars as usize;
                if bars == 0 || downbeats.len() <= bars {
                    return Err(invalid(format!(
                        "the song has {} whole bars, {bars} were asked for",
                        downbeats.len().saturating_sub(1)
                    )));
                }
                Ok((downbeats[0], downbeats[bars]))
            }
            Self::Whole => Ok((0.0, duration)),
            Self::Between { start, end } => inside(start, end, duration),
            Self::Loudest { band, length } => {
                positive("length", length)?;
                loudest(music, band, length, duration)
            }
        }
    }
}

/// The window of `length` seconds with the greatest total level, the earliest if two tie.
fn loudest(music: &dyn Music, band: Band, length: Seconds, duration: Seconds) -> Result<(Seconds, Seconds), Error> {
    let (rate, levels) = music.band(band, 0.0, duration)?;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a window is a small whole number of samples"
    )]
    let window = (length * rate).round() as usize;
    if window == 0 || window > levels.len() {
        return Err(invalid(format!(
            "a window of {length} s does not fit the song's {duration} s"
        )));
    }
    let mut sum: f64 = levels[..window].iter().sum();
    let (mut best, mut best_at) = (sum, 0);
    for at in 1..=levels.len() - window {
        sum += levels[at + window - 1] - levels[at - 1];
        if sum > best {
            (best, best_at) = (sum, at);
        }
    }
    #[expect(clippy::cast_precision_loss, reason = "a sample index is far below 2^52")]
    let start = best_at as f64 / rate;
    inside(start, start + length, duration)
}

fn inside(start: Seconds, end: Seconds, duration: Seconds) -> Result<(Seconds, Seconds), Error> {
    if !(start.is_finite() && end.is_finite() && 0.0 <= start && start < end) {
        return Err(invalid(format!("a span needs 0 <= start < end, got {start} to {end}")));
    }
    if end > duration {
        return Err(invalid(format!(
            "the span ends at {end} s, past the song's end at {duration} s"
        )));
    }
    Ok((start, end))
}

fn positive(name: &str, value: f64) -> Result<(), Error> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(invalid(format!("{name} must be a positive number, got {value}")))
    }
}

fn finite(name: &str, value: f64) -> Result<(), Error> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(invalid(format!("{name} must be a finite number, got {value}")))
    }
}

fn invalid(message: String) -> Error {
    Error::Invalid { message }
}
