//! A clip: one picture on the timeline, with the values set on it and the signals that drive it.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::colour::Ramp;
use crate::error::Error;
use crate::ports::{Picture, Signal};
use crate::types::{Kind, Parameter, Seconds, Value, Values};

/// A signal that drives one address, read `lag` seconds late (a negative lag leads).
#[derive(Clone)]
struct Mapping {
    signal: Arc<dyn Signal>,
    lag: Seconds,
    /// For a colour: the ramp the signal's value picks a position on.
    ramp: Option<Ramp>,
}

/// A picture placed on the timeline from `start` to `end` (`None`: to the next clip or the end of the piece).
///
/// Every address that is set or mapped is checked against the picture's own description when it is given, so a clip never
/// holds a value its picture would refuse. A value is **held to the parameter's range** (a cyclic one wraps, an integer
/// rounds, a bool is on from one half); a pixel value is never scaled here, the picture reads it at its own design size.
#[derive(Clone)]
pub struct Clip {
    name: String,
    picture: Arc<dyn Picture>,
    parameters: BTreeMap<String, Kind>,
    start: Seconds,
    end: Option<Seconds>,
    sets: Values,
    mappings: BTreeMap<String, Mapping>,
}

impl Clip {
    /// A clip of `picture`, named `name`, from `start` seconds to `end`.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the name is empty, a time is not finite, `start` is negative or `end` is not after it;
    /// [`Error::Port`] when the picture cannot describe itself.
    pub fn new(name: &str, picture: Arc<dyn Picture>, start: Seconds, end: Option<Seconds>) -> Result<Self, Error> {
        if name.is_empty() {
            return Err(invalid("a clip needs a name".into()));
        }
        if !(start.is_finite() && start >= 0.0) || end.is_some_and(|e| !(e.is_finite() && e > start)) {
            return Err(invalid(format!(
                "clip `{name}` needs 0 <= start < end, got {start} to {end:?}"
            )));
        }
        let parameters = describe(picture.as_ref())?;
        Ok(Self {
            name: name.into(),
            picture,
            parameters,
            start,
            end,
            sets: Values::new(),
            mappings: BTreeMap::new(),
        })
    }

    /// The clip's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where the clip starts, in piece seconds.
    #[must_use]
    pub fn start(&self) -> Seconds {
        self.start
    }

    /// Where the clip ends, if it was given.
    #[must_use]
    pub fn end(&self) -> Option<Seconds> {
        self.end
    }

    /// The picture.
    #[must_use]
    pub fn picture(&self) -> &Arc<dyn Picture> {
        &self.picture
    }

    /// Drives `address` with `signal`, read `lag` seconds late.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the address is not the picture's (the nearest names are listed), is not a number or a bool, is
    /// already mapped or already set, or `lag` is not finite.
    pub fn map(&mut self, address: &str, signal: Arc<dyn Signal>, lag: Seconds) -> Result<(), Error> {
        let kind = self.kind_of(address)?;
        if matches!(kind, Kind::Color) {
            return Err(invalid(format!(
                "`{address}` of clip `{}` is a colour, which a bare signal cannot drive: map it with a ramp, btv.Ramp(stop, stop, signal)",
                self.name
            )));
        }
        if !matches!(kind, Kind::Number { .. } | Kind::Bool) {
            return Err(invalid(format!(
                "`{address}` of clip `{}` is not a number or a bool, so a signal cannot drive it: set it",
                self.name
            )));
        }
        self.insert_mapping(
            address,
            Mapping {
                signal,
                lag,
                ramp: None,
            },
        )
    }

    /// Drives the colour at `address`: `signal`, read `lag` seconds late, picks the position on `ramp`.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the address is not the picture's, is not a colour, is already mapped or set, or `lag` is not finite.
    pub fn map_ramp(&mut self, address: &str, ramp: Ramp, signal: Arc<dyn Signal>, lag: Seconds) -> Result<(), Error> {
        if !matches!(self.kind_of(address)?, Kind::Color) {
            return Err(invalid(format!(
                "`{address}` of clip `{}` is not a colour, so a ramp cannot drive it: a number or a bool takes a plain signal, the rest are set",
                self.name
            )));
        }
        self.insert_mapping(
            address,
            Mapping {
                signal,
                lag,
                ramp: Some(ramp),
            },
        )
    }

