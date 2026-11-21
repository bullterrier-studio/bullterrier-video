"""`Clip.set` takes a typed caller's dict of strings, or of handles, but not of anything else: proven with mypy."""

import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
pytestmark = pytest.mark.typecheck

HEADER = """from dataclasses import dataclass

from bullterrier_video import Clip


@dataclass(frozen=True)
class Handle:
    address: str


def probe(clip: Clip) -> None:
"""

CASES = {
    "a dict[str, float] variable": ("v: dict[str, float] = {}\n    clip.set(v)", True),
    "a dict[handle, float] variable": ("v: dict[Handle, float] = {}\n    clip.set(v)", True),
    "a literal of strings": ('clip.set({"a.b": 1.0, "c": "text"})', True),
    "a literal of handles": ('clip.set({Handle("a.b"): 1.0})', True),
    "a mixed literal, annotated": (
        'v: dict[str | Handle, float] = {"a.b": 1.0, Handle("c"): 2.0}\n    clip.set(v)',
        True,
    ),
    # joins to `object`: refused, the caller annotates it as above
    "a mixed literal, bare": ('clip.set({"a.b": 1.0, Handle("c"): 2.0})', False),
    "a dict[int, float] variable": ("v: dict[int, float] = {}\n    clip.set(v)", False),
}


@pytest.mark.parametrize(("body", "accepted"), CASES.values(), ids=CASES.keys())
def test_set_accepts_exactly_the_keyed_dicts_it_documents(tmp_path, body, accepted):
    probe = tmp_path / "probe_set_keys.py"
    probe.write_text(HEADER + "    " + body + "\n", encoding="utf-8")
    command = [sys.executable, "-m", "mypy", "--no-incremental", str(probe)]
    result = subprocess.run(command, cwd=ROOT, capture_output=True, encoding="utf-8")
    assert (result.returncode == 0) == accepted, result.stdout + result.stderr
