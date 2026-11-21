"""A piece: pictures on a timeline, a song under them, signals that drive their values, rendered into a video file.

piece = Piece(Profile.vertical(width=720, fps=30))
mill = piece.clip("mill", scene)                      # a picture; its addresses are the scene's
music = piece.song(sound, around_drop(before=4.0, length=10.0), fade_out=0.5)
mill.map("glow.look.swell.amount", 0.05 + 0.35 * signal)
piece.render("mill.mp4")
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any, Protocol, TypeVar

import numpy as np

from ._native import _core
from .adapters.audio import AudioMusic, AudioSound
from .adapters.scene_compose import SceneCompose
from .errors import BullterrierVideoError, InvalidError
from .music import Music
from .ramp import Ramp
from .transitions import Cut, Dissolve, Sweep

if TYPE_CHECKING:
    from collections.abc import Callable, Mapping, Sequence

    from numpy.typing import NDArray

    from ._native._core import Profile, Span

__all__ = ["DEFAULT_SHEET_WIDTH", "DEFAULT_WORKERS", "Addressed", "Clip", "Piece", "RenderStats", "render_all"]

# Frames drawn at once. Scene and image already use every core inside one frame, so more workers only overlap the
# short serial parts of a frame. Measured on the 300-frame mill (720 by 1280): 1 worker 77.6 s and 3.2 GB peak,
# 2 workers 48.6 s, 4 workers 38.1 s and 4.0 GB peak, the file identical at each.
DEFAULT_WORKERS = 4

# The long side in pixels of one cell of a contact sheet: five phone-shaped moments fit a screen at this size.
DEFAULT_SHEET_WIDTH = 240


class Addressed(Protocol):
    """Anything with an `address` string: a typed parameter handle, accepted wherever an address is."""

    @property
    def address(self) -> str:
        """The address string."""
        ...


# Mapping keys are invariant: a typed `dict[str, float]` needs a bound, not a union. A literal mixing strings and
# handles joins to `object` and is refused: its caller annotates it `dict[str | Addressed, object]`.
_Key = TypeVar("_Key", bound="str | Addressed")


def _address(key: str | Addressed) -> str:
    """The address string of `key`, a string or an `Addressed`."""
    if isinstance(key, str):
        return key
    address = getattr(key, "address", None)
    if not isinstance(address, str):
        msg = f"an address is a str or an object with a str `.address`, not {type(key).__name__}"
        raise TypeError(msg)
    return address


@dataclass(frozen=True)
class RenderStats:
    """What a render did: the `frames` it wrote, how many of them it had to draw and how many of those it composed.

    A frame whose clips, values and camera equal the frame before it is not drawn again, so `drawn` is less than
    `frames` for a piece that holds still (a poster that pauses, a gap, a title that sits). `composed` counts the drawn
    frames that were put together by the compose adapter: a transition, or a camera that is not at rest.
    """

    frames: int
    drawn: int
    composed: int


def _signal(signal: Any) -> Any:
    """`signal` as an object with `sample(times)`: a plain function of the times becomes one."""
    if callable(signal) and not hasattr(signal, "sample"):
        return _FunctionSignal(signal)
    return signal


class _FunctionSignal:
    """A plain function of the times (array in, array out) as a signal."""

    def __init__(self, function: Callable[[NDArray[np.float64]], Any]) -> None:
        self._function = function

    def sample(self, times: NDArray[np.float64]) -> NDArray[np.float64]:
        return np.asarray(self._function(times), dtype=np.float64)


class Clip:
    """A picture on a piece's timeline. Made by `Piece.clip`, found again with `piece[name]`; its methods chain."""

    def __init__(self, native: _core.Piece, name: str) -> None:
        """A handle on the clip `name` of the piece behind `native`."""
        self._native = native
        self._name = name

    @property
    def name(self) -> str:
        """The clip's name."""
        return self._name

    def map(self, address: str | Addressed, signal: Any, *, lag: float = 0.0) -> Clip:
        """Drives `address` (a string or `Addressed` handle) with `signal`, `lag` late.

        `signal` is anything with `sample(times)`, or a plain function from an array of seconds to an array of values.
        A colour address takes a `Ramp(stop, stop, ..., signal)` instead; a ramp is refused for any other address.

        The value is held to the parameter's described range: a cyclic one wraps, a whole-number one rounds, a bool
        is on from one half; a pixel value is never scaled. A text, choice or picture is `set`, not mapped. The
        address is refused, with the nearest names, when the picture lacks it or it is mapped or set already.
        """
        if isinstance(signal, Ramp):
            self._native.map_ramp(self._name, _address(address), list(signal.stops), _signal(signal.signal), lag)
        else:
            self._native.map(self._name, _address(address), _signal(signal), lag)
        return self

    def set(self, values: Mapping[_Key, object]) -> Clip:
        """Sets constants by address (a string or `Addressed` handle): numbers, bools, text, colours, choices, pictures.

        Nothing is set if any of them is refused.
        """
        self._native.set(self._name, {_address(k): v for k, v in values.items()})
        return self

    def swap(self, picture: Any) -> Clip:
        """Puts another picture in the clip. Refused, naming the address, if it lacks one that is set or mapped."""
        self._native.swap(self._name, picture)
        return self


