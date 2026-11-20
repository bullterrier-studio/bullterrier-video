//! The ffmpeg adapter through a stand-in program: a script that records its arguments, keeps a copy of any `.wav` it is given and
//! writes its standard input to the last argument. Unix only (the script is a shell script). What real ffmpeg does with the
//! arguments is checked by hand in the demo, not here.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use bullterrier_video::ports::{Audio, Encode, Job};
use bullterrier_video::{Error, Ffmpeg, Frame};
use tempfile::TempDir;

/// A script in `dir` with `body` as its text; returns its path.
fn script(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("fake-ffmpeg");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap_or_else(|e| unreachable!("{e}"));
    path
}

/// The script that stands in for a working ffmpeg: arguments to `args`, the sound to `sound`, the video to the last argument.
fn working(dir: &Path) -> PathBuf {
    let (args, sound) = (dir.join("args"), dir.join("sound"));
    script(
        dir,
        &format!(
            "printf '%s\\n' \"$@\" > '{}'\nfor a; do last=\"$a\"; case \"$a\" in *.audio.part) cp \"$a\" '{}';; esac; done\ncat > \"$last\"",
            args.display(),
            sound.display()
        ),
    )
}

fn frame(width: usize, height: usize, rgba: [f32; 4]) -> Result<Frame, Error> {
    Frame::new(width, height, rgba.repeat(width * height))
}

fn job<'a>(path: &'a Path, audio: Option<Audio<'a>>) -> Job<'a> {
    Job {
        path,
        fps: 30.0,
        depth: 8,
        codec: "h264",
        audio,
    }
}

