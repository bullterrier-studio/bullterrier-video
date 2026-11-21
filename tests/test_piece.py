"""A piece through the package's public interface, with fakes of everything outside it: a picture that draws its
`level`, a stand-in program for ffmpeg that keeps what it is fed, and a stand-in `bullterrier_audio`. No sibling
library is needed."""

from __future__ import annotations

import stat
import sys
import threading
import types
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any

import numpy as np
import pytest

import bullterrier_video as btv
from bullterrier_video import InvalidError, PortError

if TYPE_CHECKING:
    from pathlib import Path

WIDTH, HEIGHT = 4, 6
FRAME_BYTES = WIDTH * HEIGHT * 6  # 16-bit RGB


@dataclass(frozen=True)
class Described:
    """What a picture says of one parameter: the fields of scene's `Parameter` that video reads."""

    kind: str
    meaning: str = ""
    unit: str | None = None
    min: float = 0.0
    max: float = 1.0
    cyclic: bool = False
    choices: tuple[str, ...] = ()


@dataclass
class FakeScene:
    """A picture of one flat red whose level is `level`; it keeps every value it was given, as a scene's caller would
    see."""

    values: dict[str, Any] = field(default_factory=dict)
    seen: list[dict[str, Any]] = field(default_factory=list)
    shape: tuple[int, int] = (HEIGHT, WIDTH)

    def parameters(self) -> dict[str, Described]:
        return {
            "level": Described("float", unit="fraction"),
            "angle": Described("float", unit="degrees", max=360.0, cyclic=True),
            "count": Described("int", max=10.0),
            "title": Described("text"),
            "tint": Described("color"),
            "mode": Described("choice", choices=("normal", "screen")),
            "on": Described("bool"),
        }

    def with_values(self, values: dict[str, Any]) -> FakeScene:
        self.seen.append(dict(values))
        return FakeScene({**self.values, **values}, self.seen, self.shape)

    def render(self, size: int) -> np.ndarray:
        pixels = np.zeros((*self.shape, 4), dtype=np.float32)
        pixels[..., 0] = self.values.get("level", 0.0)
        pixels[..., 3] = 1.0
        return pixels


class Time:
    """A signal that is the time itself."""

    def sample(self, times: Any) -> np.ndarray:
        return np.asarray(times, dtype=np.float64)


class Constant:
    """A signal that is `value` at every time."""

    def __init__(self, value: float) -> None:
        self.value = value

    def sample(self, times: Any) -> np.ndarray:
        return np.full(len(times), self.value)


