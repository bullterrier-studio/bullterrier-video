//! `Clip`: what a mapped signal becomes (held, wrapped, rounded, switched), lag, and what is refused. Rows by hand.

use std::sync::Arc;

use bullterrier_video::{Clip, Error, Frame, Kind, Value, Values};
use proptest::prelude::*;

use crate::common::fakes::{Described, FnSignal, constant, number, parameter};

fn clip_with(address: &str, kind: Kind) -> Clip {
    Clip::new("c", Arc::new(Described(vec![parameter(address, kind)])), 0.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"))
}

/// The value a constant signal `raw` gives `address` of a clip with `kind`.
fn driven(kind: Kind, raw: f64) -> Value {
    let mut clip = clip_with("a", kind);
    clip.map("a", constant(raw), 0.0)
        .unwrap_or_else(|e| unreachable!("{e}"));
    clip.values(&[0.0])
        .unwrap_or_else(|e| unreachable!("{e}"))
        .remove(0)
        .remove("a")
        .unwrap_or_else(|| unreachable!("no value"))
}

#[test]
fn a_number_is_held_to_its_range() {
    let rows = [(1.5, 1.0), (-0.2, 0.0), (0.4, 0.4), (0.0, 0.0), (1.0, 1.0)];
    for (raw, expected) in rows {
        assert_eq!(
            driven(number(0.0, 1.0, false, false), raw),
            Value::Number(expected),
            "{raw}"
        );
    }
}

#[test]
fn a_cyclic_number_wraps_round_instead() {
    let rows = [
        (370.0, 10.0),
        (-10.0, 350.0),
        (360.0, 0.0),
        (720.0, 0.0),
        (90.0, 90.0),
        (0.0, 0.0),
    ];
    for (raw, expected) in rows {
        assert_eq!(
            driven(number(0.0, 360.0, true, false), raw),
            Value::Number(expected),
            "{raw}"
        );
    }
    assert_eq!(
        driven(number(-1.0, 1.0, true, false), 1.5),
        Value::Number(-0.5),
        "a range that does not start at zero"
    );
    assert_eq!(
        driven(number(0.0, 360.0, true, true), 359.6),
        Value::Number(0.0),
        "a whole angle rounds first, then wraps: 360 is 0"
    );
}

#[test]
fn an_integer_rounds_after_it_is_held() {
    let rows = [(3.4, 3.0), (3.6, 4.0), (12.0, 10.0), (-2.0, 0.0)];
    for (raw, expected) in rows {
        assert_eq!(
            driven(number(0.0, 10.0, false, true), raw),
            Value::Number(expected),
            "{raw}"
        );
    }
}

#[test]
fn a_bool_is_on_from_one_half() {
    assert_eq!(driven(Kind::Bool, 0.49), Value::Bool(false));
    assert_eq!(driven(Kind::Bool, 0.5), Value::Bool(true));
    assert_eq!(driven(Kind::Bool, 3.0), Value::Bool(true));
}

#[test]
fn a_pixel_value_is_not_scaled() {
    let px = Kind::Number {
        min: 0.0,
        max: 100.0,
        unit: "px".into(),
        cyclic: false,
        integer: false,
    };
    assert_eq!(
        driven(px, 12.0),
        Value::Number(12.0),
        "the picture reads px at its own design size"
    );
}

#[test]
fn a_signal_is_read_late_by_its_lag_and_never_before_zero() {
    let mut clip = clip_with("a", number(-10.0, 10.0, false, false));
    clip.map("a", Arc::new(FnSignal(Box::new(|t| t))), 0.1)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let frames = clip
        .values(&[0.0, 0.05, 0.5, 2.0])
        .unwrap_or_else(|e| unreachable!("{e}"));
    let read: Vec<f64> = frames
        .iter()
        .map(|f| {
            if let Some(Value::Number(n)) = f.get("a") {
                *n
            } else {
                f64::NAN
            }
        })
        .collect();
    assert_eq!(read, vec![0.0, 0.0, 0.4, 1.9], "t - 0.1, held at zero");
    let mut leading = clip_with("a", number(-10.0, 10.0, false, false));
    leading
        .map("a", Arc::new(FnSignal(Box::new(|t| t))), -0.5)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        leading.values(&[1.0]).map(|f| f[0].get("a").cloned()),
        Ok(Some(Value::Number(1.5))),
        "a negative lag leads"
    );
}

