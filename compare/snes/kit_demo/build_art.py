"""Road texture for the SNES kit_demo port: assets/road.png (1024x1024, Mode 7).

  python3 compare/snes/kit_demo/build_art.py

The same art as the road rows of crates/tg16-core/toys/kit_demo.lua (BAT rows
16..31): 4-pixel grey stripes, with a 2x4 white mark every 64 pixels across
and every 32 down. On the TG16 the floor picks one texture row per line; here
it is a real plane, so the pattern is tiled across the whole 1024x1024 map.
"""
import os
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))


def c3(r, g, b):  # TG16 3-bit channels, snapped to BGR555
    return tuple((v * 255 // 7) & 0xF8 for v in (r, g, b))


DARK, LIGHT, MARK = c3(2, 2, 2), c3(3, 3, 3), c3(7, 7, 7)

im = Image.new("RGB", (1024, 1024))
px = im.load()
for y in range(1024):
    for x in range(1024):
        tx, ty, ix, iy = x // 8, y // 8, x % 8, y % 8
        if ty % 4 == 0 and tx % 8 == 3:  # the TG16 "road marks" tile
            c = MARK if 3 <= ix <= 4 and 2 <= iy <= 5 else DARK
        else:  # the stripes tile
            c = DARK if iy < 4 else LIGHT
        px[x, y] = c
os.makedirs(os.path.join(HERE, "assets"), exist_ok=True)
im.save(os.path.join(HERE, "assets", "road.png"), optimize=True)
