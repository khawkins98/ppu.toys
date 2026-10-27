# Comet Lancer

An on-rails 3D shooter for the TurboGrafx-16 / PC Engine model in
`tg16-core`, in the style of the early-90s polygon flying games (Star Fox
on the SNES). The name, ship, enemies and art are original and are all
drawn by code; there are no image assets.

Fly over a checkerboard planet, go through gold rings to get shield back,
dodge the towers and shoot down Hornet fighters before they shoot you.

![Comet Lancer in the model](../../../../docs/tg16/comet-lancer.png)

## Playing

```sh
ppu tg16 check crates/tg16-core/toys/comet_lancer --duration 20
ppu tg16 render crates/tg16-core/toys/comet_lancer --at 2,6,10 -o previews
ppu tg16 pack crates/tg16-core/toys/comet_lancer
```

In the browser viewer it is listed as `comet_lancer`.

| Pad | Viewer key | Does |
| --- | --- | --- |
| d-pad | arrows | steer; up climbs |
| I | X | twin lasers (hold for auto fire) |
| II | Z | barrel roll: enemy shots miss while you roll |
| Run | Enter | start a game; after a game over any button plays again |

With nobody at the pad the ship flies itself (an attract mode), so
`render`, `check` and the ROM export show a game being played.

## Files

| File | What is in it |
| --- | --- |
| `10_raster.lua` | the software 3D renderer (transform, light, cull, sort, fill) and the scaled sprite sets |
| `20_models.lua` | the ship and Hornet models, palettes, and the pre-scaled art |
| `30_world.lua` | the background: HUD, sky, mountains, the perspective floor |
| `main.lua` | the game: course, ship, camera, collisions, autopilot, drawing, sound |

## How the 3D works

The SNES got polygon games from a Super FX chip in the cartridge. The TG16
has nothing like it: one background layer, 64 sprites, no scaling, no
rotation. So the toy uses three tricks, each one something a HuCard game
could do.

### 1. The player's ship: real polygons, filled by the CPU

The Lancer is 26 vertices and 17 faces. Every frame `R3.draw` in
`10_raster.lua`:

1. rotates the vertices by the ship's roll, pitch and yaw, plus a fixed
   look-down angle, and projects them with a small perspective divide;