@pytest.fixture
def ffmpeg(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """A stand-in program for ffmpeg that writes its standard input to the last argument; `BT_VIDEO_FFMPEG` points at
    it."""
    script = tmp_path / "fake-ffmpeg"
    keep_sound = f'case "$a" in *.audio.part) cp "$a" "{tmp_path}/sound.wav";; esac'
    script.write_text(f'#!/bin/sh\nfor a; do last="$a"; {keep_sound}; done\ncat > "$last"\n', encoding="utf-8")
    script.chmod(script.stat().st_mode | stat.S_IEXEC)
    monkeypatch.setenv("BT_VIDEO_FFMPEG", str(script))
    return script


def first_values(path: Path) -> list[int]:
    """The red of the first pixel of every frame in what the stand-in kept."""
    data = path.read_bytes()
    return [int.from_bytes(data[at : at + 2], "little") for at in range(0, len(data), FRAME_BYTES)]


def piece_of(*, fps: float = 4.0, end: float = 1.0) -> tuple[btv.Piece, FakeScene]:
    scene = FakeScene()
    piece = btv.Piece(btv.Profile(WIDTH, HEIGHT, fps))
    piece.clip("a", scene, end=end)
    return piece, scene


def test_a_mapped_signal_becomes_the_frames_in_order(ffmpeg: Path, tmp_path: Path):
    piece, _ = piece_of()
    piece["a"].map("level", Time())
    out = tmp_path / "clip.mp4"
    piece.render(out)
    # one second at four a second: the level is the time (0, .25, .5, .75), times 65535 rounded, in 16-bit
    assert first_values(out) == [0, 16384, 32768, 49151]


def test_one_frame_is_a_float_array_of_the_profiles_shape():
    piece, _ = piece_of()
    piece["a"].map("level", Constant(0.5))
    frame = piece.frame(0.25)
    assert frame.shape == (HEIGHT, WIDTH, 4)
    assert frame.dtype == np.float32
    assert frame[0, 0, 0] == pytest.approx(0.5)


@pytest.mark.parametrize("workers", [1, 3])
def test_the_file_does_not_depend_on_the_number_of_workers(ffmpeg: Path, tmp_path: Path, workers: int):
    piece, _ = piece_of(fps=40.0)
    piece["a"].map("level", Time())
    serial, parallel = tmp_path / "serial.mp4", tmp_path / "parallel.mp4"
    piece.render(serial, workers=1)
    piece.render(parallel, workers=workers)
    assert serial.read_bytes() == parallel.read_bytes()


def test_values_reach_the_picture_in_its_own_terms():
    piece, scene = piece_of()
    clip = piece["a"]
    clip.map("angle", Constant(370.0)).map("count", Constant(3.6)).map("on", Constant(0.7))
    clip.set({"title": "PILOT", "tint": "#ff00ff", "mode": "screen"})
    piece.frame(0.0)
    seen = scene.seen[-1]
    assert seen["angle"] == pytest.approx(10.0), "a turn wraps"
    assert seen["count"] == 4
    assert isinstance(seen["count"], int), "a whole-number parameter is given as an int"
    assert seen["on"] is True
    assert (seen["title"], seen["tint"], seen["mode"]) == ("PILOT", "#ff00ff", "screen")


def test_a_value_is_held_to_the_described_range_and_a_lag_reads_late():
    piece, scene = piece_of()
    piece["a"].map("level", Time(), lag=0.1)
    piece.frame(0.0)
    assert scene.seen[-1]["level"] == 0.0, "t - lag is held at zero"
    piece.frame(0.75)
    assert scene.seen[-1]["level"] == pytest.approx(0.65)
    held, _ = piece_of()
    held["a"].map("level", Constant(7.0))
    held.frame(0.0)
    assert held["a"].name == "a"


def test_what_the_caller_gets_wrong_is_an_invalid_error_naming_it():
    piece, _ = piece_of()
    with pytest.raises(InvalidError, match=r"`levle`.*`level`"):
        piece["a"].map("levle", Time())
    with pytest.raises(InvalidError, match="title"):
        piece["a"].map("title", Time())
    with pytest.raises(InvalidError, match="no clip `zz`"):
        piece["zz"]
    with pytest.raises(InvalidError, match="already"):
        piece.clip("a", FakeScene())
    piece["a"].map("level", Time())
    with pytest.raises(InvalidError, match="already mapped"):
        piece["a"].map("level", Constant(1.0))
    with pytest.raises(InvalidError, match="one of normal, screen"):
        piece["a"].set({"mode": "multiply"})
    with pytest.raises(InvalidError, match="even"):
        btv.Profile(5, 6, 30)


def test_an_object_that_is_not_a_picture_is_a_port_error_naming_what_it_lacks():
    piece = btv.Piece(btv.Profile(WIDTH, HEIGHT, 4))
    with pytest.raises(PortError, match=r"picture: AttributeError.*parameters"):
        piece.clip("a", object(), end=1.0)


def test_a_picture_of_the_wrong_shape_is_refused_naming_both_sizes():
    piece = btv.Piece(btv.Profile(WIDTH, HEIGHT, 4))
    piece.clip("a", FakeScene(shape=(HEIGHT - 2, WIDTH)), end=1.0)
    with pytest.raises(PortError, match=r"4 by 4.*4 by 6"):
        piece.frame(0.0)


def test_copy_is_independent_of_its_source():
    piece, _ = piece_of()
    piece["a"].map("level", Constant(0.25))
    other = piece.copy()
    with pytest.raises(InvalidError, match="already mapped"):
        other["a"].map("level", Constant(0.5))  # the copy has what the source had
    other["a"].map("count", Constant(1.0))
    piece["a"].map("count", Constant(2.0))  # the copy's mapping did not reach the source
    piece.frame(0.0)
    other.frame(0.0)


def test_ffmpeg_that_is_missing_or_failing_is_a_port_error_in_its_own_words(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
):
    piece, _ = piece_of()
    piece["a"].map("level", Time())
    monkeypatch.setenv("BT_VIDEO_FFMPEG", str(tmp_path / "nowhere"))
    with pytest.raises(PortError, match="BT_VIDEO_FFMPEG"):
        piece.render(tmp_path / "clip.mp4")
    failing = tmp_path / "failing"
    failing.write_text("#!/bin/sh\ncat > /dev/null\necho 'Unknown encoder libx264' >&2\nexit 3\n", encoding="utf-8")
    failing.chmod(failing.stat().st_mode | stat.S_IEXEC)
    monkeypatch.setenv("BT_VIDEO_FFMPEG", str(failing))
    with pytest.raises(PortError, match="Unknown encoder libx264"):
        piece.render(tmp_path / "clip.mp4")
    assert not (tmp_path / "clip.mp4").exists()


def test_a_profile_says_its_shape_and_the_presets_have_theirs():
    vertical = btv.Profile.vertical(720, 30, depth=10, loudness=-14)
    assert (vertical.width, vertical.height, vertical.fps, vertical.depth, vertical.loudness) == (
        720,
        1280,
        30,
        10,
        -14,
    )
    assert vertical.safe == (0.12, 0.80)
    assert (btv.Profile.landscape(1920, 24).height, btv.Profile.square(1080, 30).height) == (1080, 1080)
    with pytest.raises(InvalidError, match="depth"):
        btv.Profile.vertical(720, 30, depth=12)


# ---- the song: a stand-in for bullterrier_audio -------------------------------------------------------------------


@dataclass
class Event:
    t: float
    strength: float


class FakeSound:
    """A 20 s stereo song: beats every half second, a drop at 14, kicks, two sections, rising bass; it keeps what was
    done to it."""

    def __init__(self, start: float = 0.0, end: float = 20.0, ops: tuple[Any, ...] = ()) -> None:
        self.start, self.end, self.ops = start, end, ops

    rate, channels = 4, 2

    @property
    def duration(self) -> float:
        return self.end - self.start

    @property
    def samples(self) -> np.ndarray:
        return np.stack([np.arange(8, dtype=np.float32), -np.arange(8, dtype=np.float32)])  # one row a channel

    def grid(self) -> Any:
        return types.SimpleNamespace(beats=np.arange(0.0, 20.0, 0.5), downbeats=np.arange(0.0, 20.0, 2.0))

    def drops(self) -> list[Event]:
        return [Event(5.0, 0.4), Event(14.0, 0.9)]

    def hits(self, hit: str) -> list[Event]:
        assert hit == "KICK"
        return [Event(t, 1.0) for t in (9.0, 10.5, 12.0, 21.0)]

    def sections(self) -> list[Any]:
        return [
            types.SimpleNamespace(start=0.0, end=8.0, label="a"),
            types.SimpleNamespace(start=8.0, end=20.0, label="b"),
        ]

    def cut(self, start: float, end: float) -> FakeSound:
        return FakeSound(start, end, (*self.ops, ("cut", start, end)))

    def fade(self, in_: float, out: float) -> FakeSound:
        return FakeSound(self.start, self.end, (*self.ops, ("fade", in_, out)))

    def normalised(self, lufs: float) -> FakeSound:
        return FakeSound(self.start, self.end, (*self.ops, ("lufs", lufs)))

    def energy(self, band: str) -> Any:
        assert band == "BASS"
        values = np.arange(self.start, self.end, 1.0 / 10)  # rises with time, ten a second
        scaled = (values - values.min()) / (values.max() - values.min())
        return types.SimpleNamespace(percentile_scaled=lambda: types.SimpleNamespace(rate=10.0, values=scaled))


@pytest.fixture
def audio(monkeypatch: pytest.MonkeyPatch) -> FakeSound:
    """`bullterrier_audio` as the adapter finds it: its `Hit` and `Band` names, and a song to hand over."""
    library = types.ModuleType("bullterrier_audio")
    library.Hit = types.SimpleNamespace(KICK="KICK", SNARE="SNARE", HAT="HAT")  # type: ignore[attr-defined]
    library.Band = types.SimpleNamespace(BASS="BASS")  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "bullterrier_audio", library)
    return FakeSound()


