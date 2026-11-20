"""The mill for a client: the same piece in several styles, looked at before any is rendered.

A piece is a value: `copy()` is cheap, `piece[name]` is a clip, `set` changes constants on it (a look is addresses with values), `swap` puts another
picture in it; a variant never touches the piece it came from. A timing change builds the piece again, because its mappings hold the
song's moments. `sheet` draws chosen moments at a small size, one image
per variant, in seconds; `render_all` renders the survivors in one batch.

Open (undecided): whether a variant is only a look, as the first two below, or also timing and
choreography, as the third. This file shows both so the API does not decide it.
"""

import bullterrier_video as btv
from mill import build  # mill.py's piece, built and not rendered

STYLES = {
    "violet": {"night.look.grade.exposure": -0.35},
    "acid": {"night.look.grade.exposure": -0.1, "lit0.color": "#c8ff3f", "lit1.color": "#3fffb0", "lit2.color": "#fff03f"},
}

base = build()
variants = {}
for name, look in STYLES.items():
    variants[name] = base.copy()
    variants[name]["mill"].set(look)  # a clip by its name

variants["late"] = build(before=2.0)  # a timing variant is built again: its drop is two seconds in, and every mapping follows it

moments = [0.5, 3.0, 3.95, 4.1, 8.5]  # the drop of the others is at 4.0
for name, variant in variants.items():
    variant.sheet(f"{name}.png", at=moments, width=240)  # five frames side by side

btv.render_all(variants, folder="out", workers=2)  # out/violet.mp4, out/acid.mp4, out/late.mp4
