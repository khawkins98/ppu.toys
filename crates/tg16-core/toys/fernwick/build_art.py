#!/usr/bin/env python3
"""Fernwick art: draws every tile, sprite, font glyph, the logo and the
crest, and writes them as Lua data to 10_art.lua.

The art is original and drawn here: procedural tiles (grass, water, stone)
and hand-made ASCII sprites. Both machines load the same 10_art.lua; each
turns the index data into its own VRAM format (TG16 tile()/pattern(), SNES
4bpp/2bpp planar words and a Mode 7 plane).

Run from anywhere:  python3 crates/tg16-core/toys/fernwick/build_art.py
It also copies the shared Lua (10_art, 20_world, 30_game) into
compare/snes/fernwick/ so the two toys never drift (a test checks this), and
writes preview PNGs to --preview DIR if given.
"""
import os
import random
import re
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
SNES_DIR = os.path.join(ROOT, "compare", "snes", "fernwick")
SHARED = ["10_art.lua", "20_world.lua", "30_game.lua"]

# --------------------------------------------------------------- palettes
# 8-bit RGB. The TG16 rounds each channel to 3 bits, the SNES to 5.

def hexc(s):
    return tuple(int(s[i:i + 2], 16) for i in (0, 2, 4))

BGPAL = [
    # 0 field: grass, trees, bushes, path, flowers
    ["000000", "58a848", "3c8438", "8cc85c", "2a6030", "184830", "2c7038", "4c9840",
     "88c860", "482818", "805028", "e0c888", "b89860", "f8f8e8", "e85058", "103020"],
    # 1 stone: rocks, cliffs, cave mouth (with grass to sit on)
    ["000000", "58a848", "3c8438", "8cc85c", "404050", "707080", "a8a8b0", "d8d8e0",
     "583828", "906038", "c09058", "100810", "302028", "2a6030", "60883a", "181820"],
    # 2 water (4..7 cycle) and its shore
    ["000000", "204898", "3068c0", "4890e0", "3068c0", "4890e0", "88c0f0", "e8f8ff",
     "58a848", "3c8438", "e0c888", "102048", "8cc85c", "b89860", "2a6030", "000000"],
    # 3 dungeon
    ["000000", "282840", "404060", "585880", "202038", "485078", "7078a8", "a0a8d0",
     "080810", "c83018", "f88820", "ffe860", "784828", "e8c040", "406040", "101018"],
    # 4 shrine
    ["000000", "786048", "b09870", "e0d0a0", "f8f0d0", "603818", "a86828", "e0a048",
     "100808", "c8b890", "a08868", "ffd840", "f89028", "58a848", "3c8438", "8cc85c"],
]

# HUD palettes: four colours each (SNES BG3 2bpp; TG16 BG palettes 8..11).
HUDPAL = [
    ["101020", "101020", "f8f8f8", "585878"],  # text
    ["101020", "101020", "e82840", "585878"],  # hearts: red, grey
    ["101020", "101020", "40d878", "b8ffd0"],  # gem
    ["101020", "101020", "f0c840", "a07820"],  # key
]

SPRPAL = [
    # 0 hero
    ["000000", "181020", "f8c090", "c88058", "e87828", "a04018", "30a088", "186060",
     "70d8b0", "784828", "503020", "ffffff", "d83030", "901828", "c8d0e0", "202030"],
    # 1 items and effects
    ["000000", "181020", "c8d0e0", "7080a0", "ffffff", "804820", "f0c840", "b07820",
     "40d878", "188048", "b8ffd0", "e03040", "901830", "ffa0b0", "f0f0f0", "a0a0b0"],
    # 2 blob
    ["000000", "281030", "602880", "9848b8", "d080e8", "f8d8ff", "ffffff", "100810",
     "000000", "000000", "000000", "000000", "000000", "000000", "000000", "000000"],
    # 3 beetle
    ["000000", "181008", "802818", "c84820", "f08840", "ffd8a0", "402818", "ffe840",
     "000000", "000000", "000000", "000000", "000000", "000000", "000000", "000000"],
    # 4 shadow (SNES: colour math halves it onto the ground; TG16: flickers)
    ["000000", "000000", "000000", "000000", "000000", "000000", "000000", "000000",
     "000000", "000000", "000000", "000000", "000000", "000000", "000000", "000000"],
    # 5 wisp (SNES: added to what is behind it)
    ["000000", "ffffff", "b0f0ff", "60c8f0", "3078d8", "101840", "000000", "000000",
     "000000", "000000", "000000", "000000", "000000", "000000", "000000", "000000"],
]

LOGOPAL = ["000000", "200810", "fff0a0", "ffd040", "f09820", "c85818", "882810", "ffffff",
           "583020", "000000", "000000", "000000", "000000", "000000", "000000", "000000"]
CRESTPAL = ["000000", "181020", "f8e080", "e0a830", "a06820", "2850a0", "3870c8", "5898e0",
            "287048", "40a060", "e8e8f0", "a0a8c0", "f87030", "ffd860", "604020", "fff8e0"]

# ------------------------------------------------------------- utilities

def grid(w, h, v=0):
    return [[v] * w for _ in range(h)]

def from_ascii(rows, w=None):
    w = w or max(len(r) for r in rows)
    rows = [r.replace(" ", "") for r in rows]
    for r in rows:
        assert len(r) <= w, (len(r), r)
    rows = [r + "." * (w - len(r)) for r in rows]
    return [[0 if ch == "." else int(ch, 16) for ch in r] for r in rows]

def sub(g, x, y, w, h):
    return [row[x:x + w] for row in g[y:y + h]]

def flat_hex(g):
    return "".join("0123456789abcdef"[v] for row in g for v in row)

def paste(dst, src, x, y, transparent=True):
    for j, row in enumerate(src):
        for i, v in enumerate(row):
            if 0 <= y + j < len(dst) and 0 <= x + i < len(dst[0]):
                if v or not transparent:
                    dst[y + j][x + i] = v

