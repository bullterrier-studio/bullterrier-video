"""How one clip gives way to the next: the `kind` of `Piece.transition`."""

from __future__ import annotations

from dataclasses import dataclass

from .errors import InvalidError

__all__ = ["Cut", "Dissolve", "Sweep"]


@dataclass(frozen=True)
class Cut:
    """The first clip until the middle of the transition, the second after it. Nothing is blended."""


@dataclass(frozen=True)
class Dissolve:
    """The second clip fades in over the first."""


@dataclass(frozen=True)
class Sweep:
    """A soft edge crosses the frame and shows the second clip behind it.

    `kind` is the bar that rides the edge: `"light"` (white, screen blend) or `"dark"` (black). `angle` is the direction
    of travel in degrees clockwise (0: left to right, 90: downwards). `feather` is the edge's width as a fraction of the
    frame's long side.
    """

    kind: str = "light"
    angle: float = 0.0
    feather: float = 0.15

    def __post_init__(self) -> None:
        """Refuses a kind that is not `"light"` or `"dark"`."""
        if self.kind not in {"light", "dark"}:
            message = f'a sweep is "light" or "dark", got {self.kind!r}'
            raise InvalidError(message)
