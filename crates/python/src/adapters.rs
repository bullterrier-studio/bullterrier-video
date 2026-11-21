//! The Python objects a caller hands over, as the core's ports: a scene as a [`Picture`], a control signal as a [`Signal`], and
//! the audio adapter's objects as [`Music`] and [`Sound`]. They call the object's own methods and keep nothing of its library.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use bullterrier_video::ports::{Blend, Compose, Layer, Music, Picture, Signal, Sound};
use bullterrier_video::{Band, Error, Frame, Hit, Kind, Moment, Parameter, Seconds, Section, Value, Values, View};
use numpy::{PyArray1, PyArrayMethods, PyReadonlyArray1, PyReadonlyArrayDyn, PyUntypedArrayMethods};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyString};

use crate::error::from_python;

/// A Python picture: an object with `parameters()`, `with_values(mapping)` and `render(long_side)`, which is a scene's shape.
pub(crate) struct PyPicture {
    object: Py<PyAny>,
    kinds: OnceLock<BTreeMap<String, Kind>>,
}

impl PyPicture {
    pub(crate) fn new(object: Py<PyAny>) -> Self {
        Self {
            object,
            kinds: OnceLock::new(),
        }
    }

    fn describe(&self, py: Python<'_>) -> Result<Vec<Parameter>, Error> {
        let fail = |e: PyErr| from_python("picture", py, &e);
        let listed = self.object.bind(py).call_method0("parameters").map_err(fail)?;
        let listed = listed.cast::<PyDict>().map_err(|_| Error::Port {
            port: "picture",
            message: "parameters() must return a dict from address to parameter".into(),
        })?;
        let mut out = Vec::with_capacity(listed.len());
        for (address, parameter) in listed.iter() {
            let address: String = address.extract().map_err(fail)?;
            let text = |name: &str| -> Result<String, Error> {
                match parameter.getattr(name) {
                    Ok(v) if v.is_none() => Ok(String::new()),
                    Ok(v) => v.extract().map_err(fail),
                    Err(e) => Err(fail(e)),
                }
            };
            let number =
                |name: &str| -> Result<f64, Error> { parameter.getattr(name).and_then(|v| v.extract()).map_err(fail) };
            let kind_name = text("kind")?;
            let kind = match kind_name.as_str() {
                "float" | "int" => Kind::Number {
                    min: number("min")?,
                    max: number("max")?,
                    unit: text("unit")?,
                    cyclic: parameter.getattr("cyclic").and_then(|v| v.extract()).map_err(fail)?,
                    integer: kind_name == "int",
                },
                "bool" => Kind::Bool,
                "text" => Kind::Text,
                "color" => Kind::Color,
                "picture" => Kind::Picture,
                "choice" => Kind::Choice {
                    options: parameter.getattr("choices").and_then(|v| v.extract()).map_err(fail)?,
                },
                other => Kind::Other(other.into()),
            };
            out.push(Parameter {
                address,
                meaning: text("meaning")?,
                kind,
            });
        }
        Ok(out)
    }

    fn to_python<'py>(&self, py: Python<'py>, address: &str, value: &Value) -> PyResult<Bound<'py, PyAny>> {
        let integer = matches!(
            self.kinds.get().and_then(|k| k.get(address)),
            Some(Kind::Number { integer: true, .. })
        );
        Ok(match value {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "an integer parameter is held to a small range and rounded"
            )]
            Value::Number(n) if integer => (*n as i64).into_pyobject(py)?.into_any(),
            Value::Number(n) => n.into_pyobject(py)?.into_any(),
            Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any(),
            Value::Text(t) => PyString::new(py, t).into_any(),
            Value::Picture(frame) => PyArray1::from_slice(py, frame.pixels())
                .reshape([frame.height(), frame.width(), 4])?
                .into_any(),
            // the enum is non-exhaustive: a value kind added to the core without a conversion here is a bug to see
            _ => {
                return Err(pyo3::exceptions::PyRuntimeError::new_err(
                    "a value the bindings cannot give to Python",
                ));
            }
        })
    }
}

impl Picture for PyPicture {
    fn parameters(&self) -> Result<Vec<Parameter>, Error> {
        Python::attach(|py| {
            let parameters = self.describe(py)?;
            let kinds = parameters.iter().map(|p| (p.address.clone(), p.kind.clone())).collect();
            let _ = self.kinds.set(kinds);
            Ok(parameters)
        })
    }

    fn render(&self, values: &Values, long_side: u32) -> Result<Frame, Error> {
        Python::attach(|py| {
            let fail = |e: PyErr| from_python("picture", py, &e);
            let set = PyDict::new(py);
            for (address, value) in values {
                set.set_item(address, self.to_python(py, address, value).map_err(fail)?)
                    .map_err(fail)?;
            }
            let held = self.object.bind(py).call_method1("with_values", (set,)).map_err(fail)?;
            let array = held.call_method1("render", (long_side,)).map_err(fail)?;
            frame_of(&array)
        })
    }
}

