"""Cut the TG16 photo toy's panorama into the two halves this SNES port imports.

  python3 compare/snes/photo/build_art.py

Source: crates/tg16-core/toys/photo/assets/lake.png (512x224). A SNES BG map
entry has a 10-bit tile number, so one layer can address 1024 tiles; the
whole photo needs about 1400 unique ones. The port therefore puts the left
half on BG1 and the right half on BG2 (each about 700 tiles, each with its
own character base), side by side in two 64x32 maps. Both halves go through
the stock SNES importer unchanged.
"""
import os
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "..", "..", "..", "crates", "tg16-core", "toys", "photo", "assets", "lake.png")

im = Image.open(SRC).convert("RGB")
assert im.size == (512, 224), im.size
os.makedirs(os.path.join(HERE, "assets"), exist_ok=True)
im.crop((0, 0, 256, 224)).save(os.path.join(HERE, "assets", "lake_left.png"), optimize=True)
im.crop((256, 0, 512, 224)).save(os.path.join(HERE, "assets", "lake_right.png"), optimize=True)
