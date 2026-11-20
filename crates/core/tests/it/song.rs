//! `Song`: a song in piece time. The fakes keep what was asked of them, so the tests read it back.

use std::sync::Arc;

use bullterrier_video::{Band, Error, Hit, Moment, Section, Song, Span};

use crate::common::fakes::{FakeMusic, FakeSound};

fn music() -> FakeMusic {
    let mut m = FakeMusic::of(25.0);
    m.beats = (0..50).map(|i| f64::from(i) * 0.5).collect();
    m.downbeats = vec![0.0, 2.0, 4.0, 6.0, 8.0];
    m.drops = vec![Moment { t: 14.0, strength: 1.0 }];
    m.kicks = vec![9.0, 10.5, 12.0, 21.0];
    m.sections = vec![
        Section {
            start: 0.0,
            end: 8.0,
            label: "a".into(),
        },
        Section {
            start: 8.0,
            end: 16.0,
            label: "b".into(),
        },
        Section {
            start: 16.0,
            end: 25.0,
            label: "c".into(),
        },
    ];
    m.levels = (0..25).map(f64::from).collect();
    m
}

fn song_over(span: &Span) -> (Song, Arc<FakeMusic>) {
    let music = Arc::new(music());
    let song =
        Song::new(music.clone(), &FakeSound::of(25.0), span, 0.0, 0.5, None).unwrap_or_else(|e| unreachable!("{e}"));
    (song, music)
}

#[test]
fn every_time_is_in_piece_seconds_and_only_those_inside_the_span() {
    let (song, _) = song_over(&Span::Between { start: 9.0, end: 13.0 });
    assert_eq!(song.duration(), 4.0);
    assert_eq!(song.beats(), Ok(vec![0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5]));
    assert_eq!(
        song.hits(Hit::Kick),
        Ok(vec![0.0, 1.5, 3.0]),
        "kicks at 9, 10.5, 12; 21 is outside"
    );
    assert_eq!(song.downbeats(), Ok(vec![]), "no downbeat of the fake falls in 9 to 13");
}

#[test]
fn the_drop_is_found_inside_the_span_or_refused() {
    let (song, _) = song_over(&Span::AroundDrop {
        before: 4.0,
        length: 10.0,
    });
    assert_eq!(song.drop(), Ok(4.0));
    let (early, _) = song_over(&Span::Between { start: 0.0, end: 10.0 });
    assert!(
        matches!(early.drop(), Err(Error::Invalid { .. })),
        "the drop at 14 is outside 0 to 10"
    );
}

#[test]
fn a_measure_the_music_cannot_give_is_an_error() {
    let (song, _) = song_over(&Span::Whole);
    assert!(matches!(song.hits(Hit::Snare), Err(Error::Port { port: "music", .. })));
}

#[test]
fn the_tempo_is_sixty_over_the_median_gap_between_beats() {
    let (song, _) = song_over(&Span::Between { start: 2.0, end: 8.0 });
    assert_eq!(song.tempo(), Ok(120.0));
    let mut sparse = music();
    sparse.beats = vec![3.0];
    let alone = Song::new(Arc::new(sparse), &FakeSound::of(25.0), &Span::Whole, 0.0, 0.0, None)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(alone.tempo(), Err(Error::Invalid { .. })));
}

#[test]
fn sections_are_cut_to_the_span_and_moved_into_piece_time() {
    let (song, _) = song_over(&Span::Between { start: 5.0, end: 12.0 });
    let rows: Vec<(f64, f64, String)> = song
        .sections()
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.start, s.end, s.label))
        .collect();
    assert_eq!(rows, vec![(0.0, 3.0, "a".to_string()), (3.0, 7.0, "b".to_string())]);
}

#[test]
fn the_soundtrack_is_the_span_cut_faded_and_set_to_the_loudness() {
    let sound = FakeSound::of(25.0);
    let plain = Song::new(
        Arc::new(music()),
        &sound,
        &Span::Between { start: 5.0, end: 12.0 },
        0.25,
        0.5,
        None,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(plain.soundtrack().samples(), Ok(vec![7.0, 0.25, 0.5, -1.0]));
    let loud = Song::new(Arc::new(music()), &sound, &Span::Whole, 0.0, 0.0, Some(-14.0))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(loud.soundtrack().samples(), Ok(vec![25.0, 0.0, 0.0, -14.0]));
    assert!(matches!(
        Song::new(Arc::new(music()), &sound, &Span::Whole, -1.0, 0.0, None),
        Err(Error::Invalid { .. })
    ));
}

#[test]
fn levels_are_scaled_over_the_pieces_span_and_over_a_stretch_of_it() {
    let (song, music) = song_over(&Span::Between { start: 10.0, end: 20.0 });
    let whole = song.band(Band::Bass, None).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        *music.asked.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
        Some((10.0, 20.0)),
        "the span, not the song"
    );
    assert_eq!((whole.start, whole.rate, whole.values.len()), (0.0, 1.0, 10));
    assert_eq!((whole.values[0], whole.values[9]), (0.0, 1.0), "the span's own range");
    let tail = song
        .band(Band::Bass, Some((4.0, None)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        *music.asked.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
        Some((14.0, 20.0))
    );
    assert_eq!((tail.start, tail.values.len()), (4.0, 6));
    for over in [(5.0, Some(5.0)), (-1.0, None), (2.0, Some(11.0))] {
        assert!(
            matches!(song.band(Band::Bass, Some(over)), Err(Error::Invalid { .. })),
            "{over:?}"
        );
    }
}