class Piece:
    """A video being made. Builder calls change it and chain; `copy()` is cheap, so a variant leaves its source be."""

    def __init__(self, profile: Profile, *, loop: bool = False) -> None:
        """An empty piece that will be written with `profile`.

        `loop=True` makes the clock a circle: the frame after the last would be the first. The render is refused,
        before any frame is drawn, when the piece has more than one clip (so no transition), a song with a fade in or
        out, or any mapping or camera signal whose value at the start differs from its value at `duration`
        (a cyclic address closes at its wrap: 360 is 0). Make the song's span whole bars, so the sound closes too.
        """
        self._native = _core.Piece(profile, loop)
        self.profile = profile

    def clip(self, name: str, source: Any, start: float = 0.0, end: float | None = None) -> Clip:
        """Puts a picture on the timeline as `name`, from `start` to `end` seconds (`None`: until the next clip).

        `source` is a scene, or anything with `parameters()` (a dict from address to a description with `kind`, `min`,
        `max`, `cyclic`...), `with_values(mapping)` returning the picture with those values, and `render(long_side)`
        returning a float32 `(height, width, 4)` array of the profile's shape. Clips follow one another (a cut); clips
        that overlap are refused.
        """
        self._native.clip(name, source, start, end)
        return Clip(self._native, name)

    def __getitem__(self, name: str) -> Clip:
        """The clip named `name`; an `InvalidError` lists the clips there are."""
        self._native.check_clip(name)
        return Clip(self._native, name)

    def song(self, sound: Any, span: Span, *, fade_in: float = 0.0, fade_out: float = 0.0) -> Music:
        """Puts a song under the piece: `sound` (a `bullterrier_audio.Sound`) cut to `span`, faded, set to the loudness.

        Returns what the song knows in the piece's own seconds. A piece has one song: build it again for another span.
        """
        return Music(self._native.song(AudioMusic(sound), AudioSound(sound), span, fade_in, fade_out))

    @property
    def duration(self) -> float:
        """How long the piece lasts: its song's span, else the end of its last clip."""
        return self._native.duration()

    def frame(self, t: float) -> NDArray[np.float32]:
        """The frame at `t` seconds, a float32 `(height, width, 4)` array, straight alpha."""
        return self._native.frame(t)

    def render(self, path: str | os.PathLike[str], *, workers: int | None = None) -> RenderStats:
        """Renders the piece into the file `path` (mp4, mov, gif or png by its extension) with ffmpeg.

        Frames are drawn on `workers` threads (`DEFAULT_WORKERS` if `None`) and reach the encoder in order, so the file
        does not depend on the number. A frame equal to the one before it is not drawn again. The file appears only when
        ffmpeg succeeded. ffmpeg is the program named by `BT_VIDEO_FFMPEG`, else `ffmpeg` on `PATH`.
        """
        frames, drawn, composed = self._native.render(os.fspath(path), DEFAULT_WORKERS if workers is None else workers)
        return RenderStats(frames, drawn, composed)

    def draft(self, scale: float = 0.5, *, fps: float | None = None) -> Piece:
        """The same piece smaller (each side `scale` times, to the nearest even number) and, with `fps`, at that rate.

        Pictures draw at any size, so a draft costs roughly `scale` squared of a render: a quick look before the real
        one. Time is in seconds, so a lower `fps` costs nothing else. This piece is left as it was. Use a scale that
        keeps whole pixels: a picture whose shape does not scale evenly is refused when it draws, naming both sizes.
        """
        return Piece._made(self._native.draft(scale, fps))

    def sheet(
        self,
        path: str | os.PathLike[str],
        at: Sequence[float],
        width: int = DEFAULT_SHEET_WIDTH,
        *,
        columns: int | None = None,
    ) -> None:
        """Writes a contact sheet: the piece at each moment in `at` (seconds), side by side, to the picture `path`.

        With `columns` the cells wrap into rows of that many (the last row padded with the sheet's grey).

        Each cell is drawn `width` pixels along its long side, so a sheet takes a few frames' time, not a render's. A
        moment no clip draws is a black cell. Refused if `at` is empty, `columns` is under one, or a moment is not
        inside the piece (its last frame is at `duration - 1/fps`).
        """
        self._native.sheet(os.fspath(path), list(at), width, columns)

    def camera(self, dx: Any = None, dy: Any = None, zoom: Any = None, rotate: Any = None) -> Piece:
        """Moves the finished frame: each argument a signal (anything `Clip.map` takes), or `None` to stay at rest.

        `dx` and `dy` are fractions of the frame's long side (so a draft and a final render agree), `zoom` is a scale
        about the centre (1: none) and `rotate` is degrees clockwise. Shake is a sum of signals. A camera is one move
        of every layer of the frame: nothing is drawn again, so a scene's cache holds, and a frame whose camera is at
        rest is not composed at all. A second call replaces the camera. Needs `bullterrier_scene`: a `PortError` says so
        when it is missing.
        """
        self._with_compose()
        self._native.camera(*(None if s is None else _signal(s) for s in (dx, dy, zoom, rotate)))
        return self

    def transition(
        self, a: str | Clip, b: str | Clip, at: float, length: float, kind: Cut | Dissolve | Sweep | None = None
    ) -> Piece:
        """From clip `a` to clip `b` (clips or their names), centred on `at` seconds and `length` seconds long.

        `kind` is a `Cut` (the default), a `Dissolve` or a `Sweep`.

        Both clips are drawn during the window, outside their own spans if need be (signals are total). Clips may
        overlap only inside a transition that covers the overlap; two transitions do not overlap each other. Needs
        `bullterrier_scene` for a `Dissolve` or a `Sweep`.
        """
        match kind if kind is not None else Cut():
            case Cut():
                spec: tuple[Any, ...] = ("cut",)
            case Dissolve():
                self._with_compose()
                spec = ("dissolve",)
            case Sweep(kind=edge, angle=angle, feather=feather):
                self._with_compose()
                spec = (edge, angle, feather)
            case other:
                message = f"a transition kind is Cut(), Dissolve() or Sweep(...), got {other!r}"
                raise InvalidError(message)
        self._native.transition(
            a if isinstance(a, str) else a.name, b if isinstance(b, str) else b.name, at, length, *spec
        )
        return self

    def _with_compose(self) -> None:
        """Gives the piece its compose adapter, made when a call first needs it (scene is imported then, once)."""
        self._native.compose(SceneCompose())

    def copy(self) -> Piece:
        """An independent piece sharing the pictures, signals and song: nothing the copy changes touches this one."""
        return Piece._made(self._native.copy())

    @classmethod
    def _made(cls, native: _core.Piece) -> Piece:
        """A piece around an existing core piece, with the profile that piece has."""
        piece = cls.__new__(cls)
        piece._native = native  # noqa: SLF001  (an alternate constructor sets its own class's attribute)
        piece.profile = native.profile()
        return piece


def render_all(
    pieces: Mapping[str, Piece], folder: str | os.PathLike[str], *, workers: int | None = None, extension: str = "mp4"
) -> dict[str, Path]:
    """Renders every piece into `folder` as `<name>.<extension>`, one after another, each with all `workers`.

    Returns the file of each name. The folder is made if it is missing. A name must be a plain file name. A piece that
    cannot be rendered stops the run: its error names the piece, and the files made before it are kept.
    """
    for name in pieces:
        if not name or Path(name).name != name or name in {".", ".."}:
            message = f"a piece's name is its file name, so it cannot be `{name}`: no folders, not empty"
            raise InvalidError(message)
    target = Path(folder)
    target.mkdir(parents=True, exist_ok=True)
    made: dict[str, Path] = {}
    for name, piece in pieces.items():
        file = target / f"{name}.{extension}"
        try:
            piece.render(file, workers=workers)
        except BullterrierVideoError as error:
            raise type(error)(f"piece `{name}`: {error}") from error
        made[name] = file
    return made
