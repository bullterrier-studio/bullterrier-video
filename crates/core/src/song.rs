//! A song in a piece's own time: the cut soundtrack and what the music knows, seconds from the start of the piece.

use std::sync::Arc;

use crate::error::Error;
use crate::ports::{Music, Sound};
use crate::profile::whole_frames;
use crate::span::Span;
use crate::types::{Band, Hit, Seconds, Section};

/// The levels of one band: `values` between zero and one at `rate` samples a second, the first at `start` seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Levels {
    /// The samples.
    pub values: Vec<f64>,
    /// Samples a second.
    pub rate: f64,
    /// The time of the first sample, in piece seconds.
    pub start: Seconds,
}

/// A song cut to a span. Every time it gives is in **piece seconds**: the span's start is zero.
#[derive(Clone)]
pub struct Song {
    music: Arc<dyn Music>,
    soundtrack: Arc<dyn Sound>,
    start: Seconds,
    end: Seconds,
    faded: bool,
}

impl Song {
    /// Cuts `sound` to `span` (found with `music`), fades it in and out and, when `loudness` is given, sets its loudness.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the span cannot be placed or a fade is negative; [`Error::Port`] from the music or the sound.
    pub fn new(
        music: Arc<dyn Music>,
        sound: &dyn Sound,
        span: &Span,
        fade_in: Seconds,
        fade_out: Seconds,
        loudness: Option<f64>,
    ) -> Result<Self, Error> {
        if !(fade_in.is_finite() && fade_out.is_finite() && fade_in >= 0.0 && fade_out >= 0.0) {
            return Err(Error::Invalid {
                message: format!("fades are seconds, zero or more, got {fade_in} and {fade_out}"),
            });
        }
        let (start, end) = span.resolve(music.as_ref())?;
        let mut cut = sound.cut(start, end)?.fade(fade_in, fade_out)?;
        if let Some(lufs) = loudness {
            cut = cut.normalised(lufs)?;
        }
        Ok(Self {
            music,
            soundtrack: Arc::from(cut),
            start,
            end,
            faded: fade_in > 0.0 || fade_out > 0.0,
        })
    }

    /// The song cut down to a whole number of frames at `fps`, from its start: what a looped piece plays, so that the sound, the
    /// frames and the loop's period are one length. The end is cut, not rounded up: the next bar's attack stays out of the tail.
    ///
    /// # Errors
    /// [`Error::Port`] from the sound.
    pub(crate) fn snapped(&self, fps: f64) -> Result<Self, Error> {
        let length = whole_frames(self.duration(), fps);
        if length == self.duration() {
            return Ok(self.clone());
        }
        Ok(Self {
            soundtrack: Arc::from(self.soundtrack.cut(0.0, length)?),
            end: self.start + length,
            ..self.clone()
        })
    }

    /// How long the piece's soundtrack lasts.
    #[must_use]
    pub fn duration(&self) -> Seconds {
        self.end - self.start
    }

    /// Whether the song fades in or out.
    pub(crate) fn faded(&self) -> bool {
        self.faded
    }

    /// The cut and faded sound.
    #[must_use]
    pub fn soundtrack(&self) -> &dyn Sound {
        self.soundtrack.as_ref()
    }

    /// The beats inside the span.
    ///
    /// # Errors
    /// [`Error::Port`] from the music.
    pub fn beats(&self) -> Result<Vec<Seconds>, Error> {
        Ok(self.within(self.music.beats()?))
    }

    /// The first beat of each bar inside the span.
    ///
    /// # Errors
    /// [`Error::Port`] when the meter is not known.
    pub fn downbeats(&self) -> Result<Vec<Seconds>, Error> {
        Ok(self.within(self.music.downbeats()?))
    }

    /// The hits of one kind inside the span.
    ///
    /// # Errors
    /// [`Error::Port`] when the hits cannot be found.
    pub fn hits(&self, hit: Hit) -> Result<Vec<Seconds>, Error> {
        Ok(self.within(self.music.hits(hit)?.into_iter().map(|m| m.t).collect()))
    }

    /// The strongest drop inside the span.
    ///
    /// # Errors
    /// [`Error::Invalid`] when there is none; [`Error::Port`] from the music.
    pub fn drop(&self) -> Result<Seconds, Error> {
        let drops = self.music.drops()?;
        drops
            .iter()
            .find(|d| self.start <= d.t && d.t < self.end)
            .map(|d| d.t - self.start)
            .ok_or_else(|| Error::Invalid {
                message: "the song has no drop inside this span".into(),
            })
    }

    /// The beats a minute: sixty over the median gap between beats.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the span holds fewer than two beats.
    pub fn tempo(&self) -> Result<f64, Error> {
        let beats = self.beats()?;
        if beats.len() < 2 {
            return Err(Error::Invalid {
                message: "the span holds fewer than two beats, so it has no tempo".into(),
            });
        }
        let mut gaps: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect();
        gaps.sort_by(f64::total_cmp);
        let median = if gaps.len() % 2 == 1 {
            gaps[gaps.len() / 2]
        } else {
            f64::midpoint(gaps[gaps.len() / 2 - 1], gaps[gaps.len() / 2])
        };
        Ok(60.0 / median)
    }

    /// The sections that overlap the span, cut to it.
    ///
    /// # Errors
    /// [`Error::Port`] from the music.
    pub fn sections(&self) -> Result<Vec<Section>, Error> {
        Ok(self
            .music
            .sections()?
            .into_iter()
            .filter(|s| s.end > self.start && s.start < self.end)
            .map(|s| Section {
                start: s.start.max(self.start) - self.start,
                end: s.end.min(self.end) - self.start,
                label: s.label,
            })
            .collect())
    }

    /// The level of `band` over the whole piece, or over `over` (piece seconds; `None` for its end), scaled by that stretch's
    /// own range.
    ///
    /// # Errors
    /// [`Error::Invalid`] when `over` is empty or outside the piece; [`Error::Port`] from the music.
    pub fn band(&self, band: Band, over: Option<(Seconds, Option<Seconds>)>) -> Result<Levels, Error> {
        let (from, to) = match over {
            None => (0.0, self.duration()),
            Some((from, to)) => (from, to.unwrap_or_else(|| self.duration())),
        };
        if !(from.is_finite() && to.is_finite() && 0.0 <= from && from < to && to <= self.duration()) {
            return Err(Error::Invalid {
                message: format!(
                    "`over` must lie inside the piece's 0 to {} s, got {from} to {to}",
                    self.duration()
                ),
            });
        }
        let (rate, values) = self.music.band(band, self.start + from, self.start + to)?;
        Ok(Levels {
            values,
            rate,
            start: from,
        })
    }

    /// The times inside `[start, end)`, moved into piece time.
    fn within(&self, times: Vec<Seconds>) -> Vec<Seconds> {
        times
            .into_iter()
            .filter(|t| self.start <= *t && *t < self.end)
            .map(|t| t - self.start)
            .collect()
    }
}
