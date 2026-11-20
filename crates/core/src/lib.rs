//! The core of bullterrier-video: its logic, in pure Rust. No Python, no numpy, safe code: the `bullterrier-video-python` crate binds
//! it to the Python package.
//!
//! The one feature is [`scale`], which shows the layout: a documented function here, a binding in
//! `crates/python`, a test through the public API in `tests/it/`, and an inline test only for the private kernel.

#![forbid(unsafe_code)]

mod error;
pub mod ports;
mod profile;
mod scaling;
mod song;
mod span;
mod types;

pub use error::Error;
pub use profile::Profile;
pub use scaling::scale;
pub use song::{Levels, Song};
pub use span::Span;
pub use types::{Band, Frame, Hit, Kind, Moment, Parameter, Seconds, Section, Value, Values, View};