def test_the_song_is_cut_to_its_span_and_everything_it_knows_is_in_piece_seconds(audio: FakeSound):
    piece, _ = piece_of(end=10.0)
    music = piece.song(audio, btv.around_drop(before=4.0, length=10.0), fade_out=0.5)
    assert piece.duration == 10.0
    assert music.drop == 4.0, "the strongest drop is at 14, in a span that starts at 10"
    assert music.kicks == [0.5, 2.0], "kicks at 10.5 and 12; 9 and 21 are outside"
    assert music.beats[:3] == [0.0, 0.5, 1.0]
    assert music.tempo == pytest.approx(120.0)
    assert [tuple(s) for s in music.sections] == [(0.0, 10.0, "b")], "section a ends before the span"
    assert music.duration == 10.0


def test_levels_are_plain_data_scaled_over_the_span_and_over_a_stretch(audio: FakeSound):
    piece, _ = piece_of(end=10.0)
    music = piece.song(audio, btv.between(10.0, 20.0))
    levels = music.band("bass")
    assert (levels.rate, levels.start, len(levels.values)) == (10.0, 0.0, 100)
    assert (levels.values[0], levels.values[-1]) == (0.0, 1.0), "the span's own range"
    tail = music.band("bass", over=(5.0, None))
    assert (tail.start, len(tail.values)) == (5.0, 50)
    assert isinstance(levels.values, np.ndarray)


