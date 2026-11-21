"""What the song under a piece knows, in the piece's own seconds."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, NamedTuple

from .errors import InvalidError

if TYPE_CHECKING:
    import numpy as np
    from numpy.typing import NDArray

    from ._native._core import Music as _Music

__all__ = ["Bar", "Levels", "Music", "Phrase", "Section"]


class Section(NamedTuple):
    """A named stretch of the song."""

    start: float
    end: float
    label: str


class Bar(NamedTuple):
    """One bar: from its downbeat to the next, the last to the end of the song."""

    start: float
    end: float


class Phrase(NamedTuple):
    """A run of whole bars; the last phrase of a song may hold fewer."""

    start: float
    end: float


@dataclass(frozen=True)
class Levels:
    """The level of one band: `values` between 0 and 1 at `rate` samples a second, the first at `start` seconds.

    It is plain data: make a signal with control, `btc.from_values(levels.values, levels.rate, start=levels.start)`.
    """

    values: NDArray[np.float64]
    rate: float
    start: float


class Music:
    """The song under a piece. Every time is in the piece's seconds: the start of its span is zero.

    A measure the song cannot give raises `InvalidError` (no drop, an unknown meter) or `PortError` (no drum part
    found), never an empty list that looks like silence."""

    def __init__(self, native: _Music) -> None:
        """Wraps the core's view of the song; made by `Piece.song`."""
        self._native = native

    @property
    def duration(self) -> float:
        """How long the piece's soundtrack lasts."""
        return self._native.duration

    @property
    def tempo(self) -> float:
        """Beats a minute: sixty over the median gap between beats."""
        return self._native.tempo()

    @property
    def beats(self) -> list[float]:
        """Every beat."""
        return self._native.beats()

    @property
    def downbeats(self) -> list[float]:
        """The first beat of each bar."""
        return self._native.downbeats()

    @property
    def bars(self) -> list[Bar]:
        """Each bar from its downbeat to the next; the last ends where the song does."""
        starts = self.downbeats
        return [Bar(a, b) for a, b in zip(starts, [*starts[1:], self.duration], strict=True)]

    def phrases(self, bars: int = 4) -> list[Phrase]:
        """The bars in runs of `bars` (four by default); the last run holds what is left. Refused under one bar."""
        if bars < 1:
            message = f"a phrase is at least 1 bar, not {bars}"
            raise InvalidError(message)
        every = self.bars
        return [Phrase(every[i].start, every[min(i + bars, len(every)) - 1].end) for i in range(0, len(every), bars)]

    @property
    def drop(self) -> float:
        """The strongest drop in the piece."""
        return self._native.drop()

    @property
    def kicks(self) -> list[float]:
        """The kick drums."""
        return self._native.hits("kick")

    @property
    def snares(self) -> list[float]:
        """The snares."""
        return self._native.hits("snare")

    @property
    def hats(self) -> list[float]:
        """The hi-hats."""
        return self._native.hits("hat")

    @property
    def sections(self) -> list[Section]:
        """The sections that overlap the piece, cut to it."""
        return [Section(*s) for s in self._native.sections()]

    def band(self, name: str, *, over: tuple[float, float | None] | None = None) -> Levels:
        """The level of `name` (`sub`, `bass`, `low_mid`, `mid`, `high`, `air`) over the whole piece or over `over`.

        `over=(start, end)` is a stretch of the piece (`end` of `None`: its end). The levels are scaled to 0 to 1 by
        **that stretch's own range**.
        """
        values, rate, start = self._native.band(name, over)
        return Levels(values, rate, start)