#[test]
fn constants_ride_along_with_the_mapped_values() {
    let picture = Described(vec![
        parameter("a", number(0.0, 1.0, false, false)),
        parameter("title", Kind::Text),
        parameter("tint", Kind::Color),
        parameter(
            "mode",
            Kind::Choice {
                options: vec!["normal".into(), "screen".into()],
            },
        ),
    ]);
    let mut clip = Clip::new("c", Arc::new(picture), 0.0, None).unwrap_or_else(|e| unreachable!("{e}"));
    clip.map("a", constant(0.25), 0.0)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut values = Values::new();
    values.insert("title".into(), Value::Text("PILOT".into()));
    values.insert("tint".into(), Value::Text("#ff00ff".into()));
    values.insert("mode".into(), Value::Text("screen".into()));
    clip.set(values).unwrap_or_else(|e| unreachable!("{e}"));
    let frame = clip.values(&[0.0, 1.0]).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(frame.len(), 2);
    assert_eq!(frame[1].len(), 4, "one mapped and three set");
    assert_eq!(frame[1].get("a"), Some(&Value::Number(0.25)));
    assert_eq!(frame[1].get("title"), Some(&Value::Text("PILOT".into())));
}

#[test]
fn an_unknown_address_is_refused_with_the_nearest_names() {
    let picture = Described(vec![
        parameter("logo.dx", number(0.0, 1.0, false, false)),
        parameter("logo.dy", number(0.0, 1.0, false, false)),
        parameter("title.text", Kind::Text),
    ]);
    let mut clip = Clip::new("mill", Arc::new(picture), 0.0, None).unwrap_or_else(|e| unreachable!("{e}"));
    let Err(Error::Invalid { message }) = clip.map("logo.dz", constant(0.0), 0.0) else {
        unreachable!("accepted")
    };
    assert!(
        message.contains("`logo.dz`") && message.contains("`logo.dx`") && message.contains("`logo.dy`"),
        "{message}"
    );
    assert!(
        message.find("`logo.dx`") < message.find("`title.text`").or(Some(usize::MAX)),
        "nearest first: {message}"
    );
}

#[test]
fn what_cannot_be_driven_and_what_cannot_be_twice_is_refused() {
    let picture = Described(vec![
        parameter("a", number(0.0, 1.0, false, false)),
        parameter("title", Kind::Text),
        parameter("lut", Kind::Other("lut".into())),
        parameter(
            "mode",
            Kind::Choice {
                options: vec!["normal".into()],
            },
        ),
    ]);
    let mut clip = Clip::new("c", Arc::new(picture), 0.0, None).unwrap_or_else(|e| unreachable!("{e}"));
    let invalid = |r: Result<(), Error>| matches!(r, Err(Error::Invalid { .. }));
    assert!(
        invalid(clip.map("title", constant(0.0), 0.0)),
        "text is set, not mapped"
    );
    assert!(
        invalid(clip.map("lut", constant(0.0), 0.0)),
        "a kind video does not map"
    );
    assert!(
        invalid(clip.map("a", constant(0.0), f64::NAN)),
        "a lag that is not a number"
    );
    assert!(clip.map("a", constant(0.0), 0.0).is_ok());
    assert!(invalid(clip.map("a", constant(1.0), 0.0)), "mapped twice");
    let one = |address: &str, value: Value| Values::from([(address.to_string(), value)]);
    assert!(
        invalid(clip.set(one("a", Value::Number(0.5)))),
        "set an address that is mapped"
    );
    assert!(
        invalid(clip.set(one("title", Value::Number(1.0)))),
        "a number for a text"
    );
    assert!(
        invalid(clip.set(one("mode", Value::Text("screen".into())))),
        "a choice that is not offered"
    );
    assert!(invalid(clip.set(one("lut", Value::Text("x".into())))));
    let frame = Frame::new(1, 1, vec![0.0; 4]).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        invalid(clip.set(one("title", Value::Picture(frame)))),
        "a picture for a text"
    );
    assert!(clip.set(one("title", Value::Text("OK".into()))).is_ok());
    assert!(invalid(clip.map("title", constant(0.0), 0.0)));
}

#[test]
fn a_failed_set_changes_nothing() {
    let mut clip = clip_with("a", number(0.0, 1.0, false, false));
    let both = Values::from([
        ("a".to_string(), Value::Number(0.5)),
        ("zz".to_string(), Value::Number(1.0)),
    ]);
    assert!(clip.set(both).is_err());
    assert_eq!(
        clip.values(&[0.0]).map(|f| f[0].len()),
        Ok(0),
        "the valid half was not kept either"
    );
}

