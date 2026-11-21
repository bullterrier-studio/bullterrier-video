//! `bullterrier_video._native._core`: gives the Rust core to Python. Generic: it holds no logic of the core, only the mapping of
//! its calls, its arrays and its errors. A refusal of the core is raised as the package's `bullterrier_video.errors.InvalidError`
//! (`PortError` for a library behind a port), imported when the first one is raised: importing the extension imports nothing of
//! the package.

mod adapters;
mod error;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bullterrier_video as core;
use bullterrier_video::{Band, Ffmpeg, Hit};
use numpy::{PyArray1, PyArrayMethods};
use pyo3::prelude::*;
use pyo3::sync::MutexExt;
use pyo3::types::{PyDict, PyList};

use adapters::{PyCompose, PyMusic, PyPicture, PySignal, PySound, value_of};
use error::{invalid, raised};

/// bullterrier-video's Rust core.
#[pymodule]
mod _core {
    #[pymodule_export]
    use super::{Music, Piece, Profile, Span, scale};
}

/// `value` times `factor`, never a negative zero: the template's placeholder, kept until its gate proofs are rewritten for a
/// project without it. A value that is not a finite number, or a result that is too large, is an `InvalidError`.
#[pyfunction]
pub(crate) fn scale(py: Python<'_>, value: f64, factor: f64) -> PyResult<f64> {
    core::scale(value, factor).map_err(|error| raised(py, &error))
}

/// The band named `name`.
fn band_of(py: Python<'_>, name: &str) -> PyResult<Band> {
    match name {
        "sub" => Ok(Band::Sub),
        "bass" => Ok(Band::Bass),
        "low_mid" => Ok(Band::LowMid),
        "mid" => Ok(Band::Mid),
        "high" => Ok(Band::High),
        "air" => Ok(Band::Air),
        other => Err(invalid(
            py,
            format!("a band is sub, bass, low_mid, mid, high or air, got `{other}`"),
        )),
    }
}

/// The shape, rate and encoding of the finished video.
#[pyclass(frozen, name = "Profile", module = "bullterrier_video._native._core")]
struct Profile(core::Profile);

impl Profile {
    /// `made` with the optional settings applied.
    fn finished(
        py: Python<'_>,
        made: Result<core::Profile, core::Error>,
        depth: u8,
        codec: &str,
        loudness: Option<f64>,
    ) -> PyResult<Self> {
        let mut profile = made
            .and_then(|p| p.with_depth(depth))
            .map(|p| p.with_codec(codec))
            .map_err(|e| raised(py, &e))?;
        if let Some(lufs) = loudness {
            profile = profile.with_loudness(lufs).map_err(|e| raised(py, &e))?;
        }
        Ok(Self(profile))
    }
}

#[pymethods]
impl Profile {
    /// A profile of `width` by `height` at `fps` frames a second; both sides must be even.
    #[new]
    #[pyo3(signature = (width, height, fps, *, depth=8, codec="h264", loudness=None))]
    fn new(
        py: Python<'_>,
        width: u32,
        height: u32,
        fps: f64,
        depth: u8,
        codec: &str,
        loudness: Option<f64>,
    ) -> PyResult<Self> {
        Self::finished(py, core::Profile::new(width, height, fps), depth, codec, loudness)
    }

    /// 9:16, `width` wide; titles are safe between 12 and 80 percent of the height.
    #[staticmethod]
    #[pyo3(signature = (width, fps, *, depth=8, codec="h264", loudness=None))]
    fn vertical(py: Python<'_>, width: u32, fps: f64, depth: u8, codec: &str, loudness: Option<f64>) -> PyResult<Self> {
        Self::finished(py, core::Profile::vertical(width, fps), depth, codec, loudness)
    }