fn arguments(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join("args"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn has(args: &[String], sequence: &[&str]) -> bool {
    args.windows(sequence.len())
        .any(|w| w.iter().map(String::as_str).eq(sequence.iter().copied()))
}

#[test]
fn frames_arrive_as_sixteen_bit_rgb_flattened_over_black() {
    let dir = TempDir::new().unwrap_or_else(|e| unreachable!("{e}"));
    let out = dir.path().join("clip.mp4");
    let ffmpeg = Ffmpeg::new(working(dir.path()));
    // white at half alpha is mid grey over black; the second frame is red; held to zero..one
    let mut frames = vec![
        frame(2, 2, [1.0, 1.0, 1.0, 0.5]),
        frame(2, 2, [1.0, 0.0, 0.0, 1.0]),
        frame(2, 2, [2.0, -1.0, 0.25, 7.0]),
    ]
    .into_iter();
    ffmpeg
        .encode(&job(&out, None), &mut frames)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let bytes = fs::read(&out).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(bytes.len(), 3 * 4 * 6, "three frames of four pixels, six bytes each");
    let pixel = |frame: usize| -> Vec<u16> {
        bytes[frame * 24..frame * 24 + 6]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect()
    };
    assert_eq!(pixel(0), vec![32768, 32768, 32768], "0.5 of 65535 rounded");
    assert_eq!(pixel(1), vec![65535, 0, 0]);
    assert_eq!(pixel(2), vec![65535, 0, 16384], "held to 0..1; 0.25 of 65535 rounded");
}

#[test]
fn the_arguments_say_what_is_coming_and_how_to_write_it() {
    let dir = TempDir::new().unwrap_or_else(|e| unreachable!("{e}"));
    let out = dir.path().join("clip.mp4");
    let ffmpeg = Ffmpeg::new(working(dir.path()));
    ffmpeg
        .encode(&job(&out, None), &mut vec![frame(4, 6, [0.0; 4])].into_iter())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let args = arguments(dir.path());
    assert!(
        has(
            &args,
            &[
                "-f", "rawvideo", "-pix_fmt", "rgb48le", "-s", "4x6", "-r", "30", "-i", "-"
            ]
        ),
        "{args:?}"
    );
    assert!(
        has(&args, &["-c:v", "libx264"]) && has(&args, &["-pix_fmt", "yuv420p"]),
        "{args:?}"
    );
    assert!(
        has(
            &args,
            &[
                "-colorspace",
                "bt709",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "bt709",
                "-color_range",
                "tv"
            ]
        ),
        "{args:?}"
    );
    assert!(
        args.iter()
            .any(|a| a.contains("out_color_matrix=bt709") && a.contains("in_range=full")),
        "{args:?}"
    );
    assert!(!args.iter().any(|a| a == "-c:a"), "no sound, no audio codec");
    assert_eq!(
        args.last().map(String::as_str),
        Some(&*format!("{}.part", out.display())),
        "written beside the target under a temporary name"
    );
}

#[test]
fn depth_and_codec_choose_the_encoder_and_the_pixel_format() {
    let rows = [
        ("h264", 8, "libx264", "yuv420p"),
        ("h264", 10, "libx264", "yuv420p10le"),
        ("hevc", 8, "libx265", "yuv420p"),
        ("hevc", 10, "libx265", "yuv420p10le"),
    ];
    for (codec, depth, encoder, pixel_format) in rows {
        let dir = TempDir::new().unwrap_or_else(|e| unreachable!("{e}"));
        let out = dir.path().join("clip.mp4");
        let job = Job {
            path: &out,
            fps: 24.0,
            depth,
            codec,
            audio: None,
        };
        Ffmpeg::new(working(dir.path()))
            .encode(&job, &mut vec![frame(2, 2, [0.0; 4])].into_iter())
            .unwrap_or_else(|e| unreachable!("{e}"));
        let args = arguments(dir.path());
        assert!(
            has(&args, &["-c:v", encoder]) && has(&args, &["-pix_fmt", pixel_format]),
            "{codec} {depth}: {args:?}"
        );
        assert_eq!(
            has(&args, &["-tag:v", "hvc1"]),
            codec == "hevc",
            "{codec}: only HEVC is tagged for Apple players"
        );
    }
}

#[test]
fn the_soundtrack_is_handed_over_as_a_float_wav_and_removed_afterwards() {
    let dir = TempDir::new().unwrap_or_else(|e| unreachable!("{e}"));
    let out = dir.path().join("clip.mp4");
    let samples = [0.0f32, 0.5, -0.5, 1.0];
    let audio = Audio {
        rate: 8000,
        channels: 2,
        samples: &samples,
    };
    Ffmpeg::new(working(dir.path()))
        .encode(&job(&out, Some(audio)), &mut vec![frame(2, 2, [0.0; 4])].into_iter())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let args = arguments(dir.path());
    assert!(
        has(&args, &["-c:a", "aac"]) && has(&args, &["-af", "apad", "-shortest"]),
        "the sound ends with the picture: {args:?}"
    );
    let wav = fs::read(dir.path().join("sound")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(&wav[8..16], b"WAVEfmt ");
    let field16 = |at: usize| u16::from_le_bytes([wav[at], wav[at + 1]]);
    let field32 = |at: usize| u32::from_le_bytes([wav[at], wav[at + 1], wav[at + 2], wav[at + 3]]);
    assert_eq!(
        (
            field16(20),
            field16(22),
            field32(24),
            field32(28),
            field16(32),
            field16(34)
        ),
        (3, 2, 8000, 64000, 8, 32),
        "float, 2 channels, 8000 Hz"
    );
    assert_eq!(&wav[36..40], b"fact");
    assert_eq!(&wav[48..52], b"data");
    assert_eq!(field32(52), 16, "four samples of four bytes");
    assert_eq!(
        field32(4) as usize,
        wav.len() - 8,
        "the RIFF size is the rest of the file"
    );
    assert_eq!(field32(44), 2, "two frames of two channels");
    assert_eq!(
        &wav[wav.len() - 16..],
        samples
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect::<Vec<u8>>()
            .as_slice(),
        "the samples, little endian"
    );
    let left: Vec<String> = fs::read_dir(dir.path())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .map(|e| {
            e.map(|e| e.file_name().to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    assert!(
        !left.iter().any(|n| n.contains(".part")),
        "no temporary file is left: {left:?}"
    );
}

#[test]
fn the_file_appears_only_when_ffmpeg_finished() {
    let dir = TempDir::new().unwrap_or_else(|e| unreachable!("{e}"));
    let out = dir.path().join("clip.mp4");
    let failing = script(
        dir.path(),
        "cat > /dev/null\necho 'Unknown encoder libx264' >&2\nexit 3",
    );
    let result = Ffmpeg::new(failing).encode(
        &job(&out, None),
        &mut vec![frame(2, 2, [0.0; 4]), frame(2, 2, [0.0; 4])].into_iter(),
    );
    let Err(Error::Port {
        port: "encode",
        message,
    }) = result
    else {
        unreachable!("accepted")
    };
    assert!(
        message.contains("Unknown encoder libx264") && message.contains("exit status: 3"),
        "{message}"
    );
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec!["fake-ffmpeg".to_string()],
        "no half-written file, no part file: {names:?}"
    );
}

#[test]
fn an_ffmpeg_that_stops_early_is_reported_with_its_own_words_not_a_broken_pipe() {
    let dir = TempDir::new().unwrap_or_else(|e| unreachable!("{e}"));
    let out = dir.path().join("clip.mp4");
    let quitting = script(dir.path(), "echo 'Invalid argument' >&2\nexit 1");
    // enough frames that the pipe fills after ffmpeg is gone
    let frames: Vec<_> = (0..40).map(|_| frame(64, 64, [0.0; 4])).collect();
    let Err(Error::Port {
        port: "encode",
        message,
    }) = Ffmpeg::new(quitting).encode(&job(&out, None), &mut frames.into_iter())
    else {
        unreachable!("accepted")
    };
    assert!(message.contains("Invalid argument"), "{message}");
}

#[test]
fn a_failing_frame_kills_ffmpeg_leaves_no_file_and_is_the_error() {
    let dir = TempDir::new().unwrap_or_else(|e| unreachable!("{e}"));
    let out = dir.path().join("clip.mp4");
    let bad = Error::Port {
        port: "picture",
        message: "refused".into(),
    };
    let mut frames = vec![frame(2, 2, [0.0; 4]), Err(bad.clone()), frame(2, 2, [0.0; 4])].into_iter();
    assert_eq!(
        Ffmpeg::new(working(dir.path())).encode(&job(&out, None), &mut frames),
        Err(bad)
    );
    assert!(!out.exists(), "no output");
    assert!(!dir.path().join("clip.mp4.part").exists(), "no part file");
}

#[test]
fn a_missing_program_a_bad_extension_odd_sides_and_no_frames_are_refused_by_name() {
    let dir = TempDir::new().unwrap_or_else(|e| unreachable!("{e}"));
    let out = dir.path().join("clip.mp4");
    let one = || vec![frame(2, 2, [0.0; 4])].into_iter();
    let Err(Error::Port { message, .. }) = Ffmpeg::new("/no/such/ffmpeg").encode(&job(&out, None), &mut one()) else {
        unreachable!("accepted")
    };
    assert!(
        message.contains("/no/such/ffmpeg") && message.contains("BT_VIDEO_FFMPEG"),
        "{message}"
    );
    let ffmpeg = Ffmpeg::new(working(dir.path()));
    let Err(Error::Port { message, .. }) = ffmpeg.encode(&job(&dir.path().join("clip.avi"), None), &mut one()) else {
        unreachable!("accepted")
    };
    assert!(message.contains("avi") && message.contains("mp4"), "{message}");
    let Err(Error::Port { message, .. }) =
        ffmpeg.encode(&job(&out, None), &mut vec![frame(3, 2, [0.0; 4])].into_iter())
    else {
        unreachable!("accepted")
    };
    assert!(message.contains("3 by 2") && message.contains("even"), "{message}");
    assert!(
        matches!(
            ffmpeg.encode(&job(&out, None), &mut Vec::new().into_iter()),
            Err(Error::Port { .. })
        ),
        "no frames"
    );
    let mixed = vec![frame(2, 2, [0.0; 4]), frame(4, 2, [0.0; 4])].into_iter();
    let Err(Error::Port { message, .. }) = ffmpeg.encode(&job(&out, None), &mut { mixed }) else {
        unreachable!("accepted")
    };
    assert!(message.contains("4 by 2") && message.contains("2 by 2"), "{message}");
    let Err(Error::Port { message, .. }) = ffmpeg.encode(
        &Job {
            codec: "vp9",
            ..job(&out, None)
        },
        &mut one(),
    ) else {
        unreachable!("accepted")
    };
    assert!(message.contains("vp9") && message.contains("h264"), "{message}");
}

#[test]
fn a_gif_and_a_png_are_written_with_their_own_arguments_and_no_sound() {
    for (name, expected) in [("clip.gif", ["-f", "gif"]), ("sheet.png", ["-f", "image2"])] {
        let dir = TempDir::new().unwrap_or_else(|e| unreachable!("{e}"));
        let out = dir.path().join(name);
        let audio = Audio {
            rate: 8000,
            channels: 1,
            samples: &[0.0],
        };
        Ffmpeg::new(working(dir.path()))
            .encode(&job(&out, Some(audio)), &mut vec![frame(2, 2, [0.0; 4])].into_iter())
            .unwrap_or_else(|e| unreachable!("{e}"));
        let args = arguments(dir.path());
        assert!(has(&args, &expected), "{name}: {args:?}");
        assert!(!args.iter().any(|a| a == "-c:a" || a == "wav"), "{name} holds no sound");
    }
}

fn left_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_job_the_encoder_refuses_leaves_no_soundtrack_file_behind() {
    let dir = TempDir::new().unwrap_or_else(|e| unreachable!("{e}"));
    let out = dir.path().join("clip.mp4");
    let audio = Audio {
        rate: 8000,
        channels: 1,
        samples: &[0.0; 4],
    };
    let result = Ffmpeg::new(working(dir.path())).encode(
        &Job {
            codec: "vp9",
            ..job(&out, Some(audio))
        },
        &mut vec![frame(2, 2, [0.0; 4])].into_iter(),
    );
    assert!(matches!(result, Err(Error::Port { .. })));
    assert_eq!(left_in(dir.path()), vec!["fake-ffmpeg".to_string()]);
}

#[test]
fn a_file_that_cannot_be_moved_into_place_leaves_no_temporary_file_behind() {
    let dir = TempDir::new().unwrap_or_else(|e| unreachable!("{e}"));
    let out = dir.path().join("clip.mp4");
    fs::create_dir(&out).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(out.join("inside"), b"x").unwrap_or_else(|e| unreachable!("{e}"));
    let audio = Audio {
        rate: 8000,
        channels: 1,
        samples: &[0.0; 4],
    };
    let result =
        Ffmpeg::new(working(dir.path())).encode(&job(&out, Some(audio)), &mut vec![frame(2, 2, [0.0; 4])].into_iter());
    let Err(Error::Port { message, .. }) = result else {
        unreachable!("accepted")
    };
    assert!(message.contains("clip.mp4"), "{message}");
    assert_eq!(
        left_in(dir.path()),
        vec![
            "args".to_string(),
            "clip.mp4".to_string(),
            "fake-ffmpeg".to_string(),
            "sound".to_string()
        ],
        "only the stand-in's own files and the target"
    );
}
