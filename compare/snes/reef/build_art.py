"""Paint the reef pair's art: one coral reef and sixteen species of fish.

  python3 compare/snes/reef/build_art.py

Writes, deterministically (fixed seed):

  crates/tg16-core/toys/reef/assets/reef.png   the reef, 256x224, open water painted in
  crates/tg16-core/toys/reef/assets/fish.png   16 fish, 32x16 each, one column;
                                               each species has its own 15 colours
                                               (8-shade body, 5-shade fins/stripes,
                                               black and white eye)
  compare/snes/reef/assets/reef.png            the same reef, open water transparent
  compare/snes/reef/assets/fish.png            the same 16 fish in 32x32 cells,
                                               redrawn for 8 OBJ palettes

The SNES has 8 sprite palettes to the TG16's 16, so its fish are drawn the way
an SNES artist would plan them: two species share each palette (black and
white for the eyes, then 6 colours each: a 4-shade body and 2-shade fins),
instead of each species getting a palette of 15 to itself.
"""
import colorsys
import os

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", "..", ".."))
TG16 = os.path.join(ROOT, "crates", "tg16-core", "toys", "reef", "assets")
SNES = os.path.join(HERE, "assets")
W, H = 256, 224
rng = np.random.default_rng(1994)


def hsv(h, s, v):
    return np.array(colorsys.hsv_to_rgb(h % 1.0, s, v)) * 255


# ---------------------------------------------------------------- the reef

img = np.zeros((H, W, 3))
alpha = np.zeros((H, W), bool)
yy, xx = np.mgrid[0:H, 0:W]
noise = rng.random((H, W))


