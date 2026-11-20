//! Plain fakes of the ports, behaving as their contracts say, for the tests that need a song or a sound.
#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the fakes hold small positive numbers"
)]

use std::sync::{Arc, Mutex};

use bullterrier_video::ports::{Music, Picture, Signal, Sound};
use bullterrier_video::{Band, Error, Frame, Hit, Kind, Moment, Parameter, Seconds, Section, Values};

/// A song with programmed measures. `levels` is one band at `rate` samples a second for every band.
pub(crate) struct FakeMusic {
    pub(crate) duration: Seconds,
    pub(crate) beats: Vec<Seconds>,
    pub(crate) downbeats: Vec<Seconds>,
    pub(crate) drops: Vec<Moment>,
    pub(crate) kicks: Vec<Seconds>,
    pub(crate) sections: Vec<Section>,
    pub(crate) rate: f64,
    pub(crate) levels: Vec<f64>,
    /// The `(start, end)` of the last `band` call.
    pub(crate) asked: Mutex<Option<(Seconds, Seconds)>>,
}

impl FakeMusic {
    /// A song of `duration` seconds with nothing measured.
    pub(crate) fn of(duration: Seconds) -> Self {
        Self {
            duration,
            beats: vec![],
            downbeats: vec![],
            drops: vec![],
            kicks: vec![],
            sections: vec![],
            rate: 1.0,
            levels: vec![0.0; duration as usize],
            asked: Mutex::new(None),
        }
    }
}

impl Music for FakeMusic {
    fn duration(&self) -> Seconds {
        self.duration
    }
    fn beats(&self) -> Result<Vec<Seconds>, Error> {
        Ok(self.beats.clone())
    }
    fn downbeats(&self) -> Result<Vec<Seconds>, Error> {
        Ok(self.downbeats.clone())
    }
    fn drops(&self) -> Result<Vec<Moment>, Error> {
        Ok(self.drops.clone())
    }
    fn hits(&self, hit: Hit) -> Result<Vec<Moment>, Error> {
        match hit {
            Hit::Kick => Ok(self.kicks.iter().map(|&t| Moment { t, strength: 1.0 }).collect()),
            _ => Err(Error::Port {
                port: "music",
                message: format!("no {hit:?} found"),
            }),
        }
    }
    fn sections(&self) -> Result<Vec<Section>, Error> {
        Ok(self.sections.clone())
    }
    /// The samples between `start` and `end`, stretched to the range zero to one by their own minimum and maximum.
    fn band(&self, _band: Band, start: Seconds, end: Seconds) -> Result<(f64, Vec<f64>), Error> {
        *self.asked.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((start, end));
        let (from, to) = ((start * self.rate).round() as usize, (end * self.rate).round() as usize);
        let part = &self.levels[from..to.min(self.levels.len())];
        let (low, high) = part
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), &v| (l.min(v), h.max(v)));
        let scale = if high > low { high - low } else { 1.0 };
        Ok((self.rate, part.iter().map(|v| (v - low) / scale).collect()))
    }
}

/// A sound that remembers what was done to it: `samples()` is `[duration, fade in, fade out, lufs]` (lufs is -1 if none).
#[derive(Clone)]
pub(crate) struct FakeSound {
    pub(crate) duration: Seconds,
    pub(crate) fade_in: Seconds,
    pub(crate) fade_out: Seconds,
    pub(crate) lufs: Option<f64>,
}

impl FakeSound {
    pub(crate) fn of(duration: Seconds) -> Self {
        Self {
            duration,
            fade_in: 0.0,
            fade_out: 0.0,
            lufs: None,
        }
    }
}

impl Sound for FakeSound {
    fn rate(&self) -> u32 {
        4
    }
    fn channels(&self) -> u16 {
        1
    }
    fn duration(&self) -> Seconds {
        self.duration
    }
    fn cut(&self, start: Seconds, end: Seconds) -> Result<Box<dyn Sound>, Error> {
        Ok(Box::new(Self {
            duration: end - start,
            ..self.clone()
        }))
    }
    fn fade(&self, fade_in: Seconds, fade_out: Seconds) -> Result<Box<dyn Sound>, Error> {
        Ok(Box::new(Self {
            fade_in,
            fade_out,
            ..self.clone()
        }))
    }
    fn normalised(&self, lufs: f64) -> Result<Box<dyn Sound>, Error> {
        Ok(Box::new(Self {
            lufs: Some(lufs),
            ..self.clone()
        }))
    }
    fn samples(&self) -> Result<Vec<f32>, Error> {
        Ok(vec![
            self.duration as f32,
            self.fade_in as f32,
            self.fade_out as f32,
            self.lufs.map_or(-1.0, |l| l as f32),
        ])
    }
}

/// A picture that describes itself and draws nothing: enough for what a clip checks.
pub(crate) struct Described(pub(crate) Vec<Parameter>);

impl Picture for Described {
    fn parameters(&self) -> Result<Vec<Parameter>, Error> {
        Ok(self.0.clone())
    }
    fn render(&self, _values: &Values, _long_side: u32) -> Result<Frame, Error> {
        Err(Error::Port {
            port: "picture",
            message: "draws nothing".into(),
        })
    }
}

/// A parameter of a picture, with no meaning written.
pub(crate) fn parameter(address: &str, kind: Kind) -> Parameter {
    Parameter {
        address: address.into(),
        meaning: String::new(),
        kind,
    }
}

/// A number from `min` to `max`.
pub(crate) fn number(min: f64, max: f64, cyclic: bool, integer: bool) -> Kind {
    Kind::Number {
        min,
        max,
        unit: String::new(),
        cyclic,
        integer,
    }
}

/// A signal given by a function of the time.
pub(crate) struct FnSignal(pub(crate) Box<dyn Fn(Seconds) -> f64 + Send + Sync>);

impl Signal for FnSignal {
    fn sample(&self, times: &[Seconds]) -> Result<Vec<f64>, Error> {
        Ok(times.iter().map(|&t| (self.0)(t)).collect())
    }
}

/// A signal that is `value` at every time.
pub(crate) fn constant(value: f64) -> Arc<dyn Signal> {
    Arc::new(FnSignal(Box::new(move |_| value)))
}
