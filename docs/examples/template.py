"""One piece, a post for each row: a template with holes (a photo and a title) filled per run.

The scene has a hole for the photo and a text layer; a variant sets them as constants and renders. Nothing here is saved to a
file: a piece holds Python objects (a scene, signals), so it is built by a program, not loaded.
"""

import csv

import bullterrier_audio as bta
import bullterrier_video as btv
from bullterrier_image import imageio

from post_scene import post  # (): a scene with `photo.picture` as a hole and `title.text`

base = btv.Piece(btv.Profile.vertical(width=1080, fps=30))
base.song(bta.load("bed.wav"), span=btv.bars(4), fade_out=0.5)
base.clip("post", post())

variants = {}
for row in csv.DictReader(open("posts.csv", encoding="utf-8")):  # slug, photo, title
    variants[row["slug"]] = base.copy()
    variants[row["slug"]].clip("post").set({"photo.picture": imageio.load(row["photo"]), "title.text": row["title"]})

btv.render_all(variants, folder="posts", workers=2)
