"""The mill for a client: the same piece in several styles, looked at before any is rendered.

A piece is a value: `copy()` is cheap, `set` changes constants on one clip (a look is addresses with values), `swap` puts another
picture in a clip, and a variant never touches the piece it came from. `sheet` draws chosen moments at a small size, one image
per variant, in seconds; `render_all` renders the survivors in one batch.

Open (undecided): whether a variant is only a look, as the first two below, or also timing and
choreography, as the third. This file shows both so the API does not decide it.
"""

import bullterrier_video as btv
from mill import m, piece as base, song  # the piece of mill.py, built and not yet rendered

STYLES = {
    "violet": {"night.look.grade.exposure": -0.35},
    "acid": {"night.look.grade.exposure": -0.1, "lit0.color": "#c8ff3f", "lit1.color": "#3fffb0", "lit2.color": "#fff03f"},
}

variants = {}
for name, look in STYLES.items():
    variants[name] = base.copy()
    variants[name].clip("mill").set(look)

late_drop = base.copy()  # a timing variant: the same layers, the song's window starts a bar later
late_drop.song(song, span=btv.around_drop(before=2.0, length=10.0), fade_out=0.5)
variants["late"] = late_drop

moments = [0.5, 3.0, m.drop - 0.05, m.drop + 0.1, 8.5]
for name, variant in variants.items():
    variant.sheet(f"{name}.png", at=moments, width=240)  # five frames side by side

btv.render_all(variants, folder="out", workers=2)  # out/violet.mp4, out/acid.mp4, out/late.mp4