    /// 16:9, `width` wide.
    #[staticmethod]
    #[pyo3(signature = (width, fps, *, depth=8, codec="h264", loudness=None))]
    fn landscape(
        py: Python<'_>,
        width: u32,
        fps: f64,
        depth: u8,
        codec: &str,
        loudness: Option<f64>,
    ) -> PyResult<Self> {
        Self::finished(py, core::Profile::landscape(width, fps), depth, codec, loudness)
    }

    /// Square, `width` on a side.
    #[staticmethod]
    #[pyo3(signature = (width, fps, *, depth=8, codec="h264", loudness=None))]
    fn square(py: Python<'_>, width: u32, fps: f64, depth: u8, codec: &str, loudness: Option<f64>) -> PyResult<Self> {
        Self::finished(py, core::Profile::square(width, fps), depth, codec, loudness)
    }

    /// Columns.
    #[getter]
    fn width(&self) -> u32 {
        self.0.width()
    }

    /// Rows.
    #[getter]
    fn height(&self) -> u32 {
        self.0.height()
    }

    /// Frames a second.
    #[getter]
    fn fps(&self) -> f64 {
        self.0.fps()
    }

    /// Bits a colour channel is written with.
    #[getter]
    fn depth(&self) -> u8 {
        self.0.depth()
    }

    /// The codec by name.
    #[getter]
    fn codec(&self) -> String {
        self.0.codec().to_string()
    }

    /// The soundtrack's loudness in LUFS, or `None`.
    #[getter]
    fn loudness(&self) -> Option<f64> {
        self.0.loudness()
    }

    /// Where titles are safe, as fractions of the height from the top: `(top, bottom)`.
    #[getter]
    fn safe(&self) -> (f64, f64) {
        self.0.safe()
    }
}

/// Which part of a song becomes the piece.
#[pyclass(frozen, name = "Span", module = "bullterrier_video._native._core")]
struct Span(core::Span);

#[pymethods]
impl Span {
    /// `length` seconds starting `before` seconds ahead of the strongest drop.
    #[staticmethod]
    fn around_drop(before: f64, length: f64) -> Self {
        Self(core::Span::AroundDrop { before, length })
    }

    /// `bars` whole bars from the first downbeat.
    #[staticmethod]
    fn bars(bars: u32) -> Self {
        Self(core::Span::Bars(bars))
    }

    /// The whole song.
    #[staticmethod]
    fn whole() -> Self {
        Self(core::Span::Whole)
    }

    /// From `start` to `end` seconds.
    #[staticmethod]
    fn between(start: f64, end: f64) -> Self {
        Self(core::Span::Between { start, end })
    }

    /// The `length` seconds in which `band` is strongest.
    #[staticmethod]
    fn loudest(py: Python<'_>, band: &str, length: f64) -> PyResult<Self> {
        Ok(Self(core::Span::Loudest {
            band: band_of(py, band)?,
            length,
        }))
    }
}

/// What the song under a piece knows, in the piece's own seconds.
#[pyclass(frozen, name = "Music", module = "bullterrier_video._native._core")]
struct Music(core::Song);

#[pymethods]
impl Music {
    /// How long the piece's soundtrack lasts.
    #[getter]
    fn duration(&self) -> f64 {
        self.0.duration()
    }

    /// Beats a minute.
    fn tempo(&self, py: Python<'_>) -> PyResult<f64> {
        self.0.tempo().map_err(|e| raised(py, &e))
    }

    /// The beats.
    fn beats(&self, py: Python<'_>) -> PyResult<Vec<f64>> {
        self.0.beats().map_err(|e| raised(py, &e))
    }

    /// The first beat of each bar.
    fn downbeats(&self, py: Python<'_>) -> PyResult<Vec<f64>> {
        self.0.downbeats().map_err(|e| raised(py, &e))
    }

    /// The strongest drop; an `InvalidError` when there is none.
    fn drop(&self, py: Python<'_>) -> PyResult<f64> {
        self.0.drop().map_err(|e| raised(py, &e))
    }

