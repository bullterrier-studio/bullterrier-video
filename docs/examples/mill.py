"""The mill video (10 s, 9:16, 30 fps): a night photo with window lights, a glow, a drawn-on logo and four words, on a song.

The scene is scene's business (`mill_scene.load_scene` builds it). Without video, a hand-written driver maps every value each frame
and a shell script cuts the song and runs ffmpeg. Here video owns what is left: the soundtrack span, the song's moments, which
signal moves which value, the frame, the file. A value is held to the range the scene describes (a turn wrapped) by the mapping,
so there is no clamping.
"""

import bullterrier_audio as bta
import bullterrier_control as btc
import bullterrier_video as btv
from bullterrier_control import Keyframe, lib

from mill_scene import load_scene  # builds the scene: layers night, lit0..2, glow, roof, sign, logo, logo_glow, word1..4

song = bta.load("song.wav")


def build(before: float = 4.0) -> btv.Piece:
    """The piece, with the song's drop `before` seconds in."""
    piece = btv.Piece(btv.Profile.vertical(width=720, fps=30))
    mill = piece.clip("mill", load_scene())
    m = piece.song(song, span=btv.around_drop(before=before, length=10.0), fade_out=0.5)
    drop = m.drop  # seconds from the start of the piece; raises if the song has no drop

    def level(band, **kw):  # the song's levels are plain data; a signal is control's
        lv = m.band(band, **kw)
        return btc.from_values(lv.values, lv.rate, start=lv.start)

    key = lambda *pts: lib.keyframes(points=[Keyframe(t=t, value=v, ease=e) for t, v, e in pts])
    kicks = btc.events(m.kicks)
    landed = key((drop - 0.001, 0, "linear"), (drop, 1, "linear"))
    pulse = lib.slew(0.5 * level("bass") * (1 - landed) + (1 - level("bass", over=(drop, None))) * landed, rise=0.03, fall=0.2)
    kick = lib.envelope(events=kicks, attack=0.0, decay=0.22)
    slam = key((drop - 0.02, 0, "linear"), (drop, 1, "out"), (drop + 0.45, 0, "linear"))
    charge = key((drop - 1.6, 0, "in"), (drop, 1, "linear"))
    shake = 3.0 * pulse * landed

    for g in range(3):  # the groups of windows pulse a moment apart
        mill.map(f"lit{g}.opacity", 0.7 * pulse, lag=0.06 * (g + 1))
    mill.map("glow.look.swell.amount", 0.05 + 0.35 * pulse + 0.2 * kick + 0.1 * slam)
    mill.map("roof.look.roofline.glow", 10 + 26 * pulse)
    mill.map("logo.drawing.ring_on", key((0.3, 0, "in-out"), (3.2, 1, "linear")))
    mill.map("logo.drawing.tri_on", key((2.0, 0, "in-out"), (drop - 0.1, 1, "linear")))
    mill.map("logo.drawing.landed", landed)
    mill.map("logo.scale", 1 - 0.05 * charge * (1 - landed) + 0.28 * slam + 0.04 * kick * landed)
    mill.map("logo.dx", shake * lib.lfo(shape="sine", rate=23.7, low=-1, high=1))
    mill.map("logo.dy", shake * lib.lfo(shape="sine", rate=31.3, phase=0.16, low=-1, high=1))
    words = lib.window(events=lib.thin(events=lib.window(events=kicks, start=drop + 0.4), gap=0.3), first=4)  # control picks the kicks
    for n, e in enumerate(words, start=1):  # one word lands on each of four kicks
        word = key((e.t, 0, "linear"), (e.t + 0.3, 1, "linear"))
        mill.map(f"word{n}.opacity", word)
        mill.map(f"word{n}.scale", 1.35 - 0.35 * word)
    return piece


if __name__ == "__main__":
    build().render("mill.mp4")
