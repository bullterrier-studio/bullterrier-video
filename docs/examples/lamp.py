"""A lamp whose colour follows the bass (6 s, 9:16): a colour address is mapped through a `Ramp`, not as a number.

The ramp is video's: two or more stops, evenly spaced, the signal's 0 to 1 picks the place, and the blend between stops is
done in OKLab, so the middle of blue and red is a clean violet, not mud. The picture gets three floats a frame, never a hex string.
"""

import bullterrier_audio as bta
import bullterrier_control as btc
import bullterrier_scene as bts
import bullterrier_video as btv

song = bta.load("song.wav")


def build(photo, depth) -> btv.Piece:
    """`photo` and `depth` are float32 arrays of one size (height, width, 3) and (height, width)."""
    height, width = photo.shape[:2]
    lamp = {"steps": [{"effect": "point_light", "params": {"color": "#ffe2b8"}}]}
    scene = bts.Scene(width, height).add(
        bts.Layer.fill("#202020").named("ground"),
        bts.Layer.picture(photo, maps={"depth": depth}).look(lamp).named("glow"),
    )
    piece = btv.Piece(btv.Profile.vertical(width=720, fps=30))
    m = piece.song(song, span=btv.around_drop(before=2.0, length=6.0))
    bass = m.band("bass")
    bass = btc.from_values(bass.values, bass.rate, start=bass.start)
    # `scene.parameters()` lists the colour as "glow.look.point_light.color", kind color; the last argument is the signal
    piece.clip("lamp", scene).map("glow.look.point_light.color", btv.Ramp("#3060ff", "#ff2020", bass))
    return piece