    fn insert_mapping(&mut self, address: &str, mapping: Mapping) -> Result<(), Error> {
        let lag = mapping.lag;
        if !lag.is_finite() {
            return Err(invalid(format!("lag must be a finite number of seconds, got {lag}")));
        }
        if self.mappings.contains_key(address) || self.sets.contains_key(address) {
            return Err(invalid(format!(
                "`{address}` of clip `{}` is already {}",
                self.name,
                if self.mappings.contains_key(address) {
                    "mapped"
                } else {
                    "set"
                }
            )));
        }
        self.mappings.insert(address.into(), mapping);
        Ok(())
    }

    /// Sets constants, replacing an earlier set of the same address.
    ///
    /// # Errors
    /// [`Error::Invalid`] when an address is not the picture's, is mapped, or a value does not suit its parameter (a choice that
    /// is not offered, a text for a number); nothing is set then.
    pub fn set(&mut self, values: Values) -> Result<(), Error> {
        let mut checked = Values::new();
        for (address, value) in values {
            if self.mappings.contains_key(&address) {
                return Err(invalid(format!(
                    "`{address}` of clip `{}` is mapped, so it cannot also be set",
                    self.name
                )));
            }
            let kind = self.kind_of(&address)?.clone();
            checked.insert(address.clone(), self.fit(&address, &kind, value)?);
        }
        self.sets.extend(checked);
        Ok(())
    }

    /// Puts another picture in the clip.
    ///
    /// # Errors
    /// [`Error::Invalid`] when an address that is set or mapped is missing from the new picture or has another kind; the clip
    /// is left as it was. [`Error::Port`] when the picture cannot describe itself.
    pub fn swap(&mut self, picture: Arc<dyn Picture>) -> Result<(), Error> {
        let parameters = describe(picture.as_ref())?;
        for address in self.sets.keys().chain(self.mappings.keys()) {
            if parameters.get(address) != self.parameters.get(address) {
                return Err(invalid(format!(
                    "the new picture has no `{address}` of the same kind as clip `{}` uses",
                    self.name
                )));
            }
        }
        self.picture = picture;
        self.parameters = parameters;
        Ok(())
    }

    /// The values for each of `times` (piece seconds): the constants, and each mapped signal read once for all the times.
    ///
    /// # Errors
    /// [`Error::Port`] when a signal fails or returns the wrong number of values; [`Error::Invalid`] when one returns a number
    /// that is not finite.
    pub fn values(&self, times: &[Seconds]) -> Result<Vec<Values>, Error> {
        let mut frames = vec![self.sets.clone(); times.len()];
        for (address, mapping) in &self.mappings {
            let lagged: Vec<Seconds> = times.iter().map(|t| (t - mapping.lag).max(0.0)).collect();
            let raw = mapping.signal.sample(&lagged).map_err(|e| match e {
                Error::Port { port, message } => Error::Port {
                    port,
                    message: format!("`{address}` of clip `{}`: {message}", self.name),
                },
                other => other,
            })?;
            if raw.len() != times.len() {
                return Err(Error::Port {
                    port: "signal",
                    message: format!(
                        "the signal of `{address}` gave {} values for {} times",
                        raw.len(),
                        times.len()
                    ),
                });
            }
            let kind = self.kind_of(address)?;
            for ((frame, value), t) in frames.iter_mut().zip(raw).zip(times) {
                let driven = match &mapping.ramp {
                    Some(ramp) if value.is_finite() => Ok(Value::Color(ramp.at(value))),
                    Some(_) => Err(invalid(format!("the value is {value}"))),
                    None => drive(kind, value),
                };
                frame.insert(
                    address.clone(),
                    driven.map_err(|e| invalid(format!("`{address}` at {t} s: {e}")))?,
                );
            }
        }
        Ok(frames)
    }

