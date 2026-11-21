"""Stills cut one into the next with sweeping transitions, under a slow camera push.

A transition draws both clips for its window, then the frame is put together by the compose adapter (scene). The camera moves that finished frame, so no clip is drawn again and scene's cache holds. Footage and a second
track for a title come with the decode port; the rest of the piece is built.
"""

import bullterrier_scene as bts
import bullterrier_video as btv
import numpy as np

SWEEP = 0.25  # seconds
piece = btv.Piece(btv.Profile.vertical(width=540, fps=24))
cuts = [0.0, 1.0, 2.0, 3.0]  # three stills of a second each, from the start to the end: no gap
colours = ["#c0392b", "#2c3e50", "#16a085"]
# the clips only touch: a transition draws both for its length, so a sweep needs no overlap
clips = [
    piece.clip(f"still{i}", bts.Scene(540, 960).add(bts.Layer.fill(colour)), start=a, end=b)
    for i, (colour, a, b) in enumerate(zip(colours, cuts, cuts[1:], strict=False))
]
for a, b, t in zip(clips, clips[1:], cuts[1:-1], strict=False):
    piece.transition(a, b, at=t, length=SWEEP, kind=btv.Sweep("light", angle=90))

# a push-in over the whole piece, a little sway on top (shake is a sum of signals)
piece.camera(zoom=lambda t: 1.0 + 0.08 * t / 3.0, dx=lambda t: 0.004 * np.sin(t * 9.0))
print(piece.render("montage.mp4"))  # RenderStats(frames=72, drawn=72, composed=71)
