//! A colour ramp: stops spaced evenly, a position from zero to one picks the colour between them, mixed in `OKLab`.
//!
//! Colours are sRGB-encoded floats `0..=1`, the form a picture's colour setting takes. The mix is done here, once, so no
//! caller blends per channel (which goes muddy in the middle).

use crate::error::Error;

/// An sRGB-encoded colour, red, green, blue, each `0..=1`.
pub type Rgb = [f64; 3];

/// Two or more colour stops, evenly spaced; [`Ramp::at`] reads the colour at a position `0..=1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Ramp {
    stops: Vec<Rgb>,
}

impl Ramp {
    /// A ramp through `stops`, first to last.
    ///
    /// # Errors
    /// [`Error::Invalid`] when there are fewer than two stops or a channel is not a number in `0..=1`.
    pub fn new(stops: Vec<Rgb>) -> Result<Self, Error> {
        if stops.len() < 2 {
            return Err(invalid(format!(
                "a ramp needs two or more colours, got {}",
                stops.len()
            )));
        }
        if let Some(bad) = stops.iter().flatten().find(|c| !(0.0..=1.0).contains(*c)) {
            return Err(invalid(format!("a colour channel is a number from 0 to 1, got {bad}")));
        }
        Ok(Self { stops })
    }

    /// The colour at `position`, held to `0..=1`: the first stop at 0, the last at 1, exactly; between them the mix in `OKLab`.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a position held to 0..=1 times a handful of stops is a small non-negative number"
    )]
    pub fn at(&self, position: f64) -> Rgb {
        let spans = self.stops.len() - 1;
        let scaled = position.clamp(0.0, 1.0) * spans as f64;
        let i = (scaled.floor() as usize).min(spans - 1);
        let t = scaled - i as f64;
        let (from, to) = (self.stops[i], self.stops[i + 1]);
        if t <= 0.0 {
            return from;
        }
        if t >= 1.0 {
            return to;
        }
        let (a, b) = (to_oklab(from), to_oklab(to));
        from_oklab([0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t))
    }
}

/// A colour as `#rrggbb` or `#rgb`, in `0..=1`.
///
/// # Errors
/// [`Error::Invalid`] naming the text when it is neither.
pub fn parse_hex(text: &str) -> Result<Rgb, Error> {
    let digits = text.strip_prefix('#').unwrap_or("");
    let value = |s: &str| u8::from_str_radix(s, 16).ok();
    let parsed = match digits.len() {
        6 if digits.is_ascii() => (0..3)
            .map(|k| value(&digits[2 * k..2 * k + 2]))
            .collect::<Option<Vec<_>>>(),
        3 if digits.is_ascii() => (0..3).map(|k| value(&digits[k..=k]).map(|v| v * 17)).collect(),
        _ => None,
    };
    match parsed {
        Some(c) => Ok([0, 1, 2].map(|k| f64::from(c[k]) / 255.0)),
        None => Err(invalid(format!("`{text}` is not a colour: write #rrggbb or #rgb"))),
    }
}

fn decode(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

// OKLab (Ottosson, 2020) from linear sRGB: the cone responses, their cube roots, then the lab axes.
fn to_oklab(rgb: Rgb) -> [f64; 3] {
    let [red, green, blue] = rgb.map(decode);
    let long = (0.412_221_470_8 * red + 0.536_332_536_3 * green + 0.051_445_992_9 * blue).cbrt();
    let medium = (0.211_903_498_2 * red + 0.680_699_545_1 * green + 0.107_396_956_6 * blue).cbrt();
    let short = (0.088_302_461_9 * red + 0.281_718_837_6 * green + 0.629_978_700_5 * blue).cbrt();
    [
        0.210_454_255_3 * long + 0.793_617_785_0 * medium - 0.004_072_046_8 * short,
        1.977_998_495_1 * long - 2.428_592_205_0 * medium + 0.450_593_709_9 * short,
        0.025_904_037_1 * long + 0.782_771_766_2 * medium - 0.808_675_766_0 * short,
    ]
}

fn from_oklab([light, green_red, blue_yellow]: [f64; 3]) -> Rgb {
    let long = (light + 0.396_337_777_4 * green_red + 0.215_803_757_3 * blue_yellow).powi(3);
    let medium = (light - 0.105_561_345_8 * green_red - 0.063_854_172_8 * blue_yellow).powi(3);
    let short = (light - 0.089_484_177_5 * green_red - 1.291_485_548_0 * blue_yellow).powi(3);
    [
        4.076_741_662_1 * long - 3.307_711_591_3 * medium + 0.230_969_929_2 * short,
        -1.268_438_004_6 * long + 2.609_757_401_1 * medium - 0.341_319_396_5 * short,
        -0.004_196_086_3 * long - 0.703_418_614_7 * medium + 1.707_614_701_0 * short,
    ]
    .map(encode)
}

fn invalid(message: String) -> Error {
    Error::Invalid { message }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Published OKLab values (Ottosson's reference): sRGB red and blue.
    #[test]
    fn oklab_of_primaries_matches_the_published_values() {
        let near = |got: [f64; 3], want: [f64; 3]| got.iter().zip(want).all(|(g, w)| (g - w).abs() < 2e-4);
        assert!(near(to_oklab([1.0, 0.0, 0.0]), [0.627_955, 0.224_863, 0.125_846]));
        assert!(near(to_oklab([0.0, 0.0, 1.0]), [0.452_014, -0.032_457, -0.311_528]));
        assert!(near(to_oklab([1.0, 1.0, 1.0]), [1.0, 0.0, 0.0]));
    }

    #[test]
    fn the_transfer_is_exact_at_its_knee_and_round_trips() {
        assert!((decode(0.04045) - 0.04045 / 12.92).abs() < 1e-15);
        assert!((decode(0.5) - 0.214_041_140_482_232).abs() < 1e-12);
        for k in 0..=100 {
            let c = f64::from(k) / 100.0;
            assert!((encode(decode(c)) - c).abs() < 1e-12);
        }
    }
}
