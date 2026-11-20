"""A loop that closes: time wraps and the soundtrack loops on the bar.

`loop=True` makes the piece's clock a circle: the value after the last frame is the value of the first, so every mapping must be
periodic over the piece (a wave that completes whole turns, a signal that is the same at both ends). The render checks the seam
and names the first mapping whose ends differ.
"""

import bullterrier_audio as bta
import bullterrier_video as btv
from bullterrier_control import Keyframe, lib

from badge_scene import badge  # a scene with a layer `ring` that turns and a `glow`

piece = btv.Piece(btv.Profile.square(width=1080, fps=30), loop=True)
m = piece.song(bta.load("groove.wav"), span=btv.bars(4))
ring = piece.clip("badge", badge())
ring.map("ring.rotate", lib.keyframes(points=[Keyframe(t=0, value=0), Keyframe(t=m.duration, value=360)]))  # 360 wraps to 0: the seam is invisible
ring.map("glow.opacity", 0.3 + 0.5 * lib.lfo(shape="sine", rate=m.tempo / 60 / 2, low=0, high=1))  # once every two beats
piece.render("badge.mp4")
