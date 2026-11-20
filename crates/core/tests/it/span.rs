//! `Span::resolve` against a fake song, every row worked out by hand.

use bullterrier_video::{Band, Error, Moment, Span};

use crate::common::fakes::FakeMusic;

fn song() -> FakeMusic {
    let mut music = FakeMusic::of(25.0);
    music.drops = vec![Moment { t: 14.0, strength: 1.0 }, Moment { t: 5.0, strength: 0.5 }];
    music.downbeats = vec![1.0, 3.0, 5.0, 7.0];
    music
}

fn resolve(span: &Span, music: &FakeMusic) -> Result<(f64, f64), Error> {
    span.resolve(music)
}

#[expect(clippy::needless_pass_by_value, reason = "it reads best wrapped round the call")]
fn invalid<T: std::fmt::Debug>(r: Result<T, Error>) -> bool {
    matches!(r, Err(Error::Invalid { .. }))
}

#[test]
fn around_the_drop_starts_before_the_strongest_one() {
    let music = song();
    assert_eq!(
        resolve(
            &Span::AroundDrop {
                before: 4.0,
                length: 10.0
            },
            &music
        ),
        Ok((10.0, 20.0))
    );
    assert_eq!(
        resolve(
            &Span::AroundDrop {
                before: 20.0,
                length: 10.0
            },
            &music
        ),
        Ok((0.0, 10.0)),
        "the start stops at the song's start"
    );
}

#[test]
fn around_the_drop_is_refused_without_a_drop_or_past_the_end() {
    assert!(
        invalid(resolve(
            &Span::AroundDrop {
                before: 4.0,
                length: 10.0
            },
            &FakeMusic::of(25.0)
        )),
        "no drop"
    );
    assert!(
        invalid(resolve(
            &Span::AroundDrop {
                before: 4.0,
                length: 20.0
            },
            &song()
        )),
        "ends at 30 of 25"
    );
    assert!(invalid(resolve(
        &Span::AroundDrop {
            before: 4.0,
            length: 0.0
        },
        &song()
    )));
    assert!(invalid(resolve(
        &Span::AroundDrop {
            before: f64::NAN,
            length: 5.0
        },
        &song()
    )));
}

#[test]
fn bars_run_from_the_first_downbeat() {
    let music = song();
    assert_eq!(resolve(&Span::Bars(2), &music), Ok((1.0, 5.0)));
    assert_eq!(resolve(&Span::Bars(3), &music), Ok((1.0, 7.0)));
    assert!(invalid(resolve(&Span::Bars(4), &music)), "only three whole bars");
    assert!(invalid(resolve(&Span::Bars(0), &music)));
    assert!(invalid(resolve(&Span::Bars(1), &FakeMusic::of(25.0))), "no downbeats");
}

#[test]
fn whole_and_between() {
    let music = song();
    assert_eq!(resolve(&Span::Whole, &music), Ok((0.0, 25.0)));
    assert_eq!(resolve(&Span::Between { start: 2.0, end: 6.5 }, &music), Ok((2.0, 6.5)));
    for (start, end) in [(-1.0, 5.0), (5.0, 5.0), (6.0, 5.0), (0.0, 25.5), (f64::NAN, 5.0)] {
        assert!(
            invalid(resolve(&Span::Between { start, end }, &music)),
            "{start} to {end}"
        );
    }
}

#[test]
fn the_loudest_window_is_the_earliest_with_the_greatest_total() {
    let mut music = FakeMusic::of(10.0);
    music.levels = vec![0.0, 0.0, 1.0, 5.0, 5.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    // windows of 3: sums 1, 6, 11, 11, 6, 1, ...: the first of the two tied is at 2
    assert_eq!(
        resolve(
            &Span::Loudest {
                band: Band::Bass,
                length: 3.0
            },
            &music
        ),
        Ok((2.0, 5.0))
    );
    assert!(
        invalid(resolve(
            &Span::Loudest {
                band: Band::Bass,
                length: 11.0
            },
            &music
        )),
        "longer than the song"
    );
    assert!(invalid(resolve(
        &Span::Loudest {
            band: Band::Bass,
            length: 0.0
        },
        &music
    )));
}