def disc(g, cx, cy, r, v, ry=None):
    ry = ry or r
    for y in range(len(g)):
        for x in range(len(g[0])):
            if ((x + 0.5 - cx) / r) ** 2 + ((y + 0.5 - cy) / ry) ** 2 <= 1:
                g[y][x] = v

def outline(g, v, inside=None):
    """Colour transparent pixels next to opaque ones."""
    h, w = len(g), len(g[0])
    out = [row[:] for row in g]
    for y in range(h):
        for x in range(w):
            if g[y][x] == 0:
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    xx, yy = x + dx, y + dy
                    if 0 <= xx < w and 0 <= yy < h and g[yy][xx] not in (0, v):
                        out[y][x] = v
                        break
    return out

# ------------------------------------------------------------------ tiles
# Blocks are 16x16 metatiles keyed by their map character.

rng = random.Random(7)

def grass(seed, tufts=3, specks=10):
    r = random.Random(seed)
    g = grid(16, 16, 1)
    for _ in range(specks):
        g[r.randrange(16)][r.randrange(16)] = 2
    for _ in range(tufts):
        x, y = r.randrange(1, 14), r.randrange(2, 15)
        g[y][x] = 4
        g[y][x + 2] = 4
        g[y - 1][x] = 3
        g[y - 1][x + 2] = 3
        g[y][x + 1] = 2
    return g

def flowers(seed):
    g = grass(seed, tufts=1, specks=6)
    r = random.Random(seed + 1)
    for (x, y, c) in ((3, 3, 13), (11, 5, 14), (6, 11, 13), (13, 12, 13), (2, 9, 14)):
        x += r.randrange(-1, 2)
        g[y][x] = c
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            g[y + dy][x + dx] = 13 if c == 14 else 14
        g[y][x] = 14 if c == 13 else 11
    return g

def path(seed):
    r = random.Random(seed)
    g = grid(16, 16, 11)
    for _ in range(14):
        g[r.randrange(16)][r.randrange(16)] = 12
    for _ in range(3):
        x, y = r.randrange(1, 14), r.randrange(1, 14)
        g[y][x] = 12
        g[y][x + 1] = 12
        g[y - 1][x] = 13
    return g

def tree():
    """32x32: canopy (rows 0-21) over a trunk. Returns (art, canopy mask)."""
    t = grid(32, 32, 0)
    for k, (gx, gy) in enumerate(((0, 0), (16, 0), (0, 16), (16, 16))):
        paste(t, grass(21 + k, tufts=1, specks=6), gx, gy)
    # shadow on the grass under the canopy edge
    disc(t, 16, 27, 14, 2, 4)
    canopy = grid(32, 32, 0)
    for (cx, cy, r) in ((16, 11, 12), (9, 14, 8), (23, 14, 8), (16, 6, 9)):
        disc(canopy, cx, cy, r, 6)
    # shading: dark bottom right, light top left
    for y in range(32):
        for x in range(32):
            if canopy[y][x]:
                d = (x - 11) * 0.5 + (y - 7)
                n = ((x * 7 + y * 13) % 5) - 2
                v = d + n * 1.5
                canopy[y][x] = 8 if v < -6 else 7 if v < 1 else 6 if v < 9 else 5
    # leaf clumps: little arcs of highlight
    for (x, y) in ((10, 6), (16, 4), (21, 8), (8, 12), (14, 11), (19, 14), (11, 17), (24, 17)):
        for dx in range(-2, 3):
            if canopy[y][x + dx]:
                canopy[y][x + dx] = 8 if canopy[y][x + dx] >= 7 else 7
        if canopy[y + 1][x - 3]:
            canopy[y + 1][x - 3] = 5
        if canopy[y + 1][x + 3]:
            canopy[y + 1][x + 3] = 5
    canopy = outline(canopy, 15)
    # trunk
    for y in range(20, 30):
        for x in range(13, 19):
            t[y][x] = 10 if x < 16 else 9
    for x in (12, 19):
        t[29][x] = 9
    t[29][11] = 15
    t[29][20] = 15
    for y in range(20, 30):
        t[y][12] = 15 if y < 29 else t[y][12]
        t[y][19] = 15 if y < 29 else t[y][19]
    t[24][14] = 9
    t[26][17] = 15
    paste(t, canopy, 0, 0)
    return t, canopy

def bush():
    g = grass(31, tufts=0, specks=4)
    b = grid(16, 16, 0)
    for (cx, cy, r) in ((8, 9, 6.5), (5, 8, 4), (11, 8, 4), (8, 5, 4.5)):
        disc(b, cx, cy, r, 6)
    for y in range(16):
        for x in range(16):
            if b[y][x]:
                d = (x - 6) * 0.6 + (y - 5)
                b[y][x] = 8 if d < -3 else 7 if d < 2 else 6 if d < 6 else 5
    for (x, y) in ((6, 5), (10, 7), (5, 10), (9, 11)):
        b[y][x] = 8
        b[y + 1][x + 1] = 5
    b = outline(b, 15)
    paste(g, b, 0, 0)
    return g

def rock():
    g = grass(41, tufts=1, specks=4)
    b = grid(16, 16, 0)
    disc(b, 8, 9, 7, 5, 6)
    for y in range(16):
        for x in range(16):
            if b[y][x]:
                d = (x - 5) * 0.7 + (y - 6)
                b[y][x] = 7 if d < -3 else 6 if d < 1 else 5 if d < 6 else 4
    b[6][5] = 7
    b[7][9] = 4
    b[8][10] = 4
    b[9][10] = 4
    b[11][4] = 4
    b = outline(b, 15)
    for x in range(3, 14):
        if b[15][x] == 0:
            g[15][x] = 2
    paste(g, b, 0, 0)
    return g

