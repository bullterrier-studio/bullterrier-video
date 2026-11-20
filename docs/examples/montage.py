"""A music clip from several clips of footage, cut on the beat, with a title and sweeping transitions.

Footage is a source: each frame enters a scene through a picture hole, so a title, a grade or a drawing sits over it as layers.
Decode is the second port video adds (after the encoder), so this piece comes after the still pieces.
"""

import bullterrier_audio as bta
import bullterrier_scene as bts
import bullterrier_video as btv
from bullterrier_control import Keyframe, lib

piece = btv.Piece(btv.Profile.vertical(width=1080, fps=30))
m = piece.song(bta.load("track.wav"), span=btv.around_drop(before=8.0, length=24.0))

paths = ["a.mp4", "b.mp4", "c.mp4"]
look = bts.Scene(1080, 1920).add(bts.Layer.picture(None).named("shot").fit((0, 0, 1080, 1920), "cover"))  # a hole for the frame
cuts = m.downbeats[::2]  # every second bar
clips = [
    piece.clip(f"cut{i}", look, start=a, end=b, fill={"shot.picture": btv.Footage(paths[i % 3], start=2.0 * i)})
    for i, (a, b) in enumerate(zip(cuts, cuts[1:]))
]
for a, b, t in zip(clips, clips[1:], cuts[1:]):
    piece.transition(a, b, at=t, length=0.25, kind=btv.Sweep("light", angle=90))

headline = bts.Layer.text("AFTER HOURS", size=140).named("headline")
title = piece.clip("title", bts.Scene(1080, 1920).add(headline), start=m.drop, end=m.drop + 3, track=1)  # a second track, over the cuts
title.map("headline.opacity", lib.keyframes(points=[Keyframe(t=m.drop, value=0), Keyframe(t=m.drop + 0.2, value=1), Keyframe(t=m.drop + 2.5, value=1), Keyframe(t=m.drop + 3, value=0)]))
piece.render("clip.mp4")
