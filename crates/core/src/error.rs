//! What the core refuses, as one error type the bindings map to the package's exceptions.

use std::fmt;

/// Why a call of the core was refused. New variants may come in a minor release, so a `match` needs a `_` arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// An argument is NaN or infinite; `name` is the argument.
    NotFinite {
        /// The argument that is not a finite number.
        name: &'static str,
    },
    /// The result is too large for a finite number.
    Overflow,
    /// The pixels of a frame are not `width * height * 4` values, or a side is zero.
    FrameShape {
        /// Columns given.
        width: usize,
        /// Rows given.
        height: usize,
        /// Values given.
        len: usize,
    },
    /// A piece, a clip or a mapping that cannot be rendered; `message` names the thing and the fix.
    Invalid {
        /// What is wrong and what to do.
        message: String,
    },
    /// A library behind a port refused or failed; `port` names the port and `message` is what the adapter says.
    Port {
        /// The port: `picture`, `signal`, `music`, `sound` or `encode`.
        port: &'static str,
        /// Why, in the adapter's words (it names a missing method or a refused value).
        message: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite { name } => write!(formatter, "{name} must be a finite number"),
            Self::Overflow => write!(formatter, "the result is too large for a finite number"),
            Self::FrameShape { width, height, len } => {
                write!(
                    formatter,
                    "a frame of {width} by {height} needs {} values, got {len}",
                    width.saturating_mul(*height).saturating_mul(4)
                )
            }
            Self::Invalid { message } => write!(formatter, "{message}"),
            Self::Port { port, message } => write!(formatter, "{port}: {message}"),
        }
    }
}

impl std::error::Error for Error {}
