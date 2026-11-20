"""bullterrier-video: decides what happens when and what drives what in a video, then renders it (see README.md and
CLAUDE.md)."""

from importlib import metadata

from ._native._core import Profile, scale
from .errors import BullterrierVideoError, InvalidError, PortError
from .music import Levels, Music, Section
from .piece import Clip, Piece, render_all
from .spans import Span, around_drop, bars, between, loudest, whole

__version__ = metadata.version("bullterrier-video")

__all__ = [
    "BullterrierVideoError",
    "Clip",
    "InvalidError",
    "Levels",
    "Music",
    "Piece",
    "PortError",
    "Profile",
    "Section",
    "Span",
    "__version__",
    "around_drop",
    "bars",
    "between",
    "loudest",
    "render_all",
    "scale",
    "whole",
]
