//! The signal port: a value that changes over time.

use crate::error::Error;
use crate::types::Seconds;

/// A number at every moment: an envelope, a keyframed curve, a band's level, a low-frequency wave.
///
/// A signal is total: it has a value at any time, before its start and after its end included. It is read for many times at
/// once, because a whole clip is sampled in one call.
pub trait Signal: Send + Sync {
    /// The value at each of `times`, in the order given, one number each.
    ///
    /// # Errors
    /// [`Error::Port`] when the signal cannot be read.
    fn sample(&self, times: &[Seconds]) -> Result<Vec<f64>, Error>;
}
