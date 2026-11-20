"""The audio adapter: a `bullterrier_audio.Sound` as the music and the sound video's core reads.

The core never names the library: it asks these objects for plain lists and arrays (`beats()`, `hits("kick")`,
`band("bass", a, b)`), so a changed or replaced audio library changes this file only. `bullterrier_audio` is imported
when an adapter is made."""

from __future__ import annotations

import importlib
from typing import TYPE_CHECKING, Any

import numpy as np

from bullterrier_video.errors import PortError

if TYPE_CHECKING:
    from numpy.typing import NDArray

__all__ = ["AudioMusic", "AudioSound"]


def _library() -> Any:
    """The audio library, or a `PortError` that says how to get it."""
    try:
        return importlib.import_module(
            "bullterrier_audio"
        )  # when a song is used: video needs no audio library to import
    except ImportError as failure:
        message = "music: bullterrier_audio is not installed; install bullterrier-audio to put a song under a piece"
        raise PortError(message) from failure


class AudioMusic:
    """What a sound knows about its music, as plain data in seconds from its start."""

    def __init__(self, sound: Any) -> None:
        """Wraps `sound`, a `bullterrier_audio.Sound`."""
        self._sound = sound
        self._audio = _library()

    def duration(self) -> float:
        """How long the sound lasts."""
        return float(self._sound.duration)

    def beats(self) -> list[float]:
        """Every beat."""
        return [float(t) for t in self._sound.grid().beats]

    def downbeats(self) -> list[float]:
        """The first beat of each bar; an error when the meter is not known, never an empty list."""
        found = [float(t) for t in self._sound.grid().downbeats]
        if not found:
            message = "the song has no downbeats: its meter is not known"
            raise ValueError(message)
        return found

    def drops(self) -> list[tuple[float, float]]:
        """The drops as `(time, strength)`, strongest first."""
        found = [(float(e.t), float(e.strength)) for e in self._sound.drops()]
        return sorted(found, key=lambda drop: -drop[1])

    def hits(self, name: str) -> list[tuple[float, float]]:
        """The hits of one kind (`kick`, `snare`, `hat`) as `(time, strength)`."""
        return [(float(e.t), float(e.strength)) for e in self._sound.hits(getattr(self._audio.Hit, name.upper()))]

    def sections(self) -> list[tuple[float, float, str]]:
        """The sections as `(start, end, label)`."""
        return [(float(s.start), float(s.end), str(s.label)) for s in self._sound.sections()]

    def band(self, name: str, start: float, end: float) -> tuple[float, NDArray[np.float64]]:
        """The level of a band between `start` and `end`, scaled by that stretch's own range: as `(rate, values)`."""
        series = self._sound.cut(start, end).energy(getattr(self._audio.Band, name.upper())).percentile_scaled()
        return float(series.rate), np.asarray(series.values, dtype=np.float64)


class AudioSound:
    """One sound, edited one operation at a time."""

    def __init__(self, sound: Any) -> None:
        """Wraps `sound`, a `bullterrier_audio.Sound`."""
        self._sound = sound

    def rate(self) -> int:
        """Samples a second."""
        return int(self._sound.rate)

    def channels(self) -> int:
        """Channels."""
        return int(self._sound.channels)

    def duration(self) -> float:
        """How long it lasts."""
        return float(self._sound.duration)

    def cut(self, start: float, end: float) -> AudioSound:
        """The part from `start` to `end`."""
        return AudioSound(self._sound.cut(start, end))

    def fade(self, fade_in: float, fade_out: float) -> AudioSound:
        """The sound with a fade in and out."""
        return AudioSound(self._sound.fade(fade_in, fade_out))

    def normalised(self, lufs: float) -> AudioSound:
        """The sound at `lufs` integrated loudness."""
        return AudioSound(self._sound.normalised(lufs))

    def samples(self) -> NDArray[np.float32]:
        """The samples interleaved by channel, as one float32 array (the library gives one row a channel)."""
        return np.ascontiguousarray(np.asarray(self._sound.samples, dtype=np.float32).T).reshape(-1)
