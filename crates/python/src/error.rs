//! The core's errors raised as the package's exceptions of `bullterrier_video.errors`, and Python's failures inside a port
//! turned into the core's.

use pyo3::prelude::*;

use bullterrier_video::Error;

/// The Python exception for `error`: `PortError` for a library behind a port, else `InvalidError`, with the core's message. The
/// module is imported when the first error is raised: importing the extension imports nothing of the package.
pub(crate) fn raised(py: Python<'_>, error: &Error) -> PyErr {
    let name = match error {
        Error::Port { .. } => "PortError",
        _ => "InvalidError",
    };
    instance(py, name, error.to_string()).unwrap_or_else(|failure| failure)
}

/// An `InvalidError` for something the core never sees: a Python value of the wrong type.
pub(crate) fn invalid(py: Python<'_>, message: String) -> PyErr {
    instance(py, "InvalidError", message).unwrap_or_else(|failure| failure)
}

/// A Python exception raised inside the object behind `port`, as the core's error: its class and its message, so a missing
/// method reads `picture: AttributeError: 'X' object has no attribute 'with_values'`.
pub(crate) fn from_python(port: &'static str, py: Python<'_>, failure: &PyErr) -> Error {
    let class = failure
        .get_type(py)
        .name()
        .map_or_else(|_| "Exception".into(), |n| n.to_string());
    Error::Port {
        port,
        message: format!("{class}: {}", failure.value(py)),
    }
}

/// `bullterrier_video.errors.<name>(message)` as an error to raise.
fn instance(py: Python<'_>, name: &str, message: String) -> PyResult<PyErr> {
    let class = py.import("bullterrier_video.errors")?.getattr(name)?;
    Ok(PyErr::from_value(class.call1((message,))?))
}
