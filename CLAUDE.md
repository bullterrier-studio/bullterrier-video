# CLAUDE.md: bullterrier-video

A Rust library (crate `bullterrier-video`) with a Python interface that decides what happens when and what drives what in a
video, then renders it: a piece over time, signals mapped onto the values of a picture, the soundtrack, frames, encoding. **The code
API is the product.** `README.md` (use), `docs/design-decisions.md` and `docs/template-decisions.md` (why), `docs/shared-files.md` (what
stays identical across projects), `docs/releasing.md` (CI and releases).

**Write docs terse.** This file, the README, `docs/`, docstrings, reports and briefs: state the rule or fact, drop preamble,
repetition and justification a reader does not need to act. Reasons go in `docs/design-decisions.md`, once. Committed files are
public: no absolute paths, no references to folders outside the repository.

## Start here

```bash
uv sync && uv run poe setup          # once per clone: environment (builds the Rust extension: needs rustup), git hooks, commit template, cargo tools
uv run pytest tests/test_scale.py -n 0   # the tests near your change, while you work
cargo test -p bullterrier-video --test it scale::  # one Rust area (inline: cargo test -p bullterrier-video --lib scaling::)
uv run poe fast                      # the inner loop, as often as you like: format, lint, types, quick tests
uv run poe check                     # the gate, before you report or merge (then `uv run lint-imports`)
uv run poe full                      # `check` and the gate proofs: before a tag (`poe release` runs it) and in CI
```

| To ... | Look at |
|---|---|
| add logic | a `pub` function in `crates/core/src/<concept>.rs`, re-exported in `lib.rs`; `scaling.rs` is the model |
| expose it to Python | a `#[pyfunction]` in `crates/python/src/lib.rs` exported from `mod _core`; its stub line in `src/bullterrier_video/_native/_core.pyi`; the name in `src/bullterrier_video/__init__.py` |
| add a command | `src/bullterrier_video/cli.py` |
| add an error | the core's `Error` (`error.rs`), mapped in `crates/python/src/error.rs`, a class in `errors.py` |
| test it | "Testing" and "Rust tests" in `docs/rules.md`; `crates/core/tests/it/` (Rust), `tests/` (Python) |
| add a macros crate | "Adding a macros crate" in `docs/rules.md` |
| change a gate | `pyproject.toml` (Python tools, poe tasks), `Cargo.toml` (Rust lints, tool pins); a proof in `tests/tools/` (the template's shared gates all have one; for your own it is recommended) |
| keep files identical across projects | `docs/shared-files.md` |

**State:** the API is designed (`docs/api.md`, `docs/examples/`) and the first piece runs: the mill (profile, spans, a song in piece time,
clips with their mappings, the piece and its parallel frame loop, the camera and transitions through the Compose port, the ffmpeg
adapter in the core; `Piece`, `Clip`, `Music`, `Cut`, `Dissolve`, `Sweep`, the audio and scene compose adapters and the bindings in
Python). Built surface and what is still planned (tracks, footage, loops): `docs/api.md`. The tests that need `bullterrier_scene` skip
without it: run them with it importable. The template's `scale(value, factor)` is still a placeholder in the core, the extension, the package and the command
`bt-video scale`: removing it means rewriting the gate proofs that edit its line (`tests/tools/test_rust_gates.py`). No sibling
package is a dependency: the core names none, an adapter does.

## Responsibility

`bullterrier-video` is the **video package** of the Bullterrier ecosystem: the one package with time and frames. It meets each
library it uses through a narrow contract of its own (a port) with one adapter per library, so a library that changes or is
replaced touches only its adapter.

| Responsible for | Not responsible for |
|---|---|
| The piece over time: timeline, clips, transitions, camera, musical time (beats, bars, drops) | Composing one picture from layers → scene |
| Mappings: a signal drives a value within its described range | Producing and shaping signals → control |
| The soundtrack: its span, mixing, sync; frames, decoding, encoding, output profiles | Measuring and editing one sound → audio |
| Effects over several frames; footage analysis over time | Single-picture effects and finders → image; shapes and type → vector |

## Names

`bullterrier-video` (PyPI package, GitHub repository, Rust crate; `bullterrier-video-python` is the bindings crate), `bullterrier_video` (import; alias `btv` in
docs), `bt-video` (command, config file `bt-video.toml`, cache folder), `BT_VIDEO_*` (env vars), `BullterrierVideoError` (base exception).
Identity is spelled in full, what a person types with the short prefix; a hyphen for what lives in a shell or a file system,
an underscore only where Python, Rust or an environment variable needs it. Names inside the package carry no prefix.

## Architecture

Layers, imports flow **downward only** (import-linter: contracts in `pyproject.toml`, run by `tests/test_layering.py` and
`uv run lint-imports`). A forbidden import means the code is in the wrong layer: move it.

```
app        __main__, cli
_native    home of the Rust extension `_native._core`   (imports `errors` only, lazily, to raise its errors)
errors     BullterrierVideoError > InvalidError
```

**Rust core:** `crates/core` (pure Rust: no PyO3, no numpy, `#![forbid(unsafe_code)]`; one
file per concept; `error.rs` the one error type) and `crates/python` (PyO3 bindings building `bullterrier_video._native._core`; generic: no
logic of the core, only the mapping of calls and errors). A refusal of the core is a Python `InvalidError`, imported when the first
one is raised. There is no other boundary. Python code never reimplements the core; a core function is exposed as it is.

## Rules

@docs/rules.md

The shared rules above come from the template (`docs/rules.md`, updated by `copier update`). This project's own rules go
here, below them; this file is the project's and an update never changes it.
