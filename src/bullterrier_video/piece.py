"""A piece: pictures on a timeline, a song under them, signals that drive their values, rendered into a video file.

piece = Piece(Profile.vertical(width=720, fps=30))
mill = piece.clip("mill", scene)                      # a picture; its addresses are the scene's
music = piece.song(sound, around_drop(before=4.0, length=10.0), fade_out=0.5)
mill.map("glow.look.swell.amount", 0.05 + 0.35 * signal)
piece.render("mill.mp4")
"""

from __future__ import annotations

import os
from typing import TYPE_CHECKING, Any

from ._native import _core
from .adapters.audio import AudioMusic, AudioSound
from .music import Music

if TYPE_CHECKING:
    from collections.abc import Mapping

    import numpy as np
    from numpy.typing import NDArray

    from ._native._core import Profile, Span

__all__ = ["DEFAULT_WORKERS", "Clip", "Piece"]

# Frames drawn at once. Scene and image already use every core inside one frame, so more workers only overlap the
# short serial parts of a frame. Measured on the 300-frame mill (720 by 1280): 1 worker 77.6 s and 3.2 GB peak,
# 2 workers 48.6 s, 4 workers 38.1 s and 4.0 GB peak, the file identical at each.
DEFAULT_WORKERS = 4


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

    def map(self, address: str, signal: Any, *, lag: float = 0.0) -> Clip:
        """Drives `address` with `signal` (anything with `sample(times)`, a control signal), read `lag` seconds late.

        The value is held to the parameter's described range: a cyclic one wraps, a whole-number one rounds, a bool
        is on from one half; a pixel value is never scaled. A text, colour, choice or picture is `set`, not mapped. The
        address is refused, with the nearest names, when the picture lacks it or it is mapped or set already.
        """
        self._native.map(self._name, address, signal, lag)
        return self

    def set(self, values: Mapping[str, object]) -> Clip:
        """Sets constants by address: numbers, bools, text (a colour such as `#ff00ff`, a choice) and float32 pictures.

        Nothing is set if any of them is refused.
        """
        self._native.set(self._name, dict(values))
        return self

    def swap(self, picture: Any) -> Clip:
        """Puts another picture in the clip. Refused, naming the address, if it lacks one that is set or mapped."""
        self._native.swap(self._name, picture)
        return self


class Piece:
    """A video being made. Builder calls change it and chain; `copy()` is cheap, so a variant leaves its source be."""

    def __init__(self, profile: Profile) -> None:
        """An empty piece that will be written with `profile`."""
        self._native = _core.Piece(profile)
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

    def render(self, path: str | os.PathLike[str], *, workers: int | None = None) -> None:
        """Renders the piece into the file `path` (mp4, mov, gif or png by its extension) with ffmpeg.

        Frames are drawn on `workers` threads (`DEFAULT_WORKERS` if `None`) and reach the encoder in order, so the file
        does not depend on the number. The file appears only when ffmpeg succeeded. ffmpeg is the program named by
        `BT_VIDEO_FFMPEG`, else `ffmpeg` on `PATH`.
        """
        self._native.render(os.fspath(path), DEFAULT_WORKERS if workers is None else workers)

    def copy(self) -> Piece:
        """An independent piece sharing the pictures, signals and song: nothing the copy changes touches this one."""
        copied = Piece(self.profile)
        copied._native = self._native.copy()
        return copied