def smooth_noise(scale, seed):
    r = np.random.default_rng(seed)
    g = r.random((H // scale + 2, W // scale + 2))
    im = Image.fromarray((g * 255).astype(np.uint8)).resize(
        (W + 2 * scale, H + 2 * scale), Image.BICUBIC)
    return np.asarray(im, float)[:H, :W] / 255


n1 = smooth_noise(16, 1)
n2 = smooth_noise(6, 2)


def paint(mask, base_hue, sat, v0, v1, shade, hue_jitter=0.0):
    """Fill mask with a ramp from v0 (shade 0) to v1 (shade 1)."""
    global img, alpha
    s = np.clip(shade, 0, 1)
    h = base_hue + hue_jitter * (n2 - 0.5)
    v = v0 + (v1 - v0) * s
    rgb = np.zeros((H, W, 3))
    # vectorised hsv -> rgb
    hh = (h % 1.0) * 6
    i = np.floor(hh).astype(int) % 6
    f = hh - np.floor(hh)
    p = v * (1 - sat)
    q = v * (1 - sat * f)
    t = v * (1 - sat * (1 - f))
    sel = [(v, t, p), (q, v, p), (p, v, t), (p, q, v), (t, p, v), (v, p, q)]
    for k, (r, g, b) in enumerate(sel):
        m = i == k
        rgb[m] = np.stack([np.broadcast_to(r, (H, W))[m], np.broadcast_to(g, (H, W))[m],
                           np.broadcast_to(b, (H, W))[m]], -1)
    img[mask] = rgb[mask] * 255
    alpha[mask] = True


# Distant rocks: dark blue-green silhouettes behind the reef.
far = 96 + 14 * np.sin(xx / 23.0) + 8 * np.sin(xx / 9.0 + 1) + 6 * n1 * 2
paint(yy > far, 0.53, 0.55, 0.22, 0.34, (yy - far) / 60 + 0.3 * n2)

# Rock: the reef body, purple-brown, lit from above.
rock_top = 132 + 18 * np.sin(xx / 31.0 + 2) + 10 * np.sin(xx / 11.0) + 10 * n1
paint(yy > rock_top, 0.05, 0.35, 0.55, 0.22, (yy - rock_top) / 50 + 0.35 * n2, 0.1)

# Sand at the bottom.
sand_top = 198 + 4 * np.sin(xx / 17.0)
paint(yy > sand_top, 0.11, 0.38, 0.92, 0.62,
      0.5 + 0.5 * np.sin((xx + 3 * yy) / 5.0) * 0.6 + 0.3 * n2)


def disc(cx, cy, rx, ry):
    return ((xx - cx) / rx) ** 2 + ((yy - cy) / ry) ** 2 <= 1


# Kelp: tall green strands rising into the open water.
for k, (x0, hgt, hue) in enumerate([(14, 120, 0.25), (58, 90, 0.22), (150, 110, 0.28),
                                    (204, 130, 0.24), (238, 80, 0.30)]):
    base = 200
    for y in range(base - hgt, base):
        sway = 5 * np.sin(y / 13.0 + k)
        cx = x0 + sway
        wd = 3 + 2 * np.sin(y / 7.0 + k * 2)
        m = (yy == y) & (np.abs(xx - cx) <= wd)
        paint(m, hue, 0.75, 0.35, 0.85, (xx - cx + wd) / (2 * wd + 0.01) * 0.8 + 0.2 * n2)

# Brain corals: yellow-green domes with grooves.
for cx, cy, r, hue in [(40, 168, 18, 0.17), (128, 150, 14, 0.14), (226, 162, 16, 0.19)]:
    m = disc(cx, cy, r, r * 0.8) & (yy < cy + 4)
    groove = 0.5 + 0.5 * np.sin((xx - cx) * 0.9 + np.sin((yy - cy) * 0.5) * 3)
    paint(m, hue, 0.7, 0.35, 0.95, (1 - (yy - cy + r) / (2 * r)) * 0.7 + 0.3 * groove)

# Fan corals: magenta / violet fans of branching lines.
for cx, cy, r, hue in [(92, 150, 26, 0.87), (180, 140, 30, 0.78), (250, 150, 18, 0.92)]:
    ang = np.arctan2(cy - yy, xx - cx)
    rad = np.hypot(xx - cx, yy - cy)
    fan = (rad < r) & (ang > 0.35) & (ang < np.pi - 0.35)
    veins = (np.abs(np.sin(ang * 14)) < 0.35) | (np.abs(np.sin(rad * 0.6)) < 0.18)
    paint(fan & veins, hue, 0.65, 0.4, 0.95, 1 - rad / r * 0.8 + 0.2 * n2)

# Staghorn: orange branches.
for cx, cy, hue in [(66, 182, 0.07), (162, 176, 0.05), (118, 190, 0.09)]:
    for b in range(7):
        a = np.pi / 2 + (b - 3) * 0.32
        for s in range(0, 28):
            px = cx + s * np.cos(a) + 3 * np.sin(s / 4 + b)
            py = cy - s * np.sin(a)
            m = disc(px, py, 2.2, 2.2)
            paint(m, hue, 0.85, 0.55, 1.0, 0.3 + 0.7 * s / 28)

# Tube sponges: red-violet cylinders.
for cx, h, hue in [(24, 40, 0.98), (32, 30, 0.95), (196, 44, 0.0), (206, 34, 0.96)]:
    m = (np.abs(xx - cx) <= 4) & (yy > 196 - h) & (yy < 200)
    paint(m, hue, 0.8, 0.3, 0.85, 1 - np.abs(xx - cx) / 5)
    paint(disc(cx, 196 - h, 4, 2), hue, 0.9, 0.12, 0.2, n2)

# Anemones: pink tentacle clusters.
for cx, cy in [(104, 196), (240, 196), (146, 166)]:
    for k in range(13):
        a = np.pi * (0.1 + 0.8 * k / 12)
        for s in range(14):
            px = cx + s * np.cos(a) * 0.9 + 1.5 * np.sin(s / 2 + k)
            py = cy - s * np.sin(a)
            paint(disc(px, py, 1.3, 1.3), 0.93, 0.45, 0.6, 1.0, s / 14)

# Starfish on the sand.
for cx, cy, hue in [(84, 210, 0.02), (176, 214, 0.08)]:
    ang = np.arctan2(yy - cy, xx - cx)
    rad = np.hypot(xx - cx, yy - cy)
    m = rad < 4 + 4 * np.abs(np.cos(ang * 2.5))
    paint(m, hue, 0.8, 0.5, 1.0, 1 - rad / 8)

rgba = np.zeros((H, W, 4), np.uint8)
rgba[..., :3] = np.clip(img, 0, 255).astype(np.uint8)
rgba[..., 3] = np.where(alpha, 255, 0)
reef = Image.fromarray(rgba, "RGBA")

# The TG16 version paints the open water into the picture, so the importer's
# ordered dither smooths the gradient across the 9-bit colour steps (what PC
# Engine artists did); the SNES version leaves it transparent for an HDMA
# gradient in 15-bit colour, which costs it nothing.
k = np.clip(yy / 150.0, 0, 1)[..., None]
water = np.array([83.0, 160, 236]) * (1 - k) + np.array([14.0, 64, 158]) * k
painted = np.where(alpha[..., None], img, water)
rgba_w = np.zeros((H, W, 4), np.uint8)
rgba_w[..., :3] = np.clip(painted, 0, 255).astype(np.uint8)
rgba_w[..., 3] = 255
reef_water = Image.fromarray(rgba_w, "RGBA")

# ---------------------------------------------------------------- the fish

# 16 species: (body hue, body saturation, fin hue, stripe pattern).
SPECIES = [
    (0.58, 0.85, 0.15, "stripes"),  # blue tang
    (0.07, 0.95, 0.00, "bands"),    # clownfish (white bands)
    (0.15, 0.90, 0.60, "plain"),    # yellow tang
    (0.83, 0.70, 0.13, "split"),    # royal gramma
    (0.33, 0.70, 0.95, "stripes"),  # green wrasse
    (0.97, 0.80, 0.55, "spots"),    # red hind
    (0.50, 0.75, 0.08, "bands"),    # teal
    (0.75, 0.60, 0.35, "plain"),    # violet
    (0.03, 0.70, 0.62, "stripes"),  # coral pink
    (0.62, 0.40, 0.10, "spots"),    # grey-blue
    (0.11, 0.65, 0.45, "split"),    # orange-cyan
    (0.45, 0.85, 0.85, "plain"),    # aqua-magenta
    (0.90, 0.85, 0.20, "bands"),    # magenta-yellow
    (0.25, 0.80, 0.70, "spots"),    # lime-blue
    (0.68, 0.85, 0.02, "split"),    # indigo-red
    (0.09, 0.40, 0.58, "stripes"),  # tan-blue
]


def fish_pixels(spec, body_shades, fin_shades):
    """32x16 index grid plus colours. Index 0 transparent, 1 black, 2 white,
    then body shades, then fin shades."""
    hb, sb, hf, pat = spec
    body = [hsv(hb, sb * (0.75 + 0.25 * k / max(1, body_shades - 1)),
                0.28 + 0.72 * k / max(1, body_shades - 1)) for k in range(body_shades)]
    fins = [hsv(hf, 0.85, 0.45 + 0.55 * k / max(1, fin_shades - 1)) for k in range(fin_shades)]
    if pat == "bands":  # white-ish bands use the fin ramp, desaturated
        fins = [hsv(hf, 0.15, 0.6 + 0.4 * k / max(1, fin_shades - 1)) for k in range(fin_shades)]
    cols = [np.zeros(3), np.array([8.0, 8, 16]), np.array([248.0, 248, 248])] + body + fins
    g = np.zeros((16, 32), int)
    for y in range(16):
        for x in range(32):
            u, v = (x - 13) / 11.0, (y - 7.5) / 6.0
            in_body = u * u + v * v <= 1
            in_tail = 23 <= x <= 30 and abs(y - 7.5) <= (x - 22) * 0.9 and abs(y - 7.5) > (30 - x) * 0.25 - 1
            in_fin = 6 <= x <= 18 and y <= 3 and (y >= 3 - (x - 6) * 0.4) and not in_body
            if not (in_body or in_tail or in_fin):
                continue
            if in_tail or in_fin:
                sh = (x / 31 if in_tail else (3 - y) / 3)
                g[y, x] = 3 + body_shades + min(fin_shades - 1, int(sh * fin_shades))
                continue
            # Body: lit from above-front.
            light = 1 - (v + 1) / 2 * 0.85 - max(0, u) * 0.25
            idx = 3 + min(body_shades - 1, max(0, int(light * body_shades)))
            fin_mark = False
            if pat == "stripes":
                fin_mark = abs(v) < 0.22 and u > -0.85
            elif pat == "bands":
                fin_mark = abs(u + 0.25) < 0.12 or abs(u - 0.45) < 0.1
            elif pat == "spots":
                fin_mark = (x + 2 * y) % 7 == 0 and (y * 3 + x) % 5 < 2 and abs(v) < 0.8
            elif pat == "split":
                fin_mark = u > 0.15
            if fin_mark:
                k = min(fin_shades - 1, max(0, int((1 - (v + 1) / 2) * fin_shades)))
                idx = 3 + body_shades + k
            g[y, x] = idx
    # Eye.
    g[5, 5] = g[5, 6] = g[6, 5] = 2
    g[6, 6] = 1
    # Mouth line / outline under the belly.
    for x in range(4, 22):
        if g[14, x]:
            g[14, x] = 1
    return g, cols


def fish_sheet(body_shades, fin_shades, cell_h):
    sheet = np.zeros((cell_h * 16, 32, 4), np.uint8)
    for k, spec in enumerate(SPECIES):
        g, cols = fish_pixels(spec, body_shades, fin_shades)
        for y in range(16):
            for x in range(32):
                if g[y, x]:
                    sheet[k * cell_h + y, x, :3] = np.clip(cols[g[y, x]], 0, 255)
                    sheet[k * cell_h + y, x, 3] = 255
    return Image.fromarray(sheet, "RGBA")


os.makedirs(TG16, exist_ok=True)
os.makedirs(SNES, exist_ok=True)
reef_water.save(os.path.join(TG16, "reef.png"), optimize=True)
reef.save(os.path.join(SNES, "reef.png"), optimize=True)
fish_sheet(8, 5, 16).save(os.path.join(TG16, "fish.png"), optimize=True)
# SNES: 32x32 OBJ cells (fish in the top half), 4-shade bodies and 2-shade
# fins, so two species plus black and white fit one palette of 15.
fish_sheet(4, 2, 32).save(os.path.join(SNES, "fish.png"), optimize=True)
print("wrote", TG16, "and", SNES)
