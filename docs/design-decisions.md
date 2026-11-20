# Design decisions

This project's own decisions and their reasons, one section each: what was chosen, why, and what it costs. The
template's are in [template-decisions.md](template-decisions.md). This file is the project's: an update never changes it.

## Ports and adapters; Python objects stay Python (2026-11-20)

Every outside library (a scene, a signal, music, a sound, an encoder) is met through a trait of video's own, `bullterrier_video::ports`,
with its own small types (`Frame`, `Parameter`, `Kind`, `Value`, `View`), and one adapter per library. *Reason:* the sibling
libraries will break their APIs and one may be replaced; only an adapter then changes. A port exists only where an outside library
sits, and video does not mirror a sibling's API (that would be a pass-through layer and two APIs to keep).

**A Python caller's objects are not serialised.** A scene, a control signal and an audio sound built in Python live in the sibling's
own extension module, so video's cannot take them as Rust values. Options weighed: (1) *duck-typed ports (chosen):* the adapter
wraps any Python object that has the port's methods; (2) *data across the boundary* (scene JSON, a control patch, an audio bundle):
not lossless today (scene's `to_json` refuses a picture held as an array and a plugin layer, a patch refuses a callback, audio's
bundle leaves out drum hits); (3) *video links the sibling crates and re-exposes them:* a mirror of three Python APIs.
*Cost of (1):* Python calls in the frame loop for Python callers; scene's `render` releases the GIL, so frames still draw in
parallel threads, and a sibling that does not release it would need processes. A pure-Rust caller gets a Rust adapter, so the core has no Python.

**The core crate depends on no sibling.** Adapters for Rust callers are separate crates, so a sibling's pin never constrains the core.
The picture port is one long-lived instance rendered with new values each frame: scene's cache is worth 0.19 s a frame against 0.95 s
without, and a port that rebuilt per frame would throw it away.