    /// The hits of one kind: `kick`, `snare` or `hat`.
    fn hits(&self, py: Python<'_>, kind: &str) -> PyResult<Vec<f64>> {
        let hit = match kind {
            "kick" => Hit::Kick,
            "snare" => Hit::Snare,
            "hat" => Hit::Hat,
            other => return Err(invalid(py, format!("a hit is kick, snare or hat, got `{other}`"))),
        };
        self.0.hits(hit).map_err(|e| raised(py, &e))
    }

    /// The sections as `(start, end, label)`.
    fn sections(&self, py: Python<'_>) -> PyResult<Vec<(f64, f64, String)>> {
        Ok(self
            .0
            .sections()
            .map_err(|e| raised(py, &e))?
            .into_iter()
            .map(|s| (s.start, s.end, s.label))
            .collect())
    }

    /// The level of `band` as `(values, rate, start)`, over the whole piece or over `(from, to)` (`to` of `None`: its end).
    #[pyo3(signature = (band, over=None))]
    fn band<'py>(
        &self,
        py: Python<'py>,
        band: &str,
        over: Option<(f64, Option<f64>)>,
    ) -> PyResult<(Bound<'py, PyArray1<f64>>, f64, f64)> {
        let levels = self.0.band(band_of(py, band)?, over).map_err(|e| raised(py, &e))?;
        Ok((PyArray1::from_vec(py, levels.values), levels.rate, levels.start))
    }
}

/// A video being made: clips on a timeline, one song, a profile.
#[pyclass(frozen, name = "Piece", module = "bullterrier_video._native._core")]
struct Piece(Mutex<core::Piece>);

impl Piece {
    /// The core piece, waiting for it with the GIL released: the thread that holds it may be calling Python.
    fn locked<'a>(&'a self, py: Python<'_>) -> MutexGuard<'a, core::Piece> {
        self.0.lock_py_attached(py).unwrap_or_else(PoisonError::into_inner)
    }
}

#[pymethods]
impl Piece {
    /// An empty piece with `profile`; `looped` makes its clock a circle (checked when it renders).
    #[new]
    #[pyo3(signature = (profile, looped=false))]
    fn new(py: Python<'_>, profile: &Profile, looped: bool) -> PyResult<Self> {
        let mut piece = core::Piece::new(profile.0.clone());
        piece.set_looped(looped).map_err(|e| raised(py, &e))?;
        Ok(Self(Mutex::new(piece)))
    }

    /// An independent piece that shares the pictures, signals and song.
    fn copy(&self, py: Python<'_>) -> Self {
        Self(Mutex::new(self.locked(py).clone()))
    }

    /// Adds a clip of `picture` (an object with `parameters()`, `with_values(mapping)` and `render(long_side)`).
    #[pyo3(signature = (name, picture, start=0.0, end=None))]
    fn clip(&self, py: Python<'_>, name: &str, picture: Py<PyAny>, start: f64, end: Option<f64>) -> PyResult<()> {
        let picture = Arc::new(PyPicture::new(picture));
        self.locked(py)
            .clip(name, picture, start, end)
            .map_err(|e| raised(py, &e))
    }

    /// Fails with the clip names when there is no clip `name`.
    fn check_clip(&self, py: Python<'_>, name: &str) -> PyResult<()> {
        self.locked(py).clip_named(name).map(|_| ()).map_err(|e| raised(py, &e))
    }

    /// Drives `address` of clip `clip` with `signal` (an object with `sample(times)`), read `lag` seconds late.
    #[pyo3(signature = (clip, address, signal, lag=0.0))]
    fn map(&self, py: Python<'_>, clip: &str, address: &str, signal: Py<PyAny>, lag: f64) -> PyResult<()> {
        let mut piece = self.locked(py);
        let clip = piece.clip_mut(clip).map_err(|e| raised(py, &e))?;
        clip.map(address, Arc::new(PySignal(signal)), lag)
            .map_err(|e| raised(py, &e))
    }

