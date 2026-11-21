"""bullterrier-video: decides what happens when and what drives what in a video, then renders it (see README.md and
CLAUDE.md)."""

from importlib import metadata

from ._native._core import Profile, scale
from .errors import BullterrierVideoError, InvalidError, PortError
from .music import Bar, Levels, Music, Phrase, Section
from .piece import Addressed, Clip, Piece, RenderStats, render_all
from .spans import Span, around_drop, bars, between, loudest, whole

__version__ = metadata.version("bullterrier-video")

__all__ = [
    "Addressed",
    "Bar",
    "BullterrierVideoError",
    "Clip",
    "InvalidError",
    "Levels",
    "Music",
    "Phrase",
    "Piece",
    "PortError",
    "Profile",
    "RenderStats",
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
