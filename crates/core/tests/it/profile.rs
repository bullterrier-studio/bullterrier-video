//! `Profile` through the public API: the presets worked out by hand, and what is refused.

use bullterrier_video::{Error, Profile};

#[test]
fn the_presets_have_the_shapes_their_names_say() {
    let rows = [
        (Profile::vertical(720, 30.0), (720, 1280)),
        (Profile::vertical(1080, 30.0), (1080, 1920)),
        (Profile::landscape(1456, 30.0), (1456, 819 + 1)),
        (Profile::landscape(1920, 24.0), (1920, 1080)),
        (Profile::square(1080, 30.0), (1080, 1080)),
    ];
    for (profile, size) in rows {
        let profile = profile.unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!((profile.width(), profile.height()), size);
        assert_eq!(profile.width() % 2 + profile.height() % 2, 0, "sides are even");
    }
}

#[test]
fn a_vertical_profile_keeps_titles_between_twelve_and_eighty_percent_of_the_height() {
    let vertical = Profile::vertical(720, 30.0).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(vertical.safe(), (0.12, 0.80));
    let landscape = Profile::landscape(1280, 30.0).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(landscape.safe(), (0.0, 1.0));
}

#[test]
fn the_long_side_is_what_a_picture_is_asked_to_draw_at() {
    let tall = Profile::vertical(720, 30.0).unwrap_or_else(|e| unreachable!("{e}"));
    let wide = Profile::landscape(1920, 30.0).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((tall.long_side(), wide.long_side()), (1280, 1920));
}

#[test]
fn a_profile_that_could_not_be_encoded_cannot_be_made() {
    let invalid = |r: Result<Profile, Error>| matches!(r, Err(Error::Invalid { .. }));
    assert!(invalid(Profile::new(0, 100, 30.0)));
    assert!(invalid(Profile::new(100, 0, 30.0)));
    assert!(invalid(Profile::new(101, 100, 30.0)), "an odd width");
    assert!(invalid(Profile::new(100, 101, 30.0)), "an odd height");
    for fps in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(invalid(Profile::new(100, 100, fps)), "fps {fps}");
    }
    let base = || Profile::square(100, 30.0).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(base().with_depth(12), Err(Error::Invalid { .. })));
    assert!(matches!(base().with_loudness(3.0), Err(Error::Invalid { .. })));
    assert!(matches!(base().with_loudness(f64::NAN), Err(Error::Invalid { .. })));
}

#[test]
fn the_defaults_are_eight_bit_h264_and_no_loudness_change() {
    let profile = Profile::square(100, 25.0).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        (profile.depth(), profile.codec(), profile.loudness(), profile.fps()),
        (8, "h264", None, 25.0)
    );
    let tuned = profile
        .with_depth(10)
        .and_then(|p| p.with_loudness(-14.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((tuned.depth(), tuned.loudness()), (10, Some(-14.0)));
}