2. lights each face from one direction and picks one of four shades of its
   material (hull, trim, fins), culls faces turned away, and sorts the rest
   far to near (the painter's algorithm);
3. fills each face as horizontal spans into a 64x64 pixel buffer.

The buffer is then sent to VRAM as two 32x64 sprites. Only the 16x16 cells
that have pixels this frame, or had them last frame, are uploaded, so a
level ship costs about 500 VRAM words and a barrel roll about 800, within
the ~1400 words that fit in vertical blank.

The core cannot see Lua doing math, so the toy charges the fill to the
budget meter itself with `cpu(cycles)`: about 150 cycles per vertex, 260
per face, 40 per span and 6 per pixel, plus clearing used cells. That comes
to 20,000 to 30,000 cycles, a sixth to a quarter of a frame.

### 2. Everything else: pre-scaled sprite sets

Hornets, towers, rings, laser bolts, enemy shots, explosions and the ship's
shadow are drawn once in `init()` at 4 to 10 sizes each and stored as
sprite patterns. The Hornet sizes are rendered by the same polygon code
(one level and one banked pose, mirrored for the other direction). Each
frame an object's position goes through a perspective divide and the set
picks the size nearest its on-screen size.

VRAM tricks a real game would use:

- **Rings** store only the top-left quarter; the other three are the same
  patterns drawn with the sprite flip bits.
- **Towers** store one side view and flip it so the side facing the camera
  shows.
- Large objects are several 32x64 sprites side by side.

Sprites are sorted nearest first, so near things cover far things. A line
holds 16 sprite units on a TG16; when a sprite would go over that on any of
its lines, the toy leaves the farthest ones out, instead of letting the
hardware drop whichever sprites come last.

### 3. The floor: a raster trick on one background layer

The TG16 can scroll each line sideways and pick which map row a line shows
(a `BYR` reload), but it cannot scale or rotate. So the map holds the
checkerboard pre-scaled: about 40 map rows, each a row of squares of one
width, from 2 pixels to 128.

For every line below the horizon, a raster hook works out how far away the
ground is on that line (camera height x focal length / lines below the
horizon), picks the map row whose square width matches, and scrolls it so
the squares line up with the camera's sideways position. Flying forward
changes which squares are dark: that is a shift by one square width. Far
rows use hazier palettes (14 steps toward the horizon colour).

Above the floor are a mountain band (one more `BYR` reload, scrolled slowly
for parallax), a sky gradient from backdrop colour writes every few lines,
and the HUD text in the map.

### What it costs

From `ppu tg16 check --duration 40`:

| Meter | Peak | Limit |
| --- | --- | --- |
| CPU (estimate) | about 50% of a frame | 100% |
| Software cycles (`cpu()`) | about 31,000 | 119,318 per frame |
| Raster lines | 200 | 224 |
| Sprites | 25 | 64 |
| Busiest line | 16 units | 16 |
| VRAM words per frame | about 780 | about 1400 in vertical blank |

It runs at 60 frames a second with room to spare. `ppu tg16 rom` turns 20
seconds of the attract mode into a 1 MB HuCard image that plays in
Mednafen; that ROM is a recording, not the game, because Lua does not run
on the console.

## Where the TG16 does better than the SNES here

- **CPU speed.** The HuC6280 runs at 7.16 MHz; the SNES's 65816 runs at
  2.68 to 3.58 MHz and spends more cycles per instruction on many
  operations. That is why a CPU-filled polygon ship at 60 fps is believable
  on a TG16 with no extra chip. An SNES without a Super FX would struggle to
  do the same.
- **Frame rate.** Star Fox drew its whole world in polygons and ran at
  roughly 15 to 20 frames a second. Comet Lancer draws far less in 3D, but
  everything moves at a smooth 60.
- **Big sprites and many sprite palettes.** TG16 sprites can be 32x64 and
  there are 16 sprite palettes of 15 colours, so large multi-colour objects
  (the close-up rings and towers, the 64x64 ship) are cheap to build. The
  SNES has 8 sprite palettes and two sprite sizes per screen.
- **Colours on screen.** 16 background and 16 sprite palettes give up to 482
  colours at once, against the SNES's 256-entry CGRAM. The haze steps, the
  sky bands and the separate palettes for every object type use that.

## Where it falls short

- **No polygon hardware.** Only the player's ship is truly 3D. The Hornets
  are pre-rendered: they bank between two fixed poses and never turn to show
  a new side. A Super FX game draws every object, and the ground features,
  as live polygons that can be seen from any angle.
- **No Mode 7 and no rotation.** The floor is a flat checkerboard that can
  only slide sideways and forward. When the ship banks, the horizon stays
  level; Star Fox tilts the whole world. The floor cannot turn, so the
  course is a straight line.
- **One background layer.** The sky, mountains, floor and HUD all share it,
  split by raster tricks. The SNES has two to four layers for the same job,
  with no raster time spent.
- **No colour math.** There is no transparency: the ship's shadow flickers
  on alternate frames instead of darkening the ground, and there are no
  translucent explosions or fades.
- **Fewer sprites per line.** 16 units per line (and 64 sprites in all)
  against the SNES's 32 sprites and 128 in all. Wide close-up objects use up
  a line quickly, which is why the toy drops far sprites on busy lines.
- **Only 512 colours to pick from.** Each channel has 8 levels, so the sky
  gradient and the haze show visible steps where the SNES's 32 levels per
  channel would look smooth.
- **Mid-line palette writes are late.** On real hardware a backdrop change
  from a raster interrupt lands a few pixels into its line, so the sky bands
  show a short notch at the left edge in Mednafen. The SNES changes colours
  with HDMA during horizontal blank.