/// A float32 `(height, width, 4)` array as a frame.
pub(crate) fn frame_of(array: &Bound<'_, PyAny>) -> Result<Frame, Error> {
    frame_from(array, "picture", "a picture must draw")
}

/// A float32 `(height, width, 4)` array as a frame, refused as `port` saying `what` is wanted.
fn frame_from(array: &Bound<'_, PyAny>, port: &'static str, what: &str) -> Result<Frame, Error> {
    let found = array
        .extract::<PyReadonlyArrayDyn<'_, f32>>()
        .map_err(|_| Error::Port {
            port,
            message: format!("{what} a float32 array of shape (height, width, 4)"),
        })?;
    let &[height, width, 4] = found.shape() else {
        return Err(Error::Port {
            port,
            message: format!("{what} shape (height, width, 4), got {:?}", found.shape()),
        });
    };
    let pixels = found
        .as_slice()
        .map_or_else(|_| found.as_array().iter().copied().collect(), <[f32]>::to_vec);
    Frame::new(width, height, pixels)
}

/// A Python compose: an object with `compose(layers)`, each layer `(frame, (dx, dy, zoom, rotate), opacity, blend, mask)`, the
/// frame a float32 `(height, width, 4)` array and the mask a float32 `(height, width)` array or `None`; it returns the frame.
pub(crate) struct PyCompose(pub(crate) Py<PyAny>);

impl Compose for PyCompose {
    fn compose(&self, layers: &[Layer<'_>]) -> Result<Frame, Error> {
        Python::attach(|py| {
            let fail = |e: PyErr| from_python("compose", py, &e);
            let mut given = Vec::with_capacity(layers.len());
            for layer in layers {
                let View { dx, dy, zoom, rotate } = layer.view;
                let blend = match layer.blend {
                    Blend::Screen => "screen",
                    Blend::Multiply => "multiply",
                    _ => "normal",
                };
                let mask = layer
                    .mask
                    .map(|mask| {
                        // the red channel as a map: one value to a pixel
                        PyArray1::from_iter(py, mask.pixels().iter().step_by(4).copied())
                            .reshape([mask.height(), mask.width()])
                    })
                    .transpose()
                    .map_err(fail)?;
                let frame = PyArray1::from_slice(py, layer.frame.pixels())
                    .reshape([layer.frame.height(), layer.frame.width(), 4])
                    .map_err(fail)?;
                given.push((frame, (dx, dy, zoom, rotate), layer.opacity, blend, mask));
            }
            let done = self.0.bind(py).call_method1("compose", (given,)).map_err(fail)?;
            frame_from(&done, "compose", "compose must give")
        })
    }
}

/// A Python signal: an object with `sample(times)` returning an array.
pub(crate) struct PySignal(pub(crate) Py<PyAny>);

impl Signal for PySignal {
    fn sample(&self, times: &[Seconds]) -> Result<Vec<f64>, Error> {
        Python::attach(|py| {
            let fail = |e: PyErr| from_python("signal", py, &e);
            let sampled = self
                .0
                .bind(py)
                .call_method1("sample", (PyArray1::from_slice(py, times),))
                .map_err(fail)?;
            // any numeric array or list: an int64 array does not extract as f64 as it is
            let floats = py
                .import("numpy")
                .and_then(|numpy| numpy.call_method1("asarray", (sampled, "float64")))
                .map_err(fail)?;
            let values: PyReadonlyArray1<'_, f64> = floats.extract().map_err(fail)?;
            Ok(values.as_array().to_vec())
        })
    }
}

/// The audio adapter's music object (`bullterrier_video.adapters.audio`): plain lists and arrays, whatever the library behind.
pub(crate) struct PyMusic {
    object: Py<PyAny>,
    duration: Seconds,
}

impl PyMusic {
    pub(crate) fn new(object: Py<PyAny>) -> Result<Self, Error> {
        let duration = Python::attach(|py| {
            object
                .bind(py)
                .call_method0("duration")
                .and_then(|d| d.extract())
                .map_err(|e| from_python("music", py, &e))
        })?;
        Ok(Self { object, duration })
    }