def test_a_song_without_a_drop_in_the_piece_and_a_second_song_are_refused(audio: FakeSound):
    piece, _ = piece_of(end=10.0)
    music = piece.song(audio, btv.between(0.0, 4.0))
    with pytest.raises(InvalidError, match="no drop"):
        music.drop  # noqa: B018  (the drops at 5 and 14 are outside 0 to 4)
    with pytest.raises(InvalidError, match="song already"):
        piece.song(audio, btv.whole())
    with pytest.raises(InvalidError, match="past the song's end"):
        btv.Piece(piece.profile).song(audio, btv.around_drop(before=4.0, length=30.0))


def test_the_song_reaches_the_encoder_cut_and_interleaved(audio: FakeSound, ffmpeg: Path, tmp_path: Path):
    piece, _ = piece_of(end=2.0)
    piece.song(audio, btv.between(0.0, 2.0), fade_in=0.25, fade_out=0.5)
    piece["a"].map("level", Time())
    piece.render(tmp_path / "clip.mp4")
    assert (tmp_path / "clip.mp4").stat().st_size == 8 * FRAME_BYTES, "two seconds at four a second"
    wav = (tmp_path / "sound.wav").read_bytes()
    assert (wav[8:16], int.from_bytes(wav[22:24], "little"), int.from_bytes(wav[24:28], "little")) == (
        b"WAVEfmt ",
        2,
        4,
    )
    samples = np.frombuffer(wav[56:], dtype="<f4")
    # the fake gives one row a channel, [0..7] and [-0..-7]; the encoder wants them interleaved by frame
    assert samples.tolist() == [0.0, -0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0, 5.0, -5.0, 6.0, -6.0, 7.0, -7.0]


