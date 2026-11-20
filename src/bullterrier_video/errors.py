"""The exceptions bullterrier-video raises: catch `BullterrierVideoError` for every failure it reports on purpose.

Library code raises a subclass of it, never a bare builtin, for anything a caller or an input file can get wrong; the
message says what was wrong and what is valid. A subclass also inherits the builtin it replaces (`ValueError`, ...), so
code that already catches that builtin keeps working.

Dependency direction: the bottom layer. It imports nothing from bullterrier_video, so every layer may raise them.
"""

__all__ = ["BullterrierVideoError", "InvalidError"]


class BullterrierVideoError(Exception):
    """The base of every exception bullterrier-video raises on purpose."""


class InvalidError(BullterrierVideoError, ValueError):
    """A value breaks a rule: the message names the argument and what is valid."""
