# The code API

Video decides what happens when, what drives what, and renders it. **The code API is the product**; it is designed from the
programs in [examples/](examples/): a mill video, the same piece in several styles, a growing-letter poster, a
music clip from footage, a loop, a template run for many posts. A hand-written driver for such a piece types about 50 addresses a
frame, clamps by hand, adds one shake to twelve layers and needs a shell script for the soundtrack and ffmpeg; the mill here is 23
lines of choreography.

The examples show the whole designed surface, including what is still planned; the built part is `examples/mill.py`'s calls.

**What is built:** the ports and video's own types; the core (profile, spans, a song in piece time, clips with their mappings, the
piece and its parallel frame loop, contact sheets, the ffmpeg adapter); the Python package (`Piece`, `Clip`, `Profile`, spans, `Music`,
`Levels`, `render_all`, the audio adapter), the mill piece and its variants on it. **Planned, not in the signatures yet:** `track`,
`fill`, `transition`, `camera`, `Footage`, `loop` (marked *planned* below); each arrives with the batch that needs it.

## Principles

1. **Video's own concepts, outside libraries behind ports.** A piece, a clip, music, a mapping, a camera, a profile are video's.
   A scene, a signal, a sound and an encoder are met through a trait of video's own (a *port*); one adapter per library. A
   sibling that changes or is replaced touches its adapter only. Video does not mirror a sibling's API.
2. **Python objects are never serialised.** An adapter wraps any Python object that has the port's methods (duck typing), so an
   in-memory picture works and scene's cache survives from frame to frame; a picture that is not a scene is a small class with the
   three methods below. Adapters video constructs itself (compose, encode) import their library. Why, and the options weighed:
   [design-decisions.md](design-decisions.md).
3. **Time is seconds.** Frames exist inside a render only. Musical time is a number of seconds read from the song (`m.drop`,
   `m.downbeats`), so it composes with every other signal.
4. **A mapping reads the parameter's description.** No clamping, no wrapping, no per-effect code in a piece.
5. **A piece is built, then rendered.** Builder calls mutate and return the piece; `copy()` is cheap, so a variant never
   touches its source.

## The surface (Python, `import bullterrier_video as btv`)

```python
piece = btv.Piece(btv.Profile.vertical(width=720, fps=30))  # 9:16 frame, rate, safe areas, codec, loudness
clip = piece.clip("mill", scene)  # a Picture on the timeline, named; its addresses are the scene's
m = piece.song(sound, span=btv.around_drop(before=4.0, length=10.0), fade_out=0.5)  # the soundtrack and what it knows
clip.map("glow.look.swell.amount", 0.05 + 0.35 * m.band("bass"))  # a signal drives an address
piece.render("mill.mp4")
```

| Call | Does |
|---|---|
| `Piece(profile)` | an empty piece; `duration` is the soundtrack's span (or the clips' end); `loop=` is *planned* |
| `Piece[name] -> Clip` | the clip of that name, to `set`, `map` or `swap` it |
| `Piece.clip(name, source, start=0.0, end=None) -> Clip` | puts a picture on the timeline (a cut from one clip to the next; overlap is refused); `source` is a scene or anything with the Picture port's methods. *Planned:* `track=`, a `Footage` source, `fill=` |
| `Piece.song(sound, span, *, fade_in=0.0, fade_out=0.0) -> Music` | cuts the sound to `span`, fades it, puts it under the piece, returns what it knows in the piece's own time; once: a second call is an `InvalidError` (a timing variant builds the piece again) |
| *Planned:* `Piece.transition(a, b, at, length, kind=Cut())` | from clip `a` to clip `b`, centred on `at`; `kind`: `Cut`, `Dissolve`, `Sweep("light" or "dark", angle=0)` |
| *Planned:* `Piece.camera(dx=None, dy=None, zoom=None, rotate=None)` | one transform over the finished composition; each a signal (`dx`, `dy` as fractions of the long side, `zoom` one for none, `rotate` degrees). The finished frame is moved once by the Compose port, so no layer is drawn again and scene's cache holds; the price is one resample of the frame |
| `Piece.render(path, *, workers=None)` | the file (mp4, mov, gif or png by its extension) through ffmpeg; frames drawn on `workers` threads (`DEFAULT_WORKERS`) and handed over in order, so the file does not depend on the number; written under a temporary name and renamed when ffmpeg succeeded; a frame equal to the one before it (same clip and values) is not drawn again, so a held pose or a still costs one drawing; returns `RenderStats(frames, drawn)` |
| `Piece.draft(scale=0.5, *, fps=None)` | the same piece smaller (each side `scale` times, to the nearest even number) and, with `fps`, slower: pictures draw at any size, so a draft costs about `scale` squared of a render (the mill, 38 s: half size 7.8 s, half size at 15 fps 4.5 s, quarter size at 15 fps 1.7 s); the piece itself is unchanged |
| `Piece.frame(t) -> ndarray` | one moment at the profile's size, float32 RGBA straight alpha |
| `Piece.sheet(path, at, width=240, *, columns=None)` | a contact sheet (png) of chosen moments side by side (or wrapped into rows of `columns`, the last row padded), each cell drawn `width` pixels along its long side by the picture itself, so it takes a few frames' time, not a render's; a moment no clip draws is a black cell; the last frame is at `duration - 1/fps`, a moment past it is refused naming it |
| `Piece.copy()` | an independent piece sharing the pictures and their caches |
| `render_all(pieces, folder, *, workers=None, extension="mp4")` | one file `<name>.<extension>` per entry of a mapping name to piece, one after another, each with all the workers; a piece that fails stops the run and its error names it |
| `Clip.map(address, signal, *, lag=0.0)` | see "Mapping" |
| `Clip.set(values)` | constants by address (numbers, text, colours, pictures); the picture's own values until then |
| `Clip.swap(picture)` | another picture in the clip; refused, naming the address, if it lacks one that is set or mapped (nothing changes then) |

