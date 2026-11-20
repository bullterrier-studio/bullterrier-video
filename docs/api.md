# The code API

Video decides what happens when, what drives what, and renders it. **The code API is the product**; it is designed from the
programs in [examples/](examples/): a mill video, the same piece in several styles, a growing-letter poster, a
music clip from footage, a loop, a template run for many posts. A hand-written driver for such a piece types about 50 addresses a
frame, clamps by hand, adds one shake to twelve layers and needs a shell script for the soundtrack and ffmpeg; the mill here is 23
lines of choreography.

**What is built:** the ports and video's own types (Rust, `crates/core/src/ports/`, `types.rs`), with fakes in their tests.
Nothing else yet: the Python surface below is the contract the next batches build. The first piece to run is the mill.

## Principles

1. **Video's own concepts, outside libraries behind ports.** A piece, a clip, music, a mapping, a camera, a profile are video's.
   A scene, a signal, a sound and an encoder are met through a trait of video's own (a *port*); one adapter per library. A
   sibling that changes or is replaced touches its adapter only. Video does not mirror a sibling's API.
2. **Python objects are never serialised.** An adapter wraps any Python object that has the port's methods (duck typing), so an
   in-memory picture or a callback works and scene's cache survives from frame to frame. Why, and the options weighed:
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
| `Piece(profile, *, loop=False)` | an empty piece; `duration` is the soundtrack's span (or the clips' end) |
| `Piece.clip(name, source, start=0.0, end=None, *, track=0, fill=None) -> Clip` | puts a picture on the timeline; `source` is a Picture (a scene, a drawing, anything with the port's methods) or a `Footage`; `fill={address: value}` sets holes, a `Footage` value giving a new frame each time |
| `Piece.song(sound, span, *, fade_in=0.0, fade_out=0.0) -> Music` | cuts the sound to `span`, fades it, puts it under the piece, returns what it knows in the piece's own time |
| `Piece.transition(a, b, at, length, kind=Cut())` | from clip `a` to clip `b`, centred on `at`; `kind`: `Cut`, `Dissolve`, `Sweep("light" or "dark", angle=0)` |
| `Piece.camera(dx=None, dy=None, zoom=None, rotate=None)` | one transform over the whole composition; each a signal (`dx`, `dy` as fractions of the long side, `zoom` one for none, `rotate` degrees) |
| `Piece.render(path, *, workers=None)` | the file; frames in parallel; the container from the extension |
| `Piece.frame(t, long_side) -> ndarray` | one moment, float32 RGBA |
| `Piece.sheet(path, at, width)` | a contact sheet of chosen moments, each `width` pixels along its long side, in seconds |
| `Piece.copy()` | an independent piece sharing the pictures and their caches |
| `render_all(pieces, folder, *, workers=None)` | one file per entry of a mapping name to piece, sharing one worker budget |
| `Clip.map(address, signal, *, lag=0.0)` | see "Mapping" |
| `Clip.set(values)` | constants by address (numbers, text, colours, pictures); the picture's own values until then |
| `Clip.swap(picture)` | another picture in the clip, mappings kept where its addresses exist |

**Music** (`Piece.song` returns it; every time is in piece seconds, so a clip's `start` can be `m.drop - 1.0`):
`duration`, `tempo` (beats a minute), `beats`, `downbeats`, `drop` (the strongest, or `None`), `kicks`, `snares`, `hats`,
`sections` (each `start`, `end`, `label`), `band(name, *, over=None)` (a signal between 0 and 1; `over=(a, b)` scales it to
the song's range inside that span, as the mill needs for the bass after the drop), `after(t)` (a signal: 0 before `t`, 1 from it).

**Spans** choose the soundtrack from the song: `around_drop(before, length)`, `bars(n)` (from the first downbeat, so a loop
closes on the beat), `whole()`, `between(start, end)`, `loudest(band, length)`.

**Profile:** `Profile.vertical(width, fps)` (9:16, safe area 12 to 80 percent of the height), `.landscape`, `.square`; fields
`width`, `height`, `fps`, `depth` (8 or 10 bit), `codec`, `loudness` (LUFS, delivery default), `safe`. The scene's own `fit` and
`cover` make the crop; video never crops afterward.

**Footage:** `Footage(path, start=0.0)`: a source of frames from `start` seconds into the file, handed to a scene through a
picture hole. Waits for the decode port.

### Mapping

`clip.map(address, signal, lag=0.0)` sets the value at `address` to the signal's value at `t - lag` (a negative lag leads) for
every frame of the clip.

- The address is the picture's own, as `scene.parameters()` lists it (`logo.drawing.ring_on`).
- The value is held to the described range; a **cyclic** parameter wraps into it instead (an angle of 360 reads as 0); a pixel
  value is read at the picture's own design size, so a preview and a final render agree.
- A signal is control's (or anything with `sample(times)`); signals add, subtract and multiply with numbers and each other, so a
  mix is an expression.
- Refused, naming the address: one that the picture does not have (with the nearest names), a text or picture parameter
  (those are `set`), an address mapped twice or both mapped and set.
- A clip may be drawn outside its own span (a transition needs both pictures); its signals are total, so this is safe.

## Soundtrack

One song per piece for now: cut to the span, faded, muxed in step with the frames, normalised to the profile's loudness. A
looping piece loops the sound on the bar. Mixing several sounds (voice-over, ducking, crossfades) is video's work through the Sound
port and comes after the first pieces.

## Errors

`BullterrierVideoError` is the base. `InvalidError`: a piece that cannot be rendered (an unknown address, a mapped twice, a
span outside the song, a loop whose seam differs, naming the first mapping whose ends differ). `PortError`: a library behind a
port refused or lacks a method, naming the port and the method (`picture: the object has no with_values`). Every message names
the thing and the fix.

## The ports (Rust, `bullterrier_video::ports`)

Each is a trait of video's own, `Send + Sync` so frames can be drawn from several threads. The contract is in the trait's
comments; video's own types (`Frame`, `Parameter`, `Kind`, `Value`, `View`, `Hit`, `Band`, `Moment`, `Section`) cross them and
name no sibling's type.

| Port | Promises | First adapter | Needed by |
|---|---|---|---|
| `Picture` | `parameters()` and `render(values, view, long_side)`: **one long-lived instance, rendered with new values each frame**, so its cache is kept | scene | slice 1 |
| `Signal` | `sample(times)`: a value at any time, many times in one call | control | slice 1 |
| `Music` | duration, beats, downbeats, drops, hits, sections, `band`: times in seconds and levels | audio | slice 1 |
| `Sound` | rate, channels, duration, `cut`, `fade`, `samples`: one sound edited at a time | audio | slice 1 |
| `Encode` | `encode(job, frames)`: a stream of frames and the soundtrack into one file | ffmpeg | slice 1 |
| Decode (sketch) | frames of a clip at times, with its audio | ffmpeg | footage |
| Footage maps (sketch) | a matte and a depth map per frame, steady over time | models | footage analysis |

**What a Python object must have to be an adapter's input** (an adapter refuses a missing method by name):

| Port | Methods of the Python object |
|---|---|
| Picture | `parameters()` (items with `address`, `kind`, `meaning`, `unit`, `min`, `max`, `cyclic`), `with_values(mapping)` returning a picture that shares the cache, `render(size)` returning float32 `(h, w, 4)`, straight alpha. A scene is this shape; a caller's own picture implements the three |
| Signal | `sample(times)` returning an array |
| Music and Sound | an audio `Sound` (measures and edits are one object in audio; two ports in video, so a replacement may supply either) |

**View (the camera).** `Picture::render` takes a `View`; the scene adapter makes it by drawing the scene as a group layer of a
wrapper scene (scene's `Layer.scene`), which costs no resampling of the finished frame. A picture that cannot take a view has
the frame moved afterward by the adapter. Proven with the first piece.

**Transitions** are described by video (kind, `at`, `length`) and realised through the picture port: both clips are drawn and
combined with a moving mask (and, for a sweep, a light layer) inside a wrapper scene. Proven with the first piece that uses one; if it needs a call scene
does not have, that is a request to scene.

## Dependencies

The **core crate depends on no sibling.** Its ports are traits, its types are its own. Adapters for pure-Rust callers (a
server, a command line) are separate crates (`bullterrier-video-scene`, `-control`, `-audio`, `-ffmpeg`); only they name a
sibling crate, and only the scene adapter would need image's and vector's pins, which scene fixes, so video never builds a second
copy. The Python package imports no sibling at import time. Git dependencies reach only pushed commits, so an adapter crate waits
until the sibling revision it uses is pushed. (Proposed; confirmed when the first adapter crate is made.)

## Not in the first release

Saved pieces (JSON): a piece holds Python objects, so it is built by a program, not loaded; a data form can come per port
when a sibling's own data form is lossless. Also later: a live camera, tracking, multi-frame effects, mixing several sounds.
