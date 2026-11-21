# Design decisions

This project's own decisions and their reasons, one section each: what was chosen, why, and what it costs. The
template's are in [template-decisions.md](template-decisions.md). This file is the project's: an update never changes it.

## Ports and adapters; Python objects stay Python (2026-11-20)

Every outside library (a scene, a signal, music, a sound, an encoder) is met through a trait of video's own, `bullterrier_video::ports`,
with its own small types (`Frame`, `Parameter`, `Kind`, `Value`, `View`), and one adapter per library. *Reason:* the sibling
libraries will break their APIs and one may be replaced; only an adapter then changes. A port exists only where an outside library
sits, and video does not mirror a sibling's API (that would be a pass-through layer and two APIs to keep).

**A Python caller's objects are not serialised.** A scene, a control signal and an audio sound built in Python live in the sibling's
own extension module, so video's cannot take them as Rust values. Options weighed: (1) *duck-typed ports (chosen):* the adapter
wraps any Python object that has the port's methods; (2) *data across the boundary* (scene JSON, a control patch, an audio bundle):
not lossless today (scene's `to_json` refuses a picture held as an array and a plugin layer, a patch refuses a callback, audio's
bundle leaves out drum hits); (3) *video links the sibling crates and re-exposes them:* a mirror of three Python APIs.
*Cost of (1):* Python calls in the frame loop for Python callers; scene's `render` releases the GIL, so frames still draw in
parallel threads, and a sibling that does not release it would need processes. A pure-Rust caller gets a Rust adapter, so the core has no Python.

**The core crate depends on no sibling.** Adapters for Rust callers are separate crates, so a sibling's pin never constrains the core.
The picture port is one long-lived instance rendered with new values each frame: scene's cache is worth 0.19 s a frame against 0.95 s
without, and a port that rebuilt per frame would throw it away.

**A compose port, and music as plain data (2026-11-20).** Tracks, transitions and the camera put finished frames together, which is
scene's job (layers, masks, blends), not the picture port's and not video's own pixel code: so `Compose` takes frames with a
placement, opacity, blend and mask, and the picture port has no view. Video makes the transition masks (arithmetic on its own
`Frame`). The camera moves the finished frame, so no layer is drawn again and scene's cache holds. Music returns times and
levels, not signals: a signal video made would be control's type in video's own return values, and control's functions take
only control's signals, so the caller makes them from the levels.

## ffmpeg is a program, not a library (2026-11-20)

The encode port's adapter runs `ffmpeg` as a separate process fed on its standard input; it is never linked (`ffmpeg-next`, libav). *Reasons:*
linking needs C headers at build time, makes the wheels bundle libav, complicates cross builds, puts GPL x264 inside our binary
(a common ffmpeg build is `--enable-gpl`), and neither form works in WebAssembly; a program adds no crate, keeps our licence
unaffected (GPL duties attach to whoever distributes a binary), and a pure-Rust user needs only `ffmpeg` on `PATH` or in
`BT_VIDEO_FFMPEG`, with one clear error when it is missing. *What can replace it later,* one adapter each: audio decoding is
already pure Rust (symphonia); an AV1 path could be rav1e with an mp4 muxer; there is no production permissive pure-Rust H.264,
HEVC or AAC encoder, so footage and social formats keep ffmpeg or an operating system's encoder. *Not checked:* whether
`ffmpeg-next`'s licence is in `deny.toml`'s allow list (believed WTFPL, which is not).

**Frames cross the pipe as 16-bit RGB flattened over black.** ffmpeg's scaler does the reduction to 8 or 10 bits and dithers it by
default (measured: a 16-bit dark ramp comes out as 8 distinct row patterns in 64, whatever `sws_dither` is set to), so video does no
dithering of its own and banding in dark gradients is decided where the colour conversion is. BT.709, limited range, with the
tags a player reads. The soundtrack goes in as a float WAV, padded with silence to the picture, so the two end together. The file
is written under a temporary name and renamed when ffmpeg succeeded; a failure returns the end of ffmpeg's own message.

## The frame loop (2026-11-20)