def cliff(seed):
    r = random.Random(seed)
    g = grid(16, 16, 9)
    for y in range(16):
        for x in range(16):
            band = (y + (x // 5) % 2) % 6
            g[y][x] = 10 if band == 0 else 8 if band == 5 else 9
    for _ in range(6):
        x, y = r.randrange(16), r.randrange(16)
        g[y][x] = 8
    for _ in range(4):
        x, y = r.randrange(16), r.randrange(16)
        g[y][x] = 10
    # mossy top lip
    for x in range(16):
        g[0][x] = 14 if (x * 5) % 7 > 1 else 13
        if (x * 3) % 5 == 0:
            g[1][x] = 13
    return g

def cave():
    g = cliff(51)
    for y in range(3, 16):
        for x in range(16):
            dx = (x - 7.5) / 6.5
            dy = (y - 16) / 13.0
            if dx * dx + dy * dy <= 1:
                g[y][x] = 11
            elif dx * dx + dy * dy <= 1.35:
                g[y][x] = 12 if g[y][x] != 11 else 11
    return g

def water(seed):
    g = grid(16, 16, 1)
    for y in range(16):
        for x in range(16):
            g[y][x] = 1 if (x + y) % 9 else 2
            k = (x + 2 * y) % 16
            if k < 4:
                g[y][x] = 4 + ((x // 4 + y // 4) % 4)
    for (x, y) in ((3, 2), (10, 6), (5, 11), (13, 13)):
        for dx in range(3):
            g[y][x + dx] = 4 + (dx + y) % 4
    return g

def dwall(seed):
    g = grid(16, 16, 5)
    for y in range(16):
        for x in range(16):
            row = y // 4
            off = 4 if row % 2 else 0
            bx = (x + off) % 8
            if y % 4 == 3 or bx == 7:
                g[y][x] = 4
            elif y % 4 == 0 or bx == 0:
                g[y][x] = 6
            else:
                g[y][x] = 5
    r = random.Random(seed)
    for _ in range(4):
        x, y = r.randrange(16), r.randrange(16)
        if g[y][x] == 5:
            g[y][x] = 7 if r.random() < 0.5 else 14
    return g

def dfloor(seed):
    g = grid(16, 16, 2)
    r = random.Random(seed)
    for y in range(16):
        for x in range(16):
            if x % 8 == 0 or y % 8 == 0:
                g[y][x] = 1
            elif x % 8 == 1 or y % 8 == 1:
                g[y][x] = 3 if r.random() < 0.5 else 2
    for _ in range(5):
        x, y = r.randrange(16), r.randrange(16)
        if g[y][x] == 2:
            g[y][x] = 1
    return g

def stairs():
    g = grid(16, 16, 1)
    for y in range(16):
        step = y // 4
        for x in range(1, 15):
            g[y][x] = 7 - step if y % 4 else 15
        g[y][0] = 15
        g[y][15] = 15
    return g

def pedestal():
    g = dfloor(61)
    for y in range(6, 15):
        for x in range(3, 13):
            g[y][x] = 6 if y < 8 else 5 if x < 11 else 4
    for x in range(3, 13):
        g[6][x] = 7
        g[14][x] = 15
    for y in range(6, 15):
        g[y][2] = 15
        g[y][13] = 15
    return g

def torch():
    g = dwall(71)
    for y in range(8, 14):
        g[y][7] = 12
        g[y][8] = 12
    g[8][6] = 15
    g[8][9] = 15
    disc(g, 8, 6, 3, 10, 4)
    disc(g, 8, 6.5, 2, 11, 2.5)
    g[2][8] = 9
    g[3][7] = 10
    return g

def swall(seed):
    g = grid(16, 16, 2)
    for y in range(16):
        for x in range(16):
            row = y // 8
            off = 8 if row % 2 else 0
            bx = (x + off) % 16
            if y % 8 == 7 or bx == 15:
                g[y][x] = 1
            elif y % 8 == 0 or bx == 0:
                g[y][x] = 4
            elif y % 8 == 1:
                g[y][x] = 3
            else:
                g[y][x] = 2
    return g

def sdoor():
    g = swall(81)
    for y in range(2, 16):
        for x in range(2, 14):
            g[y][x] = 6 if x < 8 else 5
        g[y][2] = 8
        g[y][13] = 8
    for x in range(2, 14):
        g[2][x] = 7
    for y in range(4, 16, 4):
        for x in range(3, 13):
            g[y][x] = 7 if x < 8 else 6
    # keyhole
    disc(g, 8, 9, 1.6, 8)
    g[11][8] = 8
    g[12][8] = 8
    g[11][7] = 8
    return g

def sfloor():
    g = grid(16, 16, 9)
    for y in range(16):
        for x in range(16):
            if (x // 8 + y // 8) % 2:
                g[y][x] = 10
            if x % 8 == 7 or y % 8 == 7:
                g[y][x] = 1
    return g

def altar():
    g = sfloor()
    for y in range(16):
        for x in range(16):
            d = ((x - 7.5) ** 2 + (y - 7.5) ** 2) ** 0.5
            if d < 6.5:
                g[y][x] = 3 if d < 5.5 else 1
    for y in range(16):
        for x in range(16):
            d = ((x - 7.5) ** 2 + (y - 7.5) ** 2) ** 0.5
            if 2 < d < 3.5:
                g[y][x] = 11
            if d <= 2:
                g[y][x] = 12
    return g

# Blocks: char -> (palette, 16x16 art, overlay or None)
TREE, CANOPY = tree()
BLOCKS = {}

def block(ch, pal, art, over=None):
    BLOCKS[ch] = (pal, art, over)

block(".", 0, grass(1))
block(",", 0, grass(2, tufts=6, specks=14))
block("*", 0, flowers(3))
block("_", 0, path(4))
block("A", 0, sub(TREE, 0, 0, 16, 16), sub(CANOPY, 0, 0, 16, 16))
block("B", 0, sub(TREE, 16, 0, 16, 16), sub(CANOPY, 16, 0, 16, 16))
block("C", 0, sub(TREE, 0, 16, 16, 16))
block("D", 0, sub(TREE, 16, 16, 16, 16))
block("b", 0, bush())
block("r", 1, rock())
block("w", 1, cliff(5))
block("o", 1, cave())
block("~", 2, water(6))
block("#", 3, dwall(7))
block(":", 3, dfloor(8))
block("s", 3, stairs())
block("k", 3, pedestal())
block("!", 3, torch())
block("S", 4, swall(9))
block("=", 4, sdoor())
block("+", 4, sfloor())
block("x", 4, altar())

# Under the canopy, the lower layer is grass: tree tops show grass on the
# SNES's BG1 and the canopy on BG2.
GRASS_UNDER = {"A": grass(11, 1, 6), "B": grass(12, 1, 6)}
for ch in ("A", "B"):
    pal, art, over = BLOCKS[ch]
    comp = [row[:] for row in GRASS_UNDER[ch]]
    for y in range(16):
        for x in range(16):
            if art[y][x] and over[y][x] == 0 and art[y][x] not in (1, 2, 3, 4):
                comp[y][x] = art[y][x]
            if art[y][x] == 2:
                comp[y][x] = 2
    paste(comp, over, 0, 0)
    BLOCKS[ch] = (pal, comp, over, GRASS_UNDER[ch])

# --------------------------------------------------------------- sprites

SPR = {}

def sprite(name, pal, rows):
    g = from_ascii(rows, 16)
    assert len(g) == 16 and len(g[0]) == 16, name
    SPR[name] = (pal, g)

# hero (palette 0): 1 outline 2 skin 3 skin shade 4 hair 5 hair dark
# 6 tunic 7 tunic dark 8 tunic light 9 belt a boots b white c scarf
# d scarf dark e metal f eye
sprite("hero_d0", 0, [
    ".....111111.....",
    "....14444451....",
    "...1444444451...",
    "...1544444451...",
    "...1452222541...",
    "...142f22f241...",
    "....12233221....",
    "...1ccccccdc1...",
    "..1d86666668d1..",
    "..128666666721..",
    "..123999999321..",
    "...1166666711...",
    "....16677661....",
    "....1a6117a1....",
    "....1aa11aa1....",
    ".....11..11.....",
])
sprite("hero_d1", 0, [
    "................",
    ".....111111.....",
    "....14444451....",
    "...1444444451...",
    "...1544444451...",
    "...1452222541...",
    "...142f22f241...",
    "....12233221....",
    "...1ccccccdc1...",
    "..1d86666668d1..",
    "..128666666721..",
    "..123999999321..",
    "...1166666711...",
    "....1667761.....",
    "....1aa1aa1.....",
    ".....11.11......",
])
sprite("hero_da", 0, [
    ".....111111.....",
    "....14444451....",
    "...1444444451...",
    "...1544444451...",
    "...1452222541...",
    "...142f22f241...",
    "....12233221....",
    "...1ccccccdc1...",
    "..1d86666668d1..",
    "..1866666667 1..",
    "..1699999996221.",
    "...16666666122..",
    "....16677661....",
    "....1a6117a1....",
    "....1aa11aa1....",
    ".....11..11.....",
])
sprite("hero_u0", 0, [
    ".....111111.....",
    "....14444441....",
    "...1444444441...",
    "...1544444451...",
    "...1554444551...",
    "...1455555541...",
    "....15555551....",
    "...1cccccccc1...",
    "..1d77777777d1..",
    "..127777777721..",
    "..123999999321..",
    "...1177777711...",
    "....17766771....",
    "....1a7117a1....",
    "....1aa11aa1....",
    ".....11..11.....",
])
sprite("hero_u1", 0, [
    "................",
    ".....111111.....",
    "....14444441....",
    "...1444444441...",
    "...1544444451...",
    "...1554444551...",
    "...1455555541...",
    "....15555551....",
    "...1cccccccc1...",
    "..1d77777777d1..",
    "..127777777721..",
    "..123999999321..",
    "...1177777711...",
    ".....1677661....",
    ".....1aa1aa1....",
    "......11.11.....",
])
sprite("hero_ua", 0, [
    "..22............",
    "..221111111.....",
    "...14444441.....",
    "...1444444441...",
    "...1544444451...",
    "...1554444551...",
    "...1455555541...",
    "...115555551....",
    "..1dcccccccc1...",
    "..1777777777d1..",
    "...17777777721..",
    "...13999999321..",
    "...1177777711...",
    "....17766771....",
    "....1aa11aa1....",
    ".....11..11.....",
])
sprite("hero_r0", 0, [
    "....1111111.....",
    "...144444441....",
    "..14444444451...",
    "..15444444451...",
    "..1554442222 1..",
    "..15542222f21...",
    "...1542222221...",
    "....1122331.....",
    "...1dccccc1.....",
    "...1d6688821....",
    "...1766888231...",
    "...1799999931...",
    "....1666666 1...",
    "....17761a71....",
    "...1aa1.1aa1....",
    "....11...11.....",
])
sprite("hero_r1", 0, [
    "................",
    "....1111111.....",
    "...144444441....",
    "..14444444451...",
    "..15444444451...",
    "..155444222221..",
    "..15542222f21...",
    "...1542222221...",
    "....1122331.....",
    "...1dccccc1.....",
    "...1d668882 1...",
    "...17668882 31..",
    "...179999993 1..",
    "....1667661.....",
    ".....1aaa1......",
    "......111.......",
])
sprite("hero_ra", 0, [
    "....1111111.....",
    "...144444441....",
    "..14444444451...",
    "..15444444451...",
    "..15544422221...",
    "..15542222f21...",
    "...1542222221...",
    "....1122331.....",
    "...1dccccc1.....",
    "...1d66888811...",
    "...176688888221.",
    "...179999991221.",
    "....16666661....",
    "....17761a71....",
    "...1aa1.1aa1....",
    "....11...11.....",
])

# sword (palette 1): 1 outline 2 metal 3 shade 4 white 5 hilt 6 gold
sprite("sword_d", 1, [
    "......1661......",
    "......1551......",
    ".....166661.....",
    "......1421......",
    "......1421......",
    "......1421......",
    "......1421......",
    "......1421......",
    "......1421......",
    "......1421......",
    "......1421......",
    ".......14 1.....",
    ".......11.......",
    "................",
    "................",
    "................",
])
sprite("sword_r", 1, [
    "................",
    "................",
    "................",
    "................",
    "................",
    "..1.............",
    ".161111111111...",
    "15664444444441..",
    "15662222222221 .",
    ".161333333331...",
    "..1.111111111...",
    "................",
    "................",
    "................",
    "................",
    "................",
])

# blob (palette 2): 1 outline 2 dark 3 mid 4 light 5 hi 6 white 7 black
sprite("blob0", 2, [
    "................",
    "................",
    "................",
    "......1111......",
    "....11444411....",
    "...1445544431...",
    "..144554444331..",
    "..143444443331..",
    ".14366344663331.",
    ".14367344673321.",
    ".13333333333321.",
    ".13333333333221.",
    ".12333333332221.",
    "..122222222221..",
    "...1111111111...",
    "................",
])
sprite("blob1", 2, [
    "................",
    "................",
    "................",
    "................",
    "................",
    ".....111111.....",
    "...1144554411...",
    "..144554444331..",
    ".1436634466333 1",
    "1143673446733321",
    "1333333333333321",
    "1333333333332221",
    "1233333333322221",
    ".12222222222221.",
    "..111111111111..",
    "................",
])
# beetle (palette 3): 1 outline 2 dark 3 mid 4 light 5 hi 6 legs 7 eye
sprite("beetle_d0", 3, [
    "......1..1......",
    ".....16..61.....",
    "...1.111111.1...",
    "..161344443161..",
    "...1345544431...",
    "..16345543431 6.",
    "...1344343331...",
    "..61334343321 6.",
    "...1333323321...",
    "..6123323322161.",
    "...1223322221...",
    "....12222221....",
    "....11777711....",
    ".....1711 71....",
    "......1111......",
    "................",
])
sprite("beetle_d1", 3, [
    "......1..1......",
    "....161..161....",
    "...1.111111.1...",
    "...61344443161..",
    "...1345544431...",
    "..16345543431...",
    "...1344343331 6.",
    "..61334343321...",
    "...1333323321 6.",
    "...1233233221...",
    "..61223322221 6.",
    "....12222221....",
    "....11777711....",
    ".....171171.....",
    "......1111......",
    "................",
])
sprite("beetle_r0", 3, [
    "................",
    "................",
    "....6...6...6...",
    "....16..16..16..",
    "..11111111111...",
    ".1334444433211..",
    "1345554443321711",
    "1345544433321771",
    "1334444333221711",
    ".1223332222211..",
    "..11111111111...",
    "....16..16..16..",
    "....6...6...6...",
    "................",
    "................",
    "................",
])
sprite("beetle_r1", 3, [
    "................",
    "................",
    "...6...6...6....",
    "...16..16..16...",
    "..11111111111...",
    ".1334444433211..",
    "1345554443321711",
    "1345544433321771",
    "1334444333221711",
    ".1223332222211..",
    "..11111111111...",
    "...16..16..16...",
    "...6...6...6....",
    "................",
    "................",
    "................",
])
# wisp (palette 5): 1 white 2 light 3 cyan 4 blue 5 eye
sprite("wisp0", 5, [
    "................",
    "......4444......",
    "....43333334....",
    "...4322222234...",
    "..432211112234..",
    "..432111111234..",
    ".43215111151234.",
    ".43215111151234.",
    ".43211111111234.",
    ".43221111112234.",
    "..432221122234..",
    "..43322222334...",
    "...433223334....",
    "...4.43344.4....",
    "......4..4......",
    "................",
])
sprite("wisp1", 5, [
    "................",
    "................",
    "......4444......",
    "....43333334....",
    "...4322222234...",
    "..432211112234..",
    "..432111111234..",
    ".43215111151234.",
    ".43215111151234.",
    ".43211111111234.",
    ".43221111112234.",
    "..432221122234..",
    "..44332223344...",
    "..4.4334334.4...",
    ".....4.44.4.....",
    "................",
])
# items (palette 1): 8 gem 9 gem dark a gem light; b heart c dark d light
# 6 gold 7 gold dark; e puff f puff grey
sprite("gem", 1, [
    "................",
    "................",
    "......1111......",
    ".....1aa881.....",
    "....1aa88881....",
    "...1a8888889 1..",
    "...1a8888889 1..",
    "...1a8888889 1..",
    "...1a8888889 1..",
    "...18888889 91..",
    "....1888899 1...",
    ".....18899 1....",
    "......1991......",
    ".......11.......",
    "................",
    "................",
])
sprite("heart", 1, [
    "................",
    "................",
    "................",
    "...111...111....",
    "..1dbb1.1bbb1...",
    ".1ddbbb1bbbbc1..",
    ".1dbbbbbbbbbc1..",
    ".1bbbbbbbbbbc1..",
    "..1bbbbbbbbc1...",
    "...1bbbbbbc1....",
    "....1bbbbc1.....",
    ".....1bbc1......",
    "......1c1.......",
    ".......1........",
    "................",
    "................",
])
sprite("key", 1, [
    "................",
    "......1111......",
    ".....166661.....",
    "....16711761....",
    "....161..161....",
    "....16711761....",
    ".....167761.....",
    "......1671......",
    "......1671......",
    "......16711.....",
    "......166661....",
    "......16711.....",
    "......166661....",
    "......17711.....",
    ".......11.......",
    "................",
])
sprite("puff0", 1, [
    "................",
    "................",
    "................",
    "................",
    "................",
    "......eeee......",
    ".....eeffee.....",
    ".....efeefe.....",
    ".....efeefe.....",
    ".....eeffee.....",
    "......eeee......",
    "................",
    "................",
    "................",
    "................",
    "................",
])
sprite("puff1", 1, [
    "................",
    "................",
    "....ee....ee....",
    "...efffe..efe...",
    "...efeef.eeffe..",
    "....eee..effe...",
    "..........ee....",
    "......ee........",
    ".....efe........",
    "..ee..ee...eee..",
    ".effe.....effe..",
    ".eefe.....efee..",
    "..ee.......ee...",
    "................",
    "................",
    "................",
])
sprite("puff2", 1, [
    "..f..........f..",
    "................",
    "....f......f....",
    "................",
    "................",
    "f..............f",
    "................",
    "................",
    "................",
    "................",
    "f..............f",
    "................",
    "................",
    "....f......f....",
    "................",
    "..f..........f..",
])
sprite("spark", 1, [
    "................",
    "................",
    ".......4........",
    ".......4........",
    "......424.......",
    "..4...424...4...",
    "...44.424.44....",
    "....4442444.....",
    "..44422222444...",
    "....4442444.....",
    "...44.424.44....",
    "..4...424...4...",
    "......424.......",
    ".......4........",
    ".......4........",
    "................",
])
sprite("shadow", 4, [
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "....11111111....",
    "...1111111111...",
    "....11111111....",
])
sprite("solid", 4, ["1" * 16] * 16)
# the Sunstone (palette 1): gold and white glow
sprite("sunstone", 1, [
    ".......4........",
    "...4...4...4....",
    "....4..4..4.....",
    ".......1........",
    "......161.......",
    "4....16661....4.",
    ".4..1666661..4..",
    "...166466661....",
    "4441664446614444",
    "...166466671....",
    ".4..1667771..4..",
    "4....17771....4.",
    "......171.......",
    ".......1........",
    "....4..4..4.....",
    "...4...4...4....",
])

# TG16 canopy sprites: the tree tops' overlay pixels, drawn in front of the
# hero when he walks under a tree (palette 6 is a copy of BG palette 0).
SPR["canopy_l"] = (6, sub(CANOPY, 0, 0, 16, 16))
SPR["canopy_r"] = (6, sub(CANOPY, 16, 0, 16, 16))
SPRPAL.append(BGPAL[0])

# ------------------------------------------------------------------ font
FONT5 = {
    "A": ["01110", "10001", "10001", "11111", "10001", "10001", "10001"],
    "B": ["11110", "10001", "10001", "11110", "10001", "10001", "11110"],
    "C": ["01110", "10001", "10000", "10000", "10000", "10001", "01110"],
    "D": ["11110", "10001", "10001", "10001", "10001", "10001", "11110"],
    "E": ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
    "F": ["11111", "10000", "10000", "11110", "10000", "10000", "10000"],
    "G": ["01110", "10001", "10000", "10111", "10001", "10001", "01111"],
    "H": ["10001", "10001", "10001", "11111", "10001", "10001", "10001"],
    "I": ["01110", "00100", "00100", "00100", "00100", "00100", "01110"],
    "J": ["00111", "00010", "00010", "00010", "00010", "10010", "01100"],
    "K": ["10001", "10010", "10100", "11000", "10100", "10010", "10001"],
    "L": ["10000", "10000", "10000", "10000", "10000", "10000", "11111"],
    "M": ["10001", "11011", "10101", "10101", "10001", "10001", "10001"],
    "N": ["10001", "10001", "11001", "10101", "10011", "10001", "10001"],
    "O": ["01110", "10001", "10001", "10001", "10001", "10001", "01110"],
    "P": ["11110", "10001", "10001", "11110", "10000", "10000", "10000"],
    "Q": ["01110", "10001", "10001", "10001", "10101", "10010", "01101"],
    "R": ["11110", "10001", "10001", "11110", "10100", "10010", "10001"],
    "S": ["01111", "10000", "10000", "01110", "00001", "00001", "11110"],
    "T": ["11111", "00100", "00100", "00100", "00100", "00100", "00100"],
    "U": ["10001", "10001", "10001", "10001", "10001", "10001", "01110"],
    "V": ["10001", "10001", "10001", "10001", "10001", "01010", "00100"],
    "W": ["10001", "10001", "10001", "10101", "10101", "10101", "01010"],
    "X": ["10001", "10001", "01010", "00100", "01010", "10001", "10001"],
    "Y": ["10001", "10001", "01010", "00100", "00100", "00100", "00100"],
    "Z": ["11111", "00001", "00010", "00100", "01000", "10000", "11111"],
    "0": ["01110", "10001", "10011", "10101", "11001", "10001", "01110"],
    "1": ["00100", "01100", "00100", "00100", "00100", "00100", "01110"],
    "2": ["01110", "10001", "00001", "00010", "00100", "01000", "11111"],
    "3": ["11110", "00001", "00001", "01110", "00001", "00001", "11110"],
    "4": ["00010", "00110", "01010", "10010", "11111", "00010", "00010"],
    "5": ["11111", "10000", "11110", "00001", "00001", "10001", "01110"],
    "6": ["00110", "01000", "10000", "11110", "10001", "10001", "01110"],
    "7": ["11111", "00001", "00010", "00100", "01000", "01000", "01000"],
    "8": ["01110", "10001", "10001", "01110", "10001", "10001", "01110"],
    "9": ["01110", "10001", "10001", "01111", "00001", "00010", "01100"],
    ":": ["00000", "00100", "00100", "00000", "00100", "00100", "00000"],
    "!": ["00100", "00100", "00100", "00100", "00100", "00000", "00100"],
    ".": ["00000", "00000", "00000", "00000", "00000", "00000", "00100"],
    ",": ["00000", "00000", "00000", "00000", "00000", "00100", "01000"],
    "-": ["00000", "00000", "00000", "11111", "00000", "00000", "00000"],
    "'": ["00100", "00100", "01000", "00000", "00000", "00000", "00000"],
    "?": ["01110", "10001", "00001", "00010", "00100", "00000", "00100"],
    "x": ["00000", "00000", "10001", "01010", "00100", "01010", "10001"],
    "(": ["00010", "00100", "01000", "01000", "01000", "00100", "00010"],
    ")": ["01000", "00100", "00010", "00010", "00010", "00100", "01000"],
}

HUD = []          # list of (palette, 8x8)
HUDCH = {}        # char -> index

def hud_tile(key, pal, g):
    HUDCH[key] = len(HUD)
    HUD.append((pal, g))

hud_tile(" ", 0, grid(8, 8, 0))
for ch, rows in FONT5.items():
    g = grid(8, 8, 0)
    for y, r in enumerate(rows):
        for x, b in enumerate(r):
            if b == "1":
                g[y][x + 1] = 2
                if g[y + 1][x + 2] == 0:
                    g[y + 1][x + 2] = 3
    hud_tile(ch, 0, g)

def heart8(fill_left, fill_right):
    shape = ["01101100", "11111110", "11111110", "11111110", "01111100", "00111000", "00010000", "00000000"]
    g = grid(8, 8, 0)
    for y, r in enumerate(shape):
        for x, b in enumerate(r):
            if b == "1":
                full = fill_left if x < 4 else fill_right
                edge = (x == 0 or r[x - 1] == "0" or x == 7 or r[x + 1] == "0" or y == 0
                        or shape[y - 1][x] == "0" or shape[y + 1][x] == "0")
                g[y][x] = 2 if full else (3 if edge else 0)
    return g

hud_tile("heart_full", 1, heart8(True, True))
hud_tile("heart_half", 1, heart8(True, False))
hud_tile("heart_empty", 1, heart8(False, False))
hud_tile("gem", 2, from_ascii([
    "..3333..", ".332222.", "33222222", "32222222", ".222222.", "..2222..", "...22...", "........"]))
hud_tile("key", 3, from_ascii([
    ".2222...", "23..32..", ".2222...", "..22....", "..2222..", "..22....", "..222...", "........"]))
# frame pieces for the HUD box
hud_tile("bar", 0, from_ascii(["........", "........", "........", "33333333", "22222222", "33333333", "........", "........"]))

# ------------------------------------------------------------------ logo
def logo():
    text = "FERNWICK"
    s = 3
    w, h = 160, 32
    mask = grid(w, h, 0)
    x0 = (w - (len(text) * 6 * s - s)) // 2
    for i, ch in enumerate(text):
        rows = FONT5[ch]
        for y, r in enumerate(rows):
            for x, b in enumerate(r):
                if b == "1":
                    for dy in range(s):
                        for dx in range(s):
                            mask[4 + y * s + dy][x0 + i * 6 * s + x * s + dx] = 1
    g = grid(w, h, 0)
    for y in range(h):
        for x in range(w):
            if mask[y][x]:
                k = y - 4
                g[y][x] = 2 if k < 4 else 3 if k < 9 else 4 if k < 14 else 5 if k < 18 else 6
                # bevel: top-left edges bright
                if (y > 0 and not mask[y - 1][x]) or (x > 0 and not mask[y][x - 1]):
                    g[y][x] = 7 if k < 9 else 3
    # drop shadow then outline
    sh = [row[:] for row in g]
    for y in range(h):
        for x in range(w):
            if mask[y][x] and y + 2 < h and x + 2 < w and not mask[y + 2][x + 2]:
                sh[y + 2][x + 2] = 8
    g2 = outline(sh, 1)
    return g2

LOGO = logo()

# ----------------------------------------------------------------- crest
def crest():
    from PIL import Image, ImageDraw
    im = Image.new("P", (128, 128), 0)
    d = ImageDraw.Draw(im)
    # sun rays behind the shield
    import math
    for k in range(16):
        a = k * math.pi / 8
        a2 = a + 0.13
        r1 = 62 if k % 2 == 0 else 52
        pts = [(64, 60), (64 + r1 * math.cos(a - 0.13), 60 + r1 * math.sin(a - 0.13)),
               (64 + r1 * math.cos(a2), 60 + r1 * math.sin(a2))]
        d.polygon(pts, fill=3 if k % 2 == 0 else 13, outline=4)
    # shield
    shield = [(28, 20), (100, 20), (100, 70), (92, 92), (64, 116), (36, 92), (28, 70)]
    d.polygon(shield, fill=1)
    inner = [(32, 24), (96, 24), (96, 69), (89, 89), (64, 111), (39, 89), (32, 69)]
    d.polygon(inner, fill=6)
    d.polygon([(32, 64), (96, 64), (96, 69), (89, 89), (64, 111), (39, 89), (32, 69)], fill=9)
    d.polygon([(32, 24), (64, 24), (64, 64), (32, 64)], fill=7)
    d.polygon([(64, 64), (96, 64), (96, 69), (89, 89), (64, 111)], fill=8)
    d.line([(32, 64), (96, 64)], fill=2, width=2)
    d.line([(64, 24), (64, 111)], fill=2, width=2)
    # gold rim
    d.line(inner + [inner[0]], fill=3, width=2)
    # the sun in the middle
    d.ellipse([46, 46, 82, 82], fill=1)
    d.ellipse([48, 48, 80, 80], fill=12)
    d.ellipse([51, 51, 77, 77], fill=3)
    d.ellipse([54, 53, 72, 71], fill=13)
    d.ellipse([57, 55, 65, 63], fill=15)
    # leaves at the bottom
    for side in (-1, 1):
        for j in range(4):
            cx = 64 + side * (18 + j * 7)
            cy = 112 - j * 9
            d.ellipse([cx - 5, cy - 3, cx + 5, cy + 3], fill=9, outline=1)
    g = [[im.getpixel((x, y)) for x in range(128)] for y in range(128)]
    return g

CREST = crest()

# --------------------------------------------------------------- emit Lua

def tiles_of(g16):
    return [sub(g16, 0, 0, 8, 8), sub(g16, 8, 0, 8, 8), sub(g16, 0, 8, 8, 8), sub(g16, 8, 8, 8, 8)]

TILES = ["0" * 64]
TILE_INDEX = {"0" * 64: 0}

def tile_id(g8):
    h = flat_hex(g8)
    if h not in TILE_INDEX:
        TILE_INDEX[h] = len(TILES)
        TILES.append(h)
    return TILE_INDEX[h]

def lua_str_list(items, per_line=1):
    return ",\n".join('  "%s"' % s for s in items)

def emit():
    out = []
    out.append("-- 10_art.lua: Fernwick's art, generated by build_art.py. Do not edit;")
    out.append("-- change build_art.py and run it (it also copies this file to the SNES toy).")
    out.append("-- Pixels are hex colour indices (0 = transparent), palettes 8-bit RGB.")
    out.append("ART = {}")

    def pal_lua(name, pals):
        out.append("ART.%s = {" % name)
        for p in pals:
            out.append("  { " + ", ".join("0x" + c for c in p) + " },")
        out.append("}")

    pal_lua("bgpal", BGPAL)
    pal_lua("hudpal", HUDPAL)
    pal_lua("sprpal", SPRPAL)
    pal_lua("logopal", [LOGOPAL])
    pal_lua("crestpal", [CRESTPAL])

    blocks = []
    for ch, b in BLOCKS.items():
        pal, art = b[0], b[1]
        t = [tile_id(x) for x in tiles_of(art)]
        entry = "{ p = %d, t = { %s }" % (pal, ", ".join(str(i) for i in t))
        if len(b) > 2 and b[2] is not None:
            lo = [tile_id(x) for x in tiles_of(b[3])]
            hi = [tile_id(x) for x in tiles_of(b[2])]
            entry += ", lo = { %s }, hi = { %s }" % (", ".join(map(str, lo)), ", ".join(map(str, hi)))
        entry += " }"
        blocks.append('  ["%s"] = %s,' % (ch, entry))
    out.append("-- 8x8 background tiles; index 0 is blank.")
    out.append("ART.tiles = {")
    out.append(lua_str_list(TILES))
    out.append("}")
    out.append("-- 16x16 blocks by map character: p palette, t the four tiles (TL TR BL BR).")
    out.append("-- Tree tops also give lo (grass under) and hi (canopy over) for two layers.")
    out.append("ART.blocks = {")
    out.extend(blocks)
    out.append("}")

    out.append("-- HUD/font tiles (colours 0..3 of a HUD palette).")
    out.append("ART.hud = {")
    for pal, g in HUD:
        out.append('  { p = %d, px = "%s" },' % (pal, flat_hex(g)))
    out.append("}")
    out.append("ART.hudch = {")
    for k, v in HUDCH.items():
        key = k.replace("\\", "\\\\").replace('"', '\\"')
        out.append('  ["%s"] = %d,' % (key, v))
    out.append("}")

    out.append("-- 16x16 sprites.")
    out.append("ART.spr = {")
    for name, (pal, g) in SPR.items():
        out.append('  %s = { p = %d, px = "%s" },' % (name, pal, flat_hex(g)))
    out.append("}")

    def image(name, g):
        out.append("ART.%s = { w = %d, h = %d, rows = {" % (name, len(g[0]), len(g)))
        for row in g:
            out.append('  "%s",' % "".join("0123456789abcdef"[v] for v in row))
        out.append("} }")

    image("logo", LOGO)
    image("crest", CREST)
    return "\n".join(out) + "\n"

def preview(outdir):
    from PIL import Image
    os.makedirs(outdir, exist_ok=True)

    def rgb(p, i):
        return hexc(p[i])

    # blocks sheet
    keys = list(BLOCKS.keys())
    im = Image.new("RGB", (len(keys) * 18, 18), (40, 0, 40))
    for k, ch in enumerate(keys):
        b = BLOCKS[ch]
        for y in range(16):
            for x in range(16):
                v = b[1][y][x]
                im.putpixel((k * 18 + x, y), rgb(BGPAL[b[0]], v) if v else (255, 0, 255))
    im.resize((im.width * 4, im.height * 4), Image.NEAREST).save(os.path.join(outdir, "blocks.png"))
    names = list(SPR.keys())
    im = Image.new("RGB", (len(names) * 18, 18), (90, 160, 80))
    for k, n in enumerate(names):
        pal, g = SPR[n]
        for y in range(16):
            for x in range(16):
                if g[y][x]:
                    im.putpixel((k * 18 + x, y), rgb(SPRPAL[pal], g[y][x]))
    im.resize((im.width * 4, im.height * 4), Image.NEAREST).save(os.path.join(outdir, "sprites.png"))
    for name, g, p in (("logo", LOGO, LOGOPAL), ("crest", CREST, CRESTPAL)):
        im = Image.new("RGB", (len(g[0]), len(g)), (30, 30, 60))
        for y, row in enumerate(g):
            for x, v in enumerate(row):
                if v:
                    im.putpixel((x, y), rgb(p, v))
        im.resize((im.width * 3, im.height * 3), Image.NEAREST).save(os.path.join(outdir, name + ".png"))
    # rooms from 20_world.lua
    world = os.path.join(HERE, "20_world.lua")
    if os.path.exists(world):
        src = open(world).read()
        rooms = re.findall(r'rows = \{([^}]*)\}', src)
        for n, body in enumerate(rooms):
            rows = re.findall(r'"([^"]*)"', body)
            im = Image.new("RGB", (256, 192))
            for ry, r in enumerate(rows):
                for rx, ch in enumerate(r):
                    b = BLOCKS.get(ch)
                    if not b:
                        continue
                    for y in range(16):
                        for x in range(16):
                            im.putpixel((rx * 16 + x, ry * 16 + y), rgb(BGPAL[b[0]], b[1][y][x]))
            im.resize((512, 384), Image.NEAREST).save(os.path.join(outdir, "room%d.png" % (n + 1)))

def main():
    text = emit()
    with open(os.path.join(HERE, "10_art.lua"), "w") as f:
        f.write(text)
    os.makedirs(SNES_DIR, exist_ok=True)
    for name in SHARED:
        src = os.path.join(HERE, name)
        if os.path.exists(src):
            shutil.copyfile(src, os.path.join(SNES_DIR, name))
    print("tiles %d, sprites %d, hud %d" % (len(TILES), len(SPR), len(HUD)))
    if "--preview" in sys.argv:
        preview(sys.argv[sys.argv.index("--preview") + 1])

if __name__ == "__main__":
    main()