    fn call<T: for<'a, 'py> FromPyObject<'a, 'py>>(
        &self,
        method: &str,
        args: impl for<'py> pyo3::call::PyCallArgs<'py>,
    ) -> Result<T, Error>
    where
        for<'a, 'py> <T as FromPyObject<'a, 'py>>::Error: Into<PyErr>,
    {
        Python::attach(|py| {
            let fail = |e: PyErr| from_python("music", py, &e);
            self.object
                .bind(py)
                .call_method(method, args, None)
                .map_err(fail)?
                .extract::<T>()
                .map_err(|e| fail(e.into()))
        })
    }
}

impl Music for PyMusic {
    fn duration(&self) -> Seconds {
        self.duration
    }
    fn beats(&self) -> Result<Vec<Seconds>, Error> {
        self.call("beats", ())
    }
    fn downbeats(&self) -> Result<Vec<Seconds>, Error> {
        self.call("downbeats", ())
    }
    fn drops(&self) -> Result<Vec<Moment>, Error> {
        Ok(self
            .call::<Vec<(f64, f64)>>("drops", ())?
            .into_iter()
            .map(|(t, strength)| Moment { t, strength })
            .collect())
    }
    fn hits(&self, hit: Hit) -> Result<Vec<Moment>, Error> {
        let name = match hit {
            Hit::Kick => "kick",
            Hit::Snare => "snare",
            _ => "hat",
        };
        Ok(self
            .call::<Vec<(f64, f64)>>("hits", (name,))?
            .into_iter()
            .map(|(t, strength)| Moment { t, strength })
            .collect())
    }
    fn sections(&self) -> Result<Vec<Section>, Error> {
        Ok(self
            .call::<Vec<(f64, f64, String)>>("sections", ())?
            .into_iter()
            .map(|(start, end, label)| Section { start, end, label })
            .collect())
    }
    fn band(&self, band: Band, start: Seconds, end: Seconds) -> Result<(f64, Vec<f64>), Error> {
        let name = match band {
            Band::Sub => "sub",
            Band::Bass => "bass",
            Band::LowMid => "low_mid",
            Band::Mid => "mid",
            Band::High => "high",
            _ => "air",
        };
        Python::attach(|py| {
            let fail = |e: PyErr| from_python("music", py, &e);
            let (rate, values): (f64, PyReadonlyArray1<'_, f64>) = self
                .object
                .bind(py)
                .call_method1("band", (name, start, end))
                .and_then(|r| r.extract())
                .map_err(fail)?;
            Ok((rate, values.as_array().to_vec()))
        })
    }
}

/// The audio adapter's sound object: a rate, channels and a duration, edited one operation at a time.
pub(crate) struct PySound {
    object: Py<PyAny>,
    rate: u32,
    channels: u16,
    duration: Seconds,
}

impl PySound {
    pub(crate) fn new(object: &Py<PyAny>) -> Result<Self, Error> {
        Python::attach(|py| {
            let fail = |e: PyErr| from_python("sound", py, &e);
            let bound = object.bind(py);
            let rate = bound.call_method0("rate").and_then(|v| v.extract()).map_err(fail)?;
            let channels = bound.call_method0("channels").and_then(|v| v.extract()).map_err(fail)?;
            let duration = bound.call_method0("duration").and_then(|v| v.extract()).map_err(fail)?;
            Ok(Self {
                object: object.clone_ref(py),
                rate,
                channels,
                duration,
            })
        })
    }

    fn edited(&self, method: &str, args: impl for<'py> pyo3::call::PyCallArgs<'py>) -> Result<Box<dyn Sound>, Error> {
        let next = Python::attach(|py| {
            self.object
                .bind(py)
                .call_method(method, args, None)
                .map(Bound::unbind)
                .map_err(|e| from_python("sound", py, &e))
        })?;
        Ok(Box::new(Self::new(&next)?))
    }
}

impl Sound for PySound {
    fn rate(&self) -> u32 {
        self.rate
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn duration(&self) -> Seconds {
        self.duration
    }
    fn cut(&self, start: Seconds, end: Seconds) -> Result<Box<dyn Sound>, Error> {
        self.edited("cut", (start, end))
    }
    fn fade(&self, fade_in: Seconds, fade_out: Seconds) -> Result<Box<dyn Sound>, Error> {
        self.edited("fade", (fade_in, fade_out))
    }
    fn normalised(&self, lufs: f64) -> Result<Box<dyn Sound>, Error> {
        self.edited("normalised", (lufs,))
    }
    fn samples(&self) -> Result<Vec<f32>, Error> {
        Python::attach(|py| {
            let fail = |e: PyErr| from_python("sound", py, &e);
            let samples = self.object.bind(py).call_method0("samples").map_err(fail)?;
            let samples: PyReadonlyArray1<'_, f32> = samples.extract().map_err(fail)?;
            Ok(samples.as_array().to_vec())
        })
    }
}

/// A Python value a caller `set`: a bool, a whole or fractional number, text, or a float32 picture array.
pub(crate) fn value_of(py: Python<'_>, object: &Bound<'_, PyAny>) -> PyResult<Value> {
    if object.is_instance_of::<PyBool>() {
        return Ok(Value::Bool(object.extract()?));
    }
    if object.is_instance_of::<PyInt>() || object.is_instance_of::<PyFloat>() {
        return Ok(Value::Number(object.extract()?));
    }
    if object.is_instance_of::<PyString>() {
        return Ok(Value::Text(object.extract()?));
    }
    frame_of(object).map(Value::Picture).map_err(|_| {
        crate::error::invalid(
            py,
            format!(
                "a value is a bool, a number, text or a float32 (height, width, 4) array, got {}",
                object.get_type()
            ),
        )
    })
}