Every mapped signal is read once for all the frames of its clip, on the calling thread, before any frame is drawn (control's
`sample` takes arrays). Frames are drawn on N threads; a worker may be at most two frames per worker ahead of the one the
encoder wants next (`FRAMES_AHEAD_PER_WORKER`), so memory stays flat (a frame in flight is 16 bytes a pixel) and the file does not
depend on N. The Rust loop runs with the GIL released; each worker takes it only to call the picture (`with_values`, then scene's
own `render`, which releases it). `DEFAULT_WORKERS` is a named parameter, chosen by measurement (BTVID-0003's demo), because scene
and image already use every core inside one frame.

## Mapping rules (2026-11-20)

A mapped value is held to the parameter's described range: a cyclic one wraps, an integer rounds, a bool is on from one half. A pixel
value is not scaled: scene reads `px` as design pixels at any size, so scaling here would do it twice. A signal is read at
`max(t - lag, 0)`. A text, colour, choice or picture is set, never mapped; an address that is unknown (with the three nearest
names), mapped twice, or both mapped and set is refused when the call is made, not at render. `Music.band` scales over the
stretch asked for (the piece, or a part of it), not over the song, as a clip measured alone would be.

## A colour is mapped through a ramp video owns (2026-11-21)

A colour is a settable kind in image and scene, so a signal cannot drive it as a number, and three channel mappings would leave every caller
to decide what a blend is (per-channel mixes go muddy between hues). `Ramp(*stops, signal)` is video's: stops evenly spaced, the signal 0..1
picks the place, the mix is in OKLab (perceptually even), done once in the core with the exact sRGB transfer, ends equal the stops exactly.
The value handed to the picture is three floats (`Value::Color`), so a slow fade never steps in 8-bit hex. Stop positions are left out until a piece needs them.

## numpy and the audio adapter (2026-11-20)

numpy is a runtime dependency of the package and of the bindings crate (`deny.toml` names it a wrapper there): pictures, signals and
levels cross as arrays, and every sibling library uses it. The core names no numpy. The audio library is reached only through
`bullterrier_video.adapters.audio`, which is imported lazily and returns plain lists and arrays, so the core never learns a
sibling's method names, enums or array layout (audio gives one row a channel; the adapter interleaves).

## Contact sheets and batches (2026-11-20)

A sheet asks each picture to draw its moment at the cell size (`render(values, width)`), rather than shrinking a full frame, so it
costs a few small frames (five moments of the mill in 0.1 to 0.3 s against 38 s for the render) and the picture's own design-size
rules still hold. Cells are tiled in one row with `SHEET_GAP` pixels of dark grey between, cells must agree in size, and a moment no
clip draws is a black cell of their size. The adapter asks for even sides only of video (4:2:0 needs them), not of a picture or a
gif: a 135-pixel cell is normal. `render_all` is a plain loop in the package, one piece after another with all the workers (a piece
already uses every core), not a scheduler. A timing variant is the piece built again, because its mappings hold the song's moments;
so a piece's choreography is best written relative to the drop (the mill's intro scales with the build length), or the keyframes of a
shorter build run backwards, which the demo's first timing variant showed.

## Where the render time goes, and the cheap wins kept (2026-11-20)

Measured on the 300-frame mill (720 by 1280, one worker): drawing the frames 76.0 s, the whole render 77.6 s, the encoder alone about 1 s
for 300 frames, so video's own pipeline (values, quantising, the pipe) is about 2 percent and the picture is almost all of it.
Micro-optimising video would gain nothing; what can pay is not drawing, and drawing smaller. Kept for that: signals read once in
bulk, one long-lived picture (scene's per-layer cache), frames streamed to the encoder and never stored, `Frame` pixels private (so
half-float or 8-bit storage or another backend changes no API), a frame equal to the one before it drawn once (held poses, stills,
gaps), and `draft` (a smaller, optionally slower copy of the piece). The render returns counts (`frames`, `drawn`) and no timings: the
library keeps no clock (`docs/rules.md`), so a caller times the render, as the demos do. Deliberately later: sending the picture only
the values that changed, letting a picture say how many cores it wants (scene and image already use every core inside a frame, so four
workers gave 2.0 times, not 4), process workers for pictures that hold the GIL, hardware encoders (pointless while encoding costs a
second).

## Camera and transitions: compose in Python, the view on every layer (2026-11-21)

A frame is made of layers (the clips drawn for it), an optional mix (a dissolve or a sweep and its progress) and the camera's view;
that value is compared with the one before it, so a moving camera is never skipped and a held frame is drawn once. The Compose
port's first adapter is Python (`adapters/scene_compose.py`), not a Rust crate on scene: scene is not published and pins image
and vector by git, so a Rust dependency would be a second pin set to keep in step, and the Python path measured fast enough
(5 ms from array to raster at 1080p after scene's bulk copy; 110 ms before it, which is why the conversion was fixed in scene
and not worked round here). *The camera is a view on every layer of the stack, in one compose call,* not a second pass over
the finished frame: each layer is resampled once from its own pixels (a second pass would resample twice, and cost a second
array conversion), and the result is the same picture. A frame with one layer and a view at rest makes no compose call at all.
*Sweep's bar* is a layer of its own (white under screen, or black) with a mask that is the edge's: 0 away from it, 1 on it,
so a sweep is exactly the first clip at progress 0 and the second at 1. The masks are made in the core as frames (scene reads
the red channel as a map); the hand-built sweeps of the cenobite piece had an exposure lift and a bloom under the bar and a
ragged edge, which stay piece-level looks. *Counts, no clock:* `RenderStats.composed` counts the composed frames; a caller times
the render. The tests that need scene skip without it, so the gate stays green on a machine without the sibling.

## Loops: a circle checked at the seam, one clip, no fades (2026-11-21)

`Piece(profile, loop=True)` makes frame N equal frame 0, so the render compares what each mapping and camera signal gives at
0 s and at `duration`, as the picture would receive it (held to its range, a cyclic address wrapped; a cyclic gap is measured the
short way round, so a hair under 360 closes with 0), and refuses the first that differs, before any frame is drawn. A looped piece
is refused with more than one clip, any transition (which needs two clips) or a song fade above zero: each would show or be heard
at the seam, and cutting them silently would change the piece. The tolerance is `SEAM_TOLERANCE`, 1e-6 of the address's unit:
above the rounding of a periodic signal read at its last moment, below anything visible. The check is by sampling two moments, not
a proof of periodicity: a signal that agrees at both ends and wanders between is allowed, because that is a loop that moves.
Audio is not measured: the span's whole bars are the caller's, and `bars(n)` already ends on the beat.

