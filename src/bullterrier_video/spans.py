"""Which part of a song becomes the piece."""

from __future__ import annotations

from ._native._core import Span

__all__ = ["Span", "around_drop", "bars", "between", "loudest", "whole"]


def around_drop(before: float, length: float) -> Span:
    """`length` seconds starting `before` seconds ahead of the song's strongest drop (it starts at the song's start if
    the drop is nearer)."""
    return Span.around_drop(before, length)


def bars(n: int) -> Span:
    """`n` whole bars from the song's first downbeat, so a loop closes on the beat."""
    return Span.bars(n)


def whole() -> Span:
    """The whole song."""
    return Span.whole()


def between(start: float, end: float) -> Span:
    """From `start` to `end` seconds."""
    return Span.between(start, end)


def loudest(band: str, length: float) -> Span:
    """The `length` seconds in which `band` (`sub`, `bass`, `low_mid`, `mid`, `high`, `air`) is strongest."""
    return Span.loudest(band, length)
