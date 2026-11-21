//! The ffmpeg adapter of the encode port: ffmpeg runs as a separate program, fed raw frames on its standard input.
//!
//! Never linked: a program needs no C headers at build time, puts no GPL encoder into our binary, and a missing one is one clear
//! error. Frames are flattened over black and written as 16-bit RGB; ffmpeg's own scaler dithers them down to the codec's 8 or 10
//! bits (BT.709, limited range, tagged so players agree), which is where banding in dark gradients is decided.

use std::ffi::OsString;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::error::Error;
use crate::ports::{Audio, Encode, Job};
use crate::types::Frame;

/// How many characters of ffmpeg's complaint an error carries (the end, where the cause is).
const STDERR_TAIL: usize = 2000;

/// Writes video files by running `ffmpeg`.
#[derive(Debug, Clone)]
pub struct Ffmpeg {
    program: PathBuf,
}

impl Ffmpeg {
    /// An adapter that runs `program` (a name looked up on `PATH`, or a path).
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    /// The program named by `BT_VIDEO_FFMPEG`, else `ffmpeg` on `PATH`.
    #[must_use]
    pub fn discover() -> Self {
        Self::new(std::env::var_os("BT_VIDEO_FFMPEG").unwrap_or_else(|| "ffmpeg".into()))
    }
}

/// The container, from the file's extension: its ffmpeg name and what it can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Container {
    Mp4,
    Mov,
    Gif,
    Png,
}

impl Container {
    fn of(path: &Path) -> Result<Self, Error> {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("mp4") => Ok(Self::Mp4),
            Some("mov") => Ok(Self::Mov),
            Some("gif") => Ok(Self::Gif),
            Some("png") => Ok(Self::Png),
            other => Err(port(format!(
                "cannot write `{}`: the extension {other:?} is not one of mp4, mov, gif, png",
                path.display()
            ))),
        }
    }
}

impl Encode for Ffmpeg {
    fn encode(&self, job: &Job<'_>, frames: &mut dyn Iterator<Item = Result<Frame, Error>>) -> Result<(), Error> {
        let container = Container::of(job.path)?;
        let first = frames
            .next()
            .ok_or_else(|| port("there are no frames to encode".into()))??;
        let (width, height) = (first.width(), first.height());
        if matches!(container, Container::Mp4 | Container::Mov)
            && !(width.is_multiple_of(2) && height.is_multiple_of(2))
        {
            return Err(port(format!(
                "a frame of {width} by {height} cannot be encoded as video: the sides must be even"
            )));
        }
        let part = sibling(job.path, "part");
        let audio_file = job
            .audio
            .as_ref()
            .filter(|_| matches!(container, Container::Mp4 | Container::Mov))
            .map(|_| sibling(job.path, "audio.part"));
        // removed however this returns: a refused job, a failed write, a failed rename (the renamed file is gone from `part`)
        let _cleanup = Cleanup([Some(part.clone()), audio_file.clone()].into_iter().flatten().collect());
        if let (Some(audio), Some(file)) = (&job.audio, &audio_file) {
            std::fs::write(file, wav(audio))
                .map_err(|e| port(format!("cannot write the soundtrack to `{}`: {e}", file.display())))?;
        }
        let arguments = arguments(job, container, (width, height), audio_file.as_deref(), &part)?;
        self.run(&arguments, first, frames)?;
        std::fs::rename(&part, job.path).map_err(|e| {
            port(format!(
                "cannot move the finished file to `{}`: {e}",
                job.path.display()
            ))
        })
    }
}

/// Files removed when it goes out of scope: the temporary names a write leaves beside its target.
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Ffmpeg {
    /// Runs the program, streams the frames to it and waits; ffmpeg's own message is the error if it fails.
    fn run(
        &self,
        arguments: &[OsString],
        first: Frame,
        rest: &mut dyn Iterator<Item = Result<Frame, Error>>,
    ) -> Result<(), Error> {
        let mut child = Command::new(&self.program)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                ErrorKind::NotFound => port(format!(
                    "ffmpeg was not found (looked for `{}`): install it or set BT_VIDEO_FFMPEG",
                    self.program.display()
                )),
                _ => port(format!("cannot run `{}`: {e}", self.program.display())),
            })?;
        let mut stderr = child.stderr.take();
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(stream) = stderr.as_mut() {
                let _ = stream.read_to_string(&mut text);
            }
            text
        });
        let fed = feed(&mut child, first, rest);
        let status = child.wait();
        let said = reader.join().unwrap_or_default();
        match (fed, status) {
            (Err(error), _) => Err(error),
            (Ok(Fed::Closed), _) => Err(port(format!("ffmpeg stopped: {}", tail(&said)))),
            (Ok(Fed::All), Ok(status)) if status.success() => Ok(()),
            (Ok(Fed::All), Ok(status)) => Err(port(format!("ffmpeg failed ({status}): {}", tail(&said)))),
            (Ok(Fed::All), Err(e)) => Err(port(format!("cannot wait for ffmpeg: {e}"))),
        }
    }
}

/// How feeding the frames ended.
enum Fed {
    /// Every frame was written.
    All,
    /// ffmpeg closed its input first: it stopped, and its own message says why.
    Closed,
}