    /// Drives the colour `address` of clip `clip`: `signal` picks the position on the ramp through `stops` (`#rrggbb`, `#rgb`
    /// or three floats from 0 to 1).
    #[pyo3(signature = (clip, address, stops, signal, lag=0.0))]
    fn map_ramp(
        &self,
        py: Python<'_>,
        clip: &str,
        address: &str,
        stops: &Bound<'_, PyList>,
        signal: Py<PyAny>,
        lag: f64,
    ) -> PyResult<()> {
        let stops = stops
            .iter()
            .map(|stop| match stop.extract::<String>() {
                Ok(text) => core::parse_hex(&text),
                Err(_) => stop.extract::<[f64; 3]>().map_err(|_| core::Error::Invalid {
                    message: format!(
                        "a ramp stop is #rrggbb, #rgb or three numbers from 0 to 1, got {}",
                        stop.get_type()
                    ),
                }),
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(core::Ramp::new)
            .map_err(|e| raised(py, &e))?;
        let mut piece = self.locked(py);
        let clip = piece.clip_mut(clip).map_err(|e| raised(py, &e))?;
        clip.map_ramp(address, stops, Arc::new(PySignal(signal)), lag)
            .map_err(|e| raised(py, &e))
    }

    /// Sets constants on clip `clip`: numbers, bools, text (a colour, a choice) and float32 picture arrays, by address.
    fn set(&self, py: Python<'_>, clip: &str, values: &Bound<'_, PyDict>) -> PyResult<()> {
        let mut converted = core::Values::new();
        for (address, value) in values.iter() {
            converted.insert(address.extract()?, value_of(py, &value)?);
        }
        let mut piece = self.locked(py);
        piece
            .clip_mut(clip)
            .map_err(|e| raised(py, &e))?
            .set(converted)
            .map_err(|e| raised(py, &e))
    }

    /// Puts another picture in clip `clip`.
    fn swap(&self, py: Python<'_>, clip: &str, picture: Py<PyAny>) -> PyResult<()> {
        let mut piece = self.locked(py);
        piece
            .clip_mut(clip)
            .map_err(|e| raised(py, &e))?
            .swap(Arc::new(PyPicture::new(picture)))
            .map_err(|e| raised(py, &e))
    }

    /// Puts the song under the piece. `music` and `sound` are the audio adapter's objects.
    #[pyo3(signature = (music, sound, span, fade_in=0.0, fade_out=0.0))]
    fn song(
        &self,
        py: Python<'_>,
        music: Py<PyAny>,
        sound: &Bound<'_, PyAny>,
        span: &Span,
        fade_in: f64,
        fade_out: f64,
    ) -> PyResult<Music> {
        let music = Arc::new(PyMusic::new(music).map_err(|e| raised(py, &e))?);
        let sound = PySound::new(&sound.clone().unbind()).map_err(|e| raised(py, &e))?;
        let song = self
            .locked(py)
            .song(music, &sound, &span.0, fade_in, fade_out)
            .map_err(|e| raised(py, &e))?;
        Ok(Music(song))
    }

    /// How long the piece lasts, in seconds.
    fn duration(&self, py: Python<'_>) -> PyResult<f64> {
        self.locked(py).duration().map_err(|e| raised(py, &e))
    }

    /// The number of frames.
    fn frame_count(&self, py: Python<'_>) -> PyResult<usize> {
        self.locked(py).frame_count().map_err(|e| raised(py, &e))
    }

    /// The frame at `t` seconds, a float32 `(height, width, 4)` array.
    fn frame<'py>(&self, py: Python<'py>, t: f64) -> PyResult<Bound<'py, numpy::PyArray3<f32>>> {
        let piece = self.locked(py).clone();
        let frame = py.detach(|| piece.frame(t)).map_err(|e| raised(py, &e))?;
        PyArray1::from_slice(py, frame.pixels()).reshape([frame.height(), frame.width(), 4])
    }