#[test]
fn a_signal_that_returns_nan_names_the_address_and_the_moment() {
    let mut clip = clip_with("a", number(0.0, 1.0, false, false));
    clip.map(
        "a",
        Arc::new(FnSignal(Box::new(|t| if t > 0.4 { f64::NAN } else { 0.0 }))),
        0.0,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let Err(Error::Invalid { message }) = clip.values(&[0.0, 0.5]) else {
        unreachable!("accepted")
    };
    assert!(message.contains("`a`") && message.contains("0.5"), "{message}");
}

#[test]
fn a_swap_keeps_what_the_new_picture_still_has_and_refuses_what_it_lost() {
    let a = Described(vec![
        parameter("a", number(0.0, 1.0, false, false)),
        parameter("b", number(0.0, 1.0, false, false)),
    ]);
    let mut clip = Clip::new("c", Arc::new(a), 0.0, None).unwrap_or_else(|e| unreachable!("{e}"));
    clip.map("a", constant(0.5), 0.0)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let same = Described(vec![parameter("a", number(0.0, 1.0, false, false))]);
    assert!(clip.swap(Arc::new(same)).is_ok(), "`b` was not used");
    let lost = Described(vec![parameter("z", number(0.0, 1.0, false, false))]);
    assert!(
        matches!(clip.swap(Arc::new(lost)), Err(Error::Invalid { .. })),
        "`a` is mapped and gone"
    );
    let other_kind = Described(vec![parameter("a", Kind::Bool)]);
    assert!(
        matches!(clip.swap(Arc::new(other_kind)), Err(Error::Invalid { .. })),
        "`a` changed kind"
    );
    assert_eq!(
        clip.values(&[0.0]).map(|f| f[0].get("a").cloned()),
        Ok(Some(Value::Number(0.5))),
        "still as it was"
    );
}

#[test]
fn a_clip_with_no_name_or_a_backwards_span_is_refused() {
    let picture = || Arc::new(Described(vec![]));
    assert!(matches!(
        Clip::new("", picture(), 0.0, None),
        Err(Error::Invalid { .. })
    ));
    assert!(matches!(
        Clip::new("c", picture(), -1.0, None),
        Err(Error::Invalid { .. })
    ));
    assert!(matches!(
        Clip::new("c", picture(), 2.0, Some(2.0)),
        Err(Error::Invalid { .. })
    ));
    assert!(matches!(
        Clip::new("c", picture(), 0.0, Some(f64::NAN)),
        Err(Error::Invalid { .. })
    ));
}

proptest! {
    /// A turn is periodic: a whole number of turns more or fewer reads the same angle.
    #[test]
    fn a_cyclic_value_repeats_every_turn(raw in -1000.0f64..1000.0, turns in -3i32..4) {
        let kind = number(0.0, 360.0, true, false);
        let (Value::Number(base), Value::Number(shifted)) = (driven(kind.clone(), raw), driven(kind, raw + f64::from(turns) * 360.0)) else { unreachable!("numbers") };
        prop_assert!((base - shifted).abs() < 1e-6 || (base - shifted).abs() > 360.0 - 1e-6, "{base} vs {shifted}");
        prop_assert!((0.0..360.0).contains(&base));
    }

    /// A held value is already held: driving with the result gives the result back.
    #[test]
    fn a_held_value_is_stable(raw in -50.0f64..50.0) {
        let kind = number(-3.0, 7.0, false, false);
        let Value::Number(once) = driven(kind.clone(), raw) else { unreachable!("number") };
        prop_assert_eq!(driven(kind, once), Value::Number(once));
        prop_assert!((-3.0..=7.0).contains(&once));
    }
}

#[test]
fn a_picture_whose_range_is_backwards_or_not_a_number_is_refused_when_it_is_described() {
    for (min, max) in [(1.0, 0.0), (f64::NAN, 1.0), (0.0, f64::NAN), (0.0, f64::INFINITY)] {
        let picture = Described(vec![parameter("a", number(min, max, false, false))]);
        let Err(Error::Port {
            port: "picture",
            message,
        }) = Clip::new("c", Arc::new(picture), 0.0, None)
        else {
            unreachable!("{min} to {max} accepted")
        };
        assert!(message.contains("`a`"), "{message}");
    }
    let one_value = Described(vec![parameter("a", number(2.0, 2.0, false, false))]);
    assert!(
        Clip::new("c", Arc::new(one_value), 0.0, None).is_ok(),
        "a range of one value is allowed"
    );
}