/// Writes the frames to the child's standard input and closes it. A frame that is an error kills the child and is the result.
fn feed(child: &mut Child, first: Frame, rest: &mut dyn Iterator<Item = Result<Frame, Error>>) -> Result<Fed, Error> {
    let Some(mut stdin) = child.stdin.take() else {
        return Err(port("ffmpeg has no input".into()));
    };
    let size = (first.width(), first.height());
    let mut next = Some(Ok(first));
    while let Some(frame) = next.take().or_else(|| rest.next()) {
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                let _ = child.kill();
                return Err(error);
            }
        };
        if (frame.width(), frame.height()) != size {
            let _ = child.kill();
            return Err(port(format!(
                "a frame of {} by {} follows frames of {} by {}",
                frame.width(),
                frame.height(),
                size.0,
                size.1
            )));
        }
        if stdin.write_all(&rgb48le(&frame)).is_err() {
            return Ok(Fed::Closed);
        }
    }
    Ok(Fed::All)
}

/// The arguments of the program for `job`: raw 16-bit RGB frames in, a file out at `target`.
fn arguments(
    job: &Job<'_>,
    container: Container,
    size: (usize, usize),
    audio: Option<&Path>,
    target: &Path,
) -> Result<Vec<OsString>, Error> {
    let mut args: Vec<OsString> = Vec::new();
    let mut push = |items: &[&str]| args.extend(items.iter().map(OsString::from));
    push(&[
        "-y",
        "-loglevel",
        "error",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb48le",
        "-s",
        &format!("{}x{}", size.0, size.1),
        "-r",
        &job.fps.to_string(),
        "-i",
        "-",
    ]);
    if let Some(audio) = audio {
        push(&["-f", "wav", "-i"]);
        args.push(audio.into());
    }
    let mut push = |items: &[&str]| args.extend(items.iter().map(OsString::from));
    match container {
        Container::Mp4 | Container::Mov => {
            let (encoder, pixel_format) = match (job.codec, job.depth) {
                ("h264", 8) => ("libx264", "yuv420p"),
                ("h264", 10) => ("libx264", "yuv420p10le"),
                ("hevc", 8) => ("libx265", "yuv420p"),
                ("hevc", 10) => ("libx265", "yuv420p10le"),
                (other, depth) => {
                    return Err(port(format!(
                        "no encoder for codec `{other}` at {depth} bits: h264 and hevc are written at 8 or 10"
                    )));
                }
            };
            // BT.709, limited range, from full-range sRGB: the matrix and the tags players read
            push(&[
                "-vf",
                "scale=in_range=full:out_range=tv:out_color_matrix=bt709:flags=accurate_rnd+full_chroma_int",
            ]);
            push(&[
                "-c:v",
                encoder,
                "-preset",
                "medium",
                "-crf",
                "18",
                "-pix_fmt",
                pixel_format,
            ]);
            if (job.codec, job.depth) == ("h264", 8) {
                // High profile: what every phone decodes; 10-bit has no profile that plays on most
                push(&["-profile:v", "high"]);
            }
            push(&[
                "-colorspace",
                "bt709",
                "-color_primaries",
                "bt709",
                "-color_trc",
                "bt709",
                "-color_range",
                "tv",
            ]);
            if job.codec == "hevc" {
                push(&["-tag:v", "hvc1"]);
            }
            if audio.is_some() {
                // pad the sound with silence to the picture, then stop with the picture: the two end together
                push(&["-c:a", "aac", "-b:a", "192k", "-af", "apad", "-shortest"]);
            }
            push(&[
                "-movflags",
                "+faststart",
                "-f",
                if container == Container::Mp4 { "mp4" } else { "mov" },
            ]);
        }
        Container::Gif => push(&[
            "-vf",
            "split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5",
            "-loop",
            "0",
            "-f",
            "gif",
        ]),
        Container::Png => push(&[
            "-frames:v",
            "1",
            "-c:v",
            "png",
            "-pix_fmt",
            "rgb24",
            "-update",
            "1",
            "-f",
            "image2",
        ]),
    }
    args.push(target.into());
    Ok(args)
}

/// A frame as 16-bit little-endian RGB, flattened over black (straight alpha times colour) and held to zero..one.
fn rgb48le(frame: &Frame) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(frame.width() * frame.height() * 6);
    for pixel in frame.pixels().as_chunks::<4>().0 {
        let alpha = pixel[3].clamp(0.0, 1.0);
        for &channel in &pixel[..3] {
            // constant: 16-bit full scale
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "held to zero..one, then rounded into u16"
            )]
            let value = (channel * alpha).clamp(0.0, 1.0).mul_add(65535.0, 0.5) as u16;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

/// The soundtrack as a 32-bit float WAV file.
fn wav(audio: &Audio<'_>) -> Vec<u8> {
    let data_len = audio.samples.len() * 4;
    let channels = audio.channels;
    #[expect(clippy::cast_possible_truncation, reason = "a soundtrack is far below 4 GiB")]
    let (data, frames) = (
        data_len as u32,
        (audio.samples.len() / usize::from(channels.max(1))) as u32,
    );
    let mut bytes = Vec::with_capacity(data_len + 58);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(48 + data).to_le_bytes()); // WAVE, fmt (24), fact (12), the data header (8), the data
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&audio.rate.to_le_bytes());
    bytes.extend_from_slice(&(audio.rate * u32::from(channels) * 4).to_le_bytes());
    bytes.extend_from_slice(&(channels * 4).to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(b"fact");
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&frames.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for sample in audio.samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

/// `path` with `.suffix` added to its file name, beside it.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map_or_else(OsString::new, OsString::from);
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}

/// The end of ffmpeg's message.
fn tail(text: &str) -> String {
    let text = text.trim();
    let start = text.char_indices().rev().nth(STDERR_TAIL).map_or(0, |(i, _)| i);
    text[start..].to_string()
}

fn port(message: String) -> Error {
    Error::Port {
        port: "encode",
        message,
    }
}
