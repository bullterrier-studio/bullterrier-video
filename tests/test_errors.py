"""The exceptions: one base to catch, each also the builtin it replaces."""

import pytest

import bullterrier_video
from bullterrier_video import BullterrierVideoError, InvalidError


def test_the_base_error_is_an_exception_a_caller_can_catch():
    with pytest.raises(BullterrierVideoError, match="no luck"):
        raise BullterrierVideoError("no luck")


def test_the_errors_are_exported_from_the_package():
    exported = (bullterrier_video.BullterrierVideoError, bullterrier_video.InvalidError)
    assert exported == (BullterrierVideoError, InvalidError)


def test_an_invalid_error_is_a_library_error_and_a_value_error():
    error = InvalidError("factor: must be finite")
    assert isinstance(error, BullterrierVideoError)
    assert isinstance(error, ValueError)