    fn kind_of(&self, address: &str) -> Result<&Kind, Error> {
        self.parameters.get(address).ok_or_else(|| {
            let near = nearest(address, self.parameters.keys());
            invalid(format!(
                "clip `{}` has no `{address}`; the nearest are {}",
                self.name,
                near.join(", ")
            ))
        })
    }

    fn fit(&self, address: &str, kind: &Kind, value: Value) -> Result<Value, Error> {
        let bad = |what: &str| invalid(format!("`{address}` of clip `{}` takes {what}", self.name));
        match (kind, value) {
            (Kind::Number { .. } | Kind::Bool, Value::Number(n)) => drive(kind, n),
            (Kind::Bool, Value::Bool(b)) => Ok(Value::Bool(b)),
            (Kind::Color, Value::Color(c)) => Ok(Value::Color(c)),
            (Kind::Text | Kind::Color, Value::Text(t)) => Ok(Value::Text(t)),
            (Kind::Choice { options }, Value::Text(t)) if options.contains(&t) => Ok(Value::Text(t)),
            (Kind::Choice { options }, Value::Text(_)) => Err(bad(&format!("one of {}", options.join(", ")))),
            (Kind::Picture, Value::Picture(frame)) => Ok(Value::Picture(frame)),
            (Kind::Number { .. }, _) => Err(bad("a number")),
            (Kind::Bool, _) => Err(bad("a bool")),
            (Kind::Text | Kind::Choice { .. }, _) => Err(bad("text")),
            (Kind::Color, _) => Err(bad("a colour as text (#rrggbb)")),
            (Kind::Picture, _) => Err(bad("a picture")),
            (Kind::Other(name), _) => Err(invalid(format!("`{address}` is a `{name}`, a kind video cannot set"))),
        }
    }
}

/// The parameters of `picture`, by address. A number's range must be two finite numbers, the first not above the second.
fn describe(picture: &dyn Picture) -> Result<BTreeMap<String, Kind>, Error> {
    let mut described = BTreeMap::new();
    for Parameter { address, kind, .. } in picture.parameters()? {
        if let Kind::Number { min, max, .. } = &kind
            && !(min.is_finite() && max.is_finite() && min <= max)
        {
            return Err(Error::Port {
                port: "picture",
                message: format!(
                    "`{address}` has the range {min} to {max}: two finite numbers, the first not above the second"
                ),
            });
        }
        described.insert(address, kind);
    }
    Ok(described)
}

/// A raw signal value as a value of `kind`: held to the range, wrapped, rounded or thresholded.
fn drive(kind: &Kind, raw: f64) -> Result<Value, Error> {
    if !raw.is_finite() {
        return Err(invalid(format!("the value is {raw}")));
    }
    match kind {
        Kind::Number {
            min,
            max,
            cyclic,
            integer,
            ..
        } => {
            // a whole number rounds first, so that 359.6 of a 0 to 360 turn is 360, which wraps to 0
            let whole = if *integer { raw.round() } else { raw };
            let held = if *cyclic && max > min {
                min + (whole - min).rem_euclid(max - min)
            } else {
                whole.clamp(*min, *max)
            };
            Ok(Value::Number(held))
        }
        // constant: one half, the middle of a signal's usual zero to one
        Kind::Bool => Ok(Value::Bool(raw >= 0.5)),
        _ => Err(invalid("only a number or a bool is driven".into())),
    }
}

/// Up to three of `names` nearest to `wanted` by edit distance, nearest first.
fn nearest<'a>(wanted: &str, names: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut scored: Vec<(usize, &String)> = names.map(|n| (distance(wanted, n), n)).collect();
    scored.sort();
    scored.into_iter().take(3).map(|(_, n)| format!("`{n}`")).collect()
}

/// The edit distance between two strings (insert, delete, change one character).
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1).min(row[j] + 1).min(diagonal + usize::from(ca != *cb));
            diagonal = above;
        }
    }
    row[b.len()]
}

fn invalid(message: String) -> Error {
    Error::Invalid { message }
}
