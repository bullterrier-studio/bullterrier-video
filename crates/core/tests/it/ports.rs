//! The ports through the public API: each trait is implemented by a plain fake and used as a trait object, which is how video's
//! logic will hold them; and the own types refuse what they must.

use std::path::Path;
use std::sync::Mutex;

use bullterrier_video::ports::{Audio, Blend, Compose, Encode, Job, Layer, Music, Picture, Signal, Sound};
use bullterrier_video::{Band, Error, Frame, Hit, Kind, Moment, Parameter, Seconds, Section, Value, Values, View};

/// A picture of one flat grey whose level is the `grey.level` parameter.
struct Grey;

impl Picture for Grey {
    fn parameters(&self) -> Result<Vec<Parameter>, Error> {
        let number = Kind::Number {
            min: 0.0,
            max: 1.0,
            unit: "fraction".into(),
            cyclic: false,
            integer: false,
        };
        let choice = Kind::Choice {
            options: vec!["normal".into(), "screen".into()],
        };
        let parameter = |address: &str, kind| Parameter {
            address: address.into(),
            meaning: String::new(),
            kind,
        };
        Ok(vec![
            parameter("grey.level", number),
            parameter("grey.tint", Kind::Color),
            parameter("grey.mode", choice),
        ])
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "the fake's levels are 0.25 and 0.75, exact in f32"
    )]
    fn render(&self, values: &Values, long_side: u32) -> Result<Frame, Error> {
        let level = match values.get("grey.level") {
            Some(Value::Number(n)) => *n as f32,
            _ => 0.0,
        };
        let side = long_side as usize;
        Frame::new(side, side, [level, level, level, 1.0].repeat(side * side))
    }
}

/// A signal that is the time itself.
struct Clock;

impl Signal for Clock {
    fn sample(&self, times: &[Seconds]) -> Result<Vec<f64>, Error> {
        Ok(times.to_vec())
    }
}

/// Music with a beat every half second for two seconds.
struct Beat;

impl Music for Beat {
    fn duration(&self) -> Seconds {
        2.0
    }
    fn beats(&self) -> Result<Vec<Seconds>, Error> {
        Ok(vec![0.0, 0.5, 1.0, 1.5])
    }
    fn downbeats(&self) -> Result<Vec<Seconds>, Error> {
        Ok(vec![0.0, 2.0])
    }
    fn drops(&self) -> Result<Vec<Moment>, Error> {
        Ok(vec![Moment { t: 1.0, strength: 1.0 }])
    }
    fn hits(&self, hit: Hit) -> Result<Vec<Moment>, Error> {
        Err(Error::Port {
            port: "music",
            message: format!("no drum part found for {hit:?}"),
        })
    }
    fn sections(&self) -> Result<Vec<Section>, Error> {
        Ok(vec![Section {
            start: 0.0,
            end: 2.0,
            label: "a".into(),
        }])
    }
    fn band(&self, _band: Band) -> Result<(f64, Vec<f64>), Error> {
        Ok((2.0, vec![0.0, 1.0, 0.0, 1.0]))
    }
}

/// A mono sound of silence at 4 samples a second.
struct Silence(f64);

impl Sound for Silence {
    fn rate(&self) -> u32 {
        4
    }
    fn channels(&self) -> u16 {
        1
    }
    fn duration(&self) -> Seconds {
        self.0
    }
    fn cut(&self, start: Seconds, end: Seconds) -> Result<Box<dyn Sound>, Error> {
        if start >= end || end > self.0 {
            return Err(Error::Port {
                port: "sound",
                message: "the span is empty or outside the sound".into(),
            });
        }
        Ok(Box::new(Silence(end - start)))
    }
    fn fade(&self, _fade_in: Seconds, _fade_out: Seconds) -> Result<Box<dyn Sound>, Error> {
        Ok(Box::new(Silence(self.0)))
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the fake's durations are small whole seconds"
    )]
    fn samples(&self) -> Result<Vec<f32>, Error> {
        Ok(vec![0.0; (self.0 * 4.0) as usize])
    }
}

/// A compositor that adds each upper layer to the ground, by its opacity and mask.
struct Adding;