    /// A contact sheet of the piece at the moments `at`, each cell `width` pixels along its long side, in rows of `columns` (one row if `None`), written to `path`.
    #[expect(clippy::needless_pass_by_value, reason = "PyO3 extracts a path and a list by value")]
    fn sheet(&self, py: Python<'_>, path: PathBuf, at: Vec<f64>, width: u32, columns: Option<usize>) -> PyResult<()> {
        let piece = self.locked(py).clone();
        py.detach(|| piece.sheet(&Ffmpeg::discover(), &path, &at, width, columns))
            .map_err(|e| raised(py, &e))
    }

    /// Renders the piece into `path` on `workers` threads: `(frames written, frames drawn, frames composed)`.
    #[expect(clippy::needless_pass_by_value, reason = "PyO3 extracts a path by value")]
    fn render(&self, py: Python<'_>, path: PathBuf, workers: usize) -> PyResult<(usize, usize, usize)> {
        let piece = self.locked(py).clone();
        let stats = py
            .detach(|| piece.render(&Ffmpeg::discover(), &path, workers))
            .map_err(|e| raised(py, &e))?;
        Ok((stats.frames, stats.drawn, stats.composed))
    }

    /// Gives the piece its compose (an object with `compose(layers)`), which a camera and a transition need.
    fn compose(&self, py: Python<'_>, compose: Py<PyAny>) {
        self.locked(py).compose(Arc::new(PyCompose(compose)));
    }

    /// The camera: each of `dx`, `dy`, `zoom` and `rotate` a signal (an object with `sample(times)`) or `None` for at rest.
    fn camera(
        &self,
        py: Python<'_>,
        dx: Option<Py<PyAny>>,
        dy: Option<Py<PyAny>>,
        zoom: Option<Py<PyAny>>,
        rotate: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        let signal = |given: Option<Py<PyAny>>| given.map(|s| Arc::new(PySignal(s)) as Arc<dyn core::ports::Signal>);
        self.locked(py)
            .camera(signal(dx), signal(dy), signal(zoom), signal(rotate))
            .map_err(|e| raised(py, &e))
    }

    /// A transition from clip `a` to clip `b` centred on `at`, `length` seconds long: `kind` is `cut`, `dissolve`, `light` or
    /// `dark` (a sweep at `angle` degrees with an edge `feather` of the long side wide).
    #[pyo3(signature = (a, b, at, length, kind, angle=0.0, feather=0.15))]
    #[expect(
        clippy::too_many_arguments,
        reason = "the call's own parameters, as the Python method lists them"
    )]
    fn transition(
        &self,
        py: Python<'_>,
        a: &str,
        b: &str,
        at: f64,
        length: f64,
        kind: &str,
        angle: f64,
        feather: f64,
    ) -> PyResult<()> {
        let kind = match kind {
            "cut" => core::TransitionKind::Cut,
            "dissolve" => core::TransitionKind::Dissolve,
            "light" => core::TransitionKind::Sweep {
                edge: core::Edge::Light,
                angle,
                feather,
            },
            "dark" => core::TransitionKind::Sweep {
                edge: core::Edge::Dark,
                angle,
                feather,
            },
            other => {
                return Err(invalid(
                    py,
                    format!("a transition is cut, dissolve, light or dark, got `{other}`"),
                ));
            }
        };
        self.locked(py)
            .transition(a, b, at, length, kind)
            .map_err(|e| raised(py, &e))
    }

    /// The profile the piece is written with.
    fn profile(&self, py: Python<'_>) -> Profile {
        Profile(self.locked(py).profile().clone())
    }

    /// The same piece as a draft: `scale` times the size and, if given, at `fps`.
    #[pyo3(signature = (scale, fps=None))]
    fn draft(&self, py: Python<'_>, scale: f64, fps: Option<f64>) -> PyResult<Self> {
        let draft = self.locked(py).draft(scale, fps).map_err(|e| raised(py, &e))?;
        Ok(Self(Mutex::new(draft)))
    }
}
