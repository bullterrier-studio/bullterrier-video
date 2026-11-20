"""A still of the dog with letters that grow like mold in time with the song, behind it and in front of it.

The growth is a pure function of (growth, seed) in vector; video makes `growth` a number that only rises, pushed up by the bass.
Frames are heavy (seconds each at 540x771), so the render is parallel and the piece is a seamless loop.

Open: control's running-sum recurrence (`Signal::rendered`) may need a named shaper; `accumulate` below stands for it.
"""

import bullterrier_audio as bta
import bullterrier_control as btc
import bullterrier_video as btv
from bullterrier_control import lib

from poster_scene import poster  # (photo) -> scene: layers sky, behind (growing letters), dog (cut out), front (growing letters)

piece = btv.Piece(btv.Profile.vertical(width=540, fps=30), loop=True)
m = piece.song(bta.load("song.wav"), span=btv.bars(8))  # 8 bars from a downbeat: a loop that closes on the beat
dog = piece.clip("poster", poster("dog.jpg"))

energy = 0.6 * m.band("bass") + 0.4 * lib.envelope(events=btc.events(m.kicks), attack=0.0, decay=0.3)
growth = lib.accumulate(energy, per_second=0.05)  # only rises; the letters join when it reaches 1
dog.set({"behind.drawing.seed": 7, "front.drawing.seed": 11})
for layer in ("behind", "front"):
    dog.map(f"{layer}.drawing.growth", growth)  # held to the parameter's own 0..1

piece.render("poster.mp4", workers=4)  # the soundtrack loops on the bar; the last frame leads into the first