**Music** (`Piece.song` returns it; every time is in piece seconds, so a clip's `start` can be `m.drop - 1.0`): `duration`, `tempo`
(beats a minute), `beats`, `downbeats`, `drop`, `kicks`, `snares`, `hats`, `sections` (each `start`, `end`, `label`), `band(name,
*, over=None) -> Levels`. It is **plain data**, never control's types: `Levels` is `values`, `rate` and `start`, and the caller makes a
signal with `btc.from_values(lv.values, lv.rate, start=lv.start)`. `band(..., over=(a, b))` scales the levels to the song's range
inside that span, as the mill needs for the bass after the drop. A measure the song cannot give raises `InvalidError` naming it:
`drop` when there is no drop, `downbeats` when the meter is unknown, the hits when no drum part can be found. Never an empty list
that looks like silence.

**Spans** choose the soundtrack from the song: `around_drop(before, length)`, `bars(n)` (from the first downbeat, so a loop
closes on the beat; an unknown meter raises), `whole()`, `between(start, end)`, `loudest(band, length)`.

**Profile:** `Profile.vertical(width, fps)` (9:16, safe area 12 to 80 percent of the height), `.landscape`, `.square`; fields
`width`, `height`, `fps`, `depth` (8 or 10 bit), `codec` (`h264` or `hevc`), `loudness` (LUFS, none by default), `safe`; the sides must be even. The scene's own `fit` and
`cover` make the crop; video never crops afterward.

**Footage** (*planned*): `Footage(path, start=0.0)`: a source of frames from `start` seconds into the file, handed to a scene
through a picture hole. Waits for the decode port.

### Mapping

`clip.map(address, signal, lag=0.0)` sets the value at `address` to the signal's value at `t - lag` (a negative lag leads) for
every frame of the clip.

- The address is the picture's own, as `scene.parameters()` lists it (`logo.drawing.ring_on`). `map` and `set` take it as a string or as any object with a str `.address` (`btv.Addressed`, a typed parameter handle); the handle's address is what errors name. A number, an integer (rounded) and
  a bool take a mapped signal; a colour, a choice, a text and a picture are `set`.
- The value is held to the described range; a **cyclic** parameter wraps into it instead (an angle of 360 reads as 0); a pixel
  value is read at the picture's own design size, so a preview and a final render agree.
- A signal is control's (or anything with `sample(times)`, or a plain function from an array of seconds to an array); signals add, subtract and multiply with numbers and each other, so a
  mix is an expression.
- Refused, naming the address: one that the picture does not have (with the nearest names), a text or picture parameter
  (those are `set`), an address mapped twice or both mapped and set.
- A moment no clip covers is black. Clips follow one another: a clip with no `end` runs to the next clip's start or the piece's end.
- *Planned:* a clip may be drawn outside its own span (a transition needs both pictures); its signals are total, so this is safe.

## Soundtrack

One song per piece for now: cut to the span, faded, set to the profile's loudness if it has one, muxed in step with the frames (the
sound is padded with silence to the picture and both end together). Mixing several sounds (voice-over, ducking, crossfades) is video's work through the Sound
port and comes after the first pieces.

## Errors