impl Compose for Adding {
    fn compose(&self, layers: &[Layer<'_>]) -> Result<Frame, Error> {
        let ground = layers.first().ok_or_else(|| Error::Port {
            port: "compose",
            message: "no layer".into(),
        })?;
        let mut pixels = ground.frame.pixels().to_vec();
        for layer in &layers[1..] {
            for (i, value) in pixels.iter_mut().enumerate() {
                let shown = layer.mask.map_or(1.0, |m| f64::from(m.pixels()[i - i % 4]));
                let add = f64::from(layer.frame.pixels()[i]) * layer.opacity * shown;
                #[expect(clippy::cast_possible_truncation, reason = "the fake's values are small")]
                let add = add as f32;
                *value += add;
            }
        }
        Frame::new(ground.frame.width(), ground.frame.height(), pixels)
    }
}

/// An encoder that records how many frames it was given.
struct Counting(Mutex<usize>);

impl Encode for Counting {
    fn encode(&self, _job: &Job<'_>, frames: &mut dyn Iterator<Item = Result<Frame, Error>>) -> Result<(), Error> {
        for frame in frames {
            frame?;
            *self.0.lock().map_err(|_| Error::Port {
                port: "encode",
                message: "lock".into(),
            })? += 1;
        }
        Ok(())
    }
}

#[test]
fn a_picture_is_drawn_again_with_new_values_through_a_trait_object() {
    let picture: Box<dyn Picture> = Box::new(Grey);
    let mut values = Values::new();
    values.insert("grey.level".into(), Value::Number(0.25));
    let frame = picture.render(&values, 2).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((frame.width(), frame.height()), (2, 2));
    assert_eq!(&frame.pixels()[..4], &[0.25, 0.25, 0.25, 1.0]);
    values.insert("grey.level".into(), Value::Number(0.75));
    let later = picture.render(&values, 2).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(&later.pixels()[..4], &[0.75, 0.75, 0.75, 1.0]);
}

#[test]
fn a_signal_is_read_for_many_times_in_one_call() {
    let signal: Box<dyn Signal> = Box::new(Clock);
    assert_eq!(signal.sample(&[0.0, 0.5, 2.0]), Ok(vec![0.0, 0.5, 2.0]));
}

#[test]
fn music_that_cannot_find_hits_says_so_instead_of_returning_none() {
    let music: Box<dyn Music> = Box::new(Beat);
    assert_eq!(music.drops().map(|d| d[0].t), Ok(1.0));
    let error = music.hits(Hit::Kick).err().map(|e| e.to_string());
    assert_eq!(error.as_deref(), Some("music: no drum part found for Kick"));
}

#[test]
fn a_sound_is_cut_and_the_samples_follow_the_span() {
    let sound: Box<dyn Sound> = Box::new(Silence(2.0));
    let cut = sound.cut(0.5, 1.5).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(cut.duration(), 1.0);
    assert_eq!(cut.samples().map(|s| s.len()), Ok(4));
    assert!(sound.cut(1.5, 1.0).is_err());
}

#[test]
fn an_encoder_takes_frames_as_a_stream_and_stops_at_a_failed_frame() {
    let encoder = Counting(Mutex::new(0));
    let job = Job {
        depth: 8,
        codec: "h264",
        path: Path::new("out.mp4"),
        fps: 30.0,
        audio: Some(Audio {
            rate: 4,
            channels: 1,
            samples: &[0.0; 4],
        }),
    };
    let frame = || Frame::new(1, 1, vec![0.0; 4]);
    let mut good = vec![frame(), frame(), frame()].into_iter();
    assert_eq!(encoder.encode(&job, &mut good), Ok(()));
    assert_eq!(*encoder.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner), 3);

    let bad = Error::Port {
        port: "picture",
        message: "refused".into(),
    };
    let mut broken = vec![frame(), Err(bad.clone()), frame()].into_iter();
    assert_eq!(encoder.encode(&job, &mut broken), Err(bad));
    assert_eq!(
        *encoder.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
        4,
        "the frame after the failure is never read"
    );
}

#[test]
fn a_frame_refuses_the_wrong_number_of_values_and_an_empty_side() {
    for (w, h, len) in [(2, 2, 15), (2, 2, 17), (0, 3, 0), (3, 0, 0)] {
        assert_eq!(
            Frame::new(w, h, vec![0.0; len]),
            Err(Error::FrameShape {
                width: w,
                height: h,
                len
            }),
            "{w}x{h} with {len}"
        );
    }
    assert!(Frame::new(2, 3, vec![0.0; 24]).is_ok());
}

#[test]
fn the_view_that_changes_nothing_has_no_move_no_turn_and_a_zoom_of_one() {
    assert_eq!(
        View::IDENTITY,
        View {
            dx: 0.0,
            dy: 0.0,
            zoom: 1.0,
            rotate: 0.0
        }
    );
}

#[test]
fn layers_are_composed_into_a_frame_of_the_grounds_size_with_opacity_and_mask() {
    let compose: Box<dyn Compose> = Box::new(Adding);
    let frame = |v| Frame::new(1, 1, vec![v; 4]).unwrap_or_else(|e| unreachable!("{e}"));
    let (ground, light, half) = (frame(0.25), frame(0.5), frame(0.5));
    let layer = |opacity, blend, mask| Layer {
        frame: &light,
        view: View::IDENTITY,
        opacity,
        blend,
        mask,
    };
    let both = compose.compose(&[
        Layer {
            frame: &ground,
            ..layer(1.0, Blend::Normal, None)
        },
        layer(0.5, Blend::Screen, Some(&half)),
    ]);
    assert_eq!(both.map(|f| f.pixels()[0]), Ok(0.25 + 0.5 * 0.5 * 0.5));
    assert!(compose.compose(&[]).is_err());
}
