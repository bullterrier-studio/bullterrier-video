"""bullterrier-video: a placeholder Rust-first project with a Python interface (see README.md and CLAUDE.md)."""

from importlib import metadata

from ._native._core import scale
from .errors import BullterrierVideoError, InvalidError

__version__ = metadata.version("bullterrier-video")

__all__ = ["BullterrierVideoError", "InvalidError", "__version__", "scale"]