`BullterrierVideoError` is the base. `InvalidError`: a piece that cannot be rendered (an unknown address, mapped twice, a span
outside the song, a song with no drop, overlapping clips). `PortError`: a library behind a port refused or lacks a method, naming
the port and what it said (`picture: AttributeError: ... no attribute 'with_values'`), or ffmpeg's own words when it fails or is
missing. Every message names the thing and the fix.

## The ports (Rust, `bullterrier_video::ports`)

Each is a trait of video's own, `Send + Sync` so frames can be drawn from several threads. The contract is in the trait's
comments; video's own types (`Frame`, `Parameter`, `Kind`, `Value`, `View`, `Hit`, `Band`, `Moment`, `Section`) cross them and
name no sibling's type.

| Port | Promises | First adapter | Needed by |
|---|---|---|---|
| `Picture` | `parameters()` and `render(values, long_side)`: **one long-lived instance, rendered with new values each frame**, so its cache is kept | scene | slice 1 |
| `Compose` | `compose(layers)`: frames, each with a placement (a `View`), opacity, blend and mask, into one frame of the ground's size: tracks, transitions, the camera | scene (its layers take a frame as a picture) | the first piece with a transition (*not built*) |
| `Signal` | `sample(times)`: a value at any time, many times in one call | control | slice 1 |
| `Music` | duration, beats, downbeats, drops, hits, sections, `band`: times in seconds and levels; an unknown measure is an error, never an empty list | audio | slice 1 |
| `Sound` | rate, channels, duration, `cut`, `fade`, `samples`: one sound edited at a time | audio | slice 1 |
| `Encode` | `encode(job, frames)`: a stream of frames and the soundtrack into one file, at a rate, codec and bit depth | ffmpeg, as a separate program | slice 1 |
| Decode (sketch) | frames of a clip at times, with its audio | ffmpeg | footage |
| Footage maps (sketch) | a matte and a depth map per frame, steady over time | models | footage analysis |

**What a Python object must have to be an adapter's input** (an adapter refuses a missing method by name):

| Port | Methods of the Python object |
|---|---|
| Picture | `parameters()` (a mapping from address to an item with `kind`, `meaning`, `unit`, `min`, `max`, `cyclic`: scene's `dict[str, Parameter]`), `with_values(mapping)` returning a picture that shares the cache, `render(size)` returning float32 `(h, w, 4)`, straight alpha. A scene is this shape; a caller's own picture implements the three |
| Signal | `sample(times)` returning an array |
| Music and Sound | a `bullterrier_audio.Sound` (measures and edits are one object in audio; two ports in video, so a replacement may supply either). `bullterrier_video.adapters.audio` wraps it as the plain-data objects the core calls (`beats()`, `hits("kick")`, `band("bass", start, end)`, `cut`, `fade`, `normalised`, `samples()` interleaved); it is the one file that names the audio library |

**Camera, tracks and transitions go through `Compose`.** Video draws each clip with the Picture port, then stacks the frames: a
title is a layer on a second track; a transition is the outgoing frame, the incoming frame with a **mask video generates** (a
soft gradient edge at an angle, moved by the transition's progress), and for a sweep a white layer under `Screen` with the same
mask; the camera is the finished frame placed by one `View`. Video makes the masks (they are arithmetic on its own `Frame`); scene
does the blend and the placement. The scene adapter is the one that builds layers, so it imports scene (its Python adapter,
lazily) and it is not duck-typed.

## ffmpeg

ffmpeg runs as a **separate program**, never linked: the encoder port's first adapter feeds it raw 16-bit RGB frames (flattened over
black) on its standard input and a float WAV of the soundtrack, and ffmpeg's own scaler dithers down to the codec's 8 or 10 bits
(BT.709, limited range, tagged so players agree). It is the program named by `BT_VIDEO_FFMPEG`, else `ffmpeg` on `PATH`; a missing
one is a `PortError` that says so. Containers: mp4, mov (h264, hevc at 8 or 10 bits), gif, png. Why a program and not a library:
[design-decisions.md](design-decisions.md).

## Dependencies

The **core crate depends on no sibling and no crate beyond the standard library.** Its ports are traits, its types are its own, and
the ffmpeg adapter is a module in it (it adds nothing to link). Adapters for pure-Rust callers that name a sibling (a scene as a
Picture) are separate crates, so only they need image's and vector's pins, which scene fixes, so video never builds a second copy.
The Python package imports no sibling at import time; `bullterrier_audio` is imported when a song is put under a piece.

## Not in the first release

Saved pieces (JSON): a piece holds Python objects, so it is built by a program, not loaded; a data form can come per port
when a sibling's own data form is lossless. Also later: a live camera, tracking, multi-frame effects, mixing several sounds.
