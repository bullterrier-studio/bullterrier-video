"""A colour that follows a signal: the value `Clip.map` takes for a colour address."""

from __future__ import annotations

from typing import Any

from .errors import InvalidError

__all__ = ["Ramp"]


class Ramp:
    """Two or more colour stops, evenly spaced, and the signal that picks the position between them.

    `Ramp("#3060ff", "#ff2020", signal)`: the signal's value 0 is the first stop, 1 the last (held to that range),
    and between stops the colours are mixed in OKLab, so a blend stays clean. A stop is `#rrggbb`, `#rgb` or three
    numbers from 0 to 1 (a tuple, list or 1-d array). The signal is anything `Clip.map` takes for a number. Stops
    and signal are checked when the ramp is mapped, with the address named.
    """

    def __init__(self, *stops_then_signal: Any) -> None:
        """A ramp through the stops (all but the last argument), driven by the last argument."""
        if len(stops_then_signal) < 3:
            message = "a ramp is Ramp(stop, stop, ..., signal): two or more colours, then the signal"
            raise InvalidError(message)
        *stops, signal = stops_then_signal
        self.stops: tuple[Any, ...] = tuple(stops)
        self.signal = signal