def test_a_missing_audio_library_is_a_port_error_that_says_how_to_get_it(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setitem(sys.modules, "bullterrier_audio", None)
    piece, _ = piece_of()
    with pytest.raises(PortError, match="install bullterrier-audio"):
        piece.song(object(), btv.whole())


def test_a_slow_picture_in_one_thread_does_not_lock_another_threads_piece_calls():
    """A piece call waiting for another thread's call must not hold the GIL while it waits: that thread may need it."""
    entered, release = threading.Event(), threading.Event()

    class Slow(FakeScene):
        def parameters(self) -> dict[str, Described]:
            entered.set()
            release.wait()  # a picture that takes its time, with the GIL free, as a scene's render does
            return super().parameters()

    piece = btv.Piece(btv.Profile(WIDTH, HEIGHT, 4))
    adding = threading.Thread(target=lambda: piece.clip("slow", Slow(), end=1.0))
    adding.start()
    entered.wait()
    copying = threading.Thread(target=piece.copy)  # needs what the first thread holds
    copying.start()
    release.set()  # only runs if the copying thread is not holding the GIL while it waits
    adding.join()
    copying.join()
    assert piece["slow"].name == "slow"


def test_a_sheet_is_the_chosen_moments_side_by_side(ffmpeg: Path, tmp_path: Path):
    piece, _ = piece_of()
    piece["a"].map("level", Time())
    out = tmp_path / "sheet.png"
    piece.sheet(out, [0.0, 0.5], width=6)
    data = out.read_bytes()
    cell_width, gap = WIDTH, 6  # the fake scene draws 4 by 6 whatever size it is asked for; the gap is the library's
    row = (2 * cell_width + gap) * 6  # bytes in a row of the sheet
    assert len(data) == row * HEIGHT
    pixel = lambda column: int.from_bytes(data[column * 6 : column * 6 + 2], "little")  # noqa: E731
    assert pixel(0) == 0, "the level at 0 s"
    assert pixel(cell_width) == round(0.12 * 65535), "the grey between the cells"
    assert pixel(cell_width + gap) == 32768, "the level at 0.5 s"


def test_a_sheet_that_cannot_be_made_is_an_invalid_error(tmp_path: Path):
    piece, _ = piece_of()
    with pytest.raises(InvalidError, match="at least one moment"):
        piece.sheet(tmp_path / "s.png", [])
    with pytest.raises(InvalidError, match="not inside the piece"):
        piece.sheet(tmp_path / "s.png", [0.5, 2.0])


def test_render_all_makes_one_file_for_each_name_and_names_the_piece_that_fails(ffmpeg: Path, tmp_path: Path):
    good, _ = piece_of()
    good["a"].map("level", Time())
    other = good.copy()
    made = btv.render_all({"one": good, "two": other}, tmp_path / "out" / "deep", workers=1)
    assert sorted(made) == ["one", "two"]
    assert [p.name for p in made.values()] == ["one.mp4", "two.mp4"]
    assert made["one"].read_bytes() == made["two"].read_bytes() != b""
    broken = btv.Piece(btv.Profile(WIDTH, HEIGHT, 4))  # nothing gives it a length
    with pytest.raises(InvalidError, match="piece `broken`"):
        btv.render_all({"first": good, "broken": broken}, tmp_path / "again")
    assert (tmp_path / "again" / "first.mp4").exists(), "what was made before the failure is kept"
    with pytest.raises(InvalidError, match="file name"):
        btv.render_all({"a/b": good}, tmp_path / "x")


def test_a_render_says_how_many_frames_it_drew_and_a_held_frame_is_not_drawn_again(ffmpeg: Path, tmp_path: Path):
    held, _ = piece_of()
    held["a"].set({"level": 0.5})
    assert held.render(tmp_path / "held.mp4") == btv.RenderStats(frames=4, drawn=1)
    assert first_values(tmp_path / "held.mp4") == [32768] * 4, "every frame is there, drawn once"
    moving, _ = piece_of()
    moving["a"].map("level", Time())
    assert moving.render(tmp_path / "moving.mp4") == btv.RenderStats(frames=4, drawn=4)


class Scaling(FakeScene):
    """A picture that draws its 2:3 shape at whatever size it is asked for, as a scene does."""

    def render(self, size: int) -> np.ndarray:
        return np.zeros((size, size * 2 // 3, 4), dtype=np.float32)

    def with_values(self, values: dict[str, Any]) -> Scaling:
        return Scaling()


def test_a_draft_is_the_same_piece_smaller_and_slower_and_the_original_is_unchanged(ffmpeg: Path, tmp_path: Path):
    piece = btv.Piece(btv.Profile(8, 12, 4))
    piece.clip("a", Scaling(), end=1.0)
    draft = piece.draft(0.5, fps=2)
    assert (draft.profile.width, draft.profile.height, draft.profile.fps) == (4, 6, 2)
    assert (piece.profile.width, piece.profile.height, piece.profile.fps) == (8, 12, 4), "the original is as it was"
    assert draft.frame(0.0).shape == (6, 4, 4)
    assert piece.frame(0.0).shape == (12, 8, 4)
    assert draft.render(tmp_path / "draft.mp4").frames == 2, "one second at two a second"
    assert piece.draft().profile.width == 4, "half size is the default"
    with pytest.raises(InvalidError, match="positive"):
        piece.draft(0)


@dataclass(frozen=True)
class Handle:
    """Anything with an `.address`: the only thing video asks of a typed parameter."""

    address: str


def test_a_handle_stands_in_for_an_address_in_map_and_set():
    piece, scene = piece_of()
    piece["a"].map(Handle("level"), Constant(0.5)).set({Handle("title"): "PILOT", "mode": "screen"})
    piece.frame(0.0)
    seen = scene.seen[-1]
    assert (seen["level"], seen["title"], seen["mode"]) == (0.5, "PILOT", "screen")


def test_an_address_mapped_by_string_is_refused_again_by_handle_naming_the_string():
    piece, _ = piece_of()
    piece["a"].map("level", Time())
    with pytest.raises(InvalidError, match=r"`level`.*already mapped"):
        piece["a"].map(Handle("level"), Constant(1.0))


def test_a_handle_to_a_missing_address_gets_the_nearest_names_and_set_stays_all_or_nothing():
    piece, scene = piece_of()
    with pytest.raises(InvalidError, match=r"`levle`.*`level`"):
        piece["a"].map(Handle("levle"), Time())
    with pytest.raises(InvalidError, match="levle"):
        piece["a"].set({Handle("title"): "X", Handle("levle"): 1.0})
    piece.frame(0.0)
    assert "title" not in scene.seen[-1]


def test_a_key_that_is_neither_a_string_nor_a_handle_is_a_type_error():
    piece, _ = piece_of()
    with pytest.raises(TypeError, match="address"):
        piece["a"].map(3, Time())  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="address"):
        piece["a"].set({None: 1.0})  # type: ignore[dict-item]
