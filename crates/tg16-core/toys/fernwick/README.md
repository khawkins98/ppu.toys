# Fernwick

A small top-down sword-and-key adventure for the TurboGrafx-16 / PC Engine
model in `tg16-core`, in the spirit of the 16-bit overhead adventure games.
The name, hero, enemies, map, art and music are original and all drawn or
written by code; there are no image assets. The same game also runs on the
SNES engine (`compare/snes/fernwick/`), and the viewer's `compare.html`
plays both side by side.

Six overworld screens and a cave. Cut bushes for gems, fight blobs, beetles
and wisps, find the key in the dark cave, open the shrine door and take the
Sunstone.

## Playing

```sh
ppu tg16 check crates/tg16-core/toys/fernwick --duration 30
ppu tg16 render crates/tg16-core/toys/fernwick --at 2,8,26 -o previews
ppu tg16 rom crates/tg16-core/toys/fernwick --duration 32 --loop -o fernwick.pce
```

In the browser viewers it is listed as `fernwick`.

| TG16 pad | SNES pad | Viewer key | Does |
| --- | --- | --- | --- |
| d-pad | d-pad | arrows | walk (diagonals slide round corners) |
| II or I | B or A | Z or X | swing the sword (cuts bushes) |
| Run | Start | Enter | start; after a game over or a win, play again |

Walk into the cave mouth in the north-west wood to go in; the stairs take
you out. Walk up into the shrine door holding the key to open it.

With nobody at the pad for five seconds the title starts an attract mode: an
autopilot (a breadth-first search over the room's blocks, then "stand beside
the nearest enemy and swing") plays for 27 seconds, through four rooms into
the cave to the key. Any button ends it. Because ROM export records the toy
with the pad released, that is what the HuCard shows: the title, then the
demo, looping (1920 frames, a 512 KB ROM).

## Files

| File | What is in it | Shared with the SNES? |
| --- | --- | --- |
| `build_art.py` | draws all the art and writes `10_art.lua`; copies the shared files to `compare/snes/fernwick/` | |
| `10_art.lua` | generated: palettes, 8x8 tiles, 16x16 blocks, sprites, font, logo, crest | yes, identical |
| `20_world.lua` | the rooms (16x12 blocks each), enemies, item places, music | yes, identical |
| `30_game.lua` | the game: hero, enemies, collision, transitions, autopilot, HUD text, music sequencer | yes, identical |
| `main.lua` | the TG16 renderer and sound | no |

### One game, two machines

The game logic is pure Lua with no machine calls. It reads the pad (both
engines name the buttons `up`, `down`, `left`, `right`, `a`, `b`, `start`)
and leaves its results in a table `G` for each machine's `main.lua` to draw:
a sprite list, HUD rows, map-drawing jobs, the camera, sound-effect names,
and effect state (iris radius, damage flash, dark room).

`10_art.lua`, `20_world.lua` and `30_game.lua` exist once as source, here.
`build_art.py` writes the art and copies all three into the SNES toy, and
`crates/ppu-cli/tests/fernwick.rs` fails if a copy differs. Copies rather
than a shared directory because each toy has to stay a self-contained
directory (that is what `ppu pack`, `ppu tg16 pack` and the viewers load);
the test keeps the copies from drifting.

The same test plays a scripted walk and fight on both engines and checks
that `quest_state()` (mode, room, hero position, hearts, gems, key, enemies,
camera) is identical on every frame. Map jobs are budgeted on the TG16 (a
room per frame of vertical blank) and instant on the SNES, so the game waits
a fixed two frames before a scroll on both, and the timing stays in step.

## How it is drawn on the TG16

One background layer, 64 sprites with 16 units per line, 512 colours and the
PSG. The map is 64x64 tiles:

| Map tiles | Holds |
| --- | --- |
| columns 0-31, rows 0-23 | the current room |
| columns 0-31, rows 24-27 | the HUD |
| columns 0-31, rows 28-55 | the title screen, laid out as it shows |
| columns 32-63 | the next room during a scroll |

- **HUD: a raster split.** BYR at the top of the frame shows the HUD rows on
  lines 0-31; a BYR write on line 32 starts the room (one raster line).
  While a room slides sideways, lines 0-31 also get BXR = 0 (32 more).
- **Room scrolls.** The next room is drawn into columns 32-63 (and, for up
  and down, the current one copied there first), BXR or BYR slides 4 pixels
  a frame, then the new room is redrawn at home: 768 VRAM words, which fit
  in one vertical blank.
- **Canopies over the hero.** The tree tops are in the map. When the hero
  walks under one, the canopy cells he touches are drawn again as front
  sprites, so only the 1-4 sprites that matter are spent on it.
- **Translucency.** Shadows and the wisps are drawn on alternate frames
  (`kit.flicker`).
- **Cave iris.** A box closing on the hero: the lines above and below it
  have both layers off, and solid 32x64 sprites cover the sides (up to 16
  units on a line, with the game frozen so nothing else needs them).
- **The dark cave.** The dungeon palette is dimmed for the frame, and raster
  palette writes brighten it on the lines around the hero, with a half step
  at each edge. Each step is spread over two lines, because a raster
  interrupt has time for about ten palette writes.
- **Damage flash, water, torches.** Palette rewrites in vertical blank; only
  changed entries are written.
- **Title.** The crest (16x16 tiles) unfolds: for 90 frames each line of
  the crest band picks a crest row by a vertical scale (a raw BYR write per
  line) and shifts by a decaying sine wave; then a glint wobble passes every
  four seconds. Backdrop writes every 16 lines paint the sky.
- **Sound.** Pulse lead, triangle bass and noise drums on channels 1, 2
  and 5; effects on 3 (tone) and 6 (noise).
- **CPU.** `cpu()` charges the game logic (a base cost, collision and path
  search work counted by `30_game.lua`, 500 cycles per enemy), building the
  sprite list and decoding map jobs.

### What it costs

From `ppu tg16 check --duration 40` (title, then the attract mode into the
cave):

| Meter | Peak | Limit |
| --- | --- | --- |
| CPU (estimate) | 48% of a frame (the frame that redraws a room at home) | 100% |
| Software cycles (`cpu()`) | about 44,000 | 119,318 per frame |
| Raster lines | 192 (iris) | 224 |
| Sprites | 20 | 64 |
| Busiest line | 16 units (iris masks) | 16, none dropped |
| VRAM words per frame | 896 (a room plus the HUD) | about 1400 in vertical blank |

`ppu tg16 rom --duration 32 --loop` makes a 512 KB HuCard; the busiest
vertical blank needs about 13,100 of 16,800 cycles and no raster event runs
past its line.

## TG16 against SNES, in this game

### Where the TG16 matches or does better

| | TG16 | SNES |
| --- | --- | --- |
| CPU time | 7.16 MHz HuC6280: the Lua logic, path search and map decoding are charged at under half a frame | 2.68-3.58 MHz 65816; the same work would be a larger share |
| Colours | 16 background + 16 sprite palettes: rooms, HUD, title, crest and logo each get their own | 8 + 8 palettes in 256 CGRAM entries; BG3's 2bpp HUD palettes overlap BG palettes 0-1, so rooms get 5 and the crest borrows palette 7 on the title |
| Big sprites | 32x64 sprites make the iris side masks with few entries | two sizes per frame (here 16x16 and 32x32) |
| Map size | the 64x64 map holds the room, HUD, title and scroll space at once, and 2048 addressable tiles | Mode 7 and the BG maps must share VRAM, so the crest is uploaded again each time the title starts |
| HUD | one raster line (32 more during a sideways scroll) | a whole layer (BG3) |
| Scrolling and gameplay | the same: both scroll 4 px a frame and play frame-for-frame identically | |

### Where it falls short

| | TG16 | SNES |
| --- | --- | --- |
| Layers | one: the HUD is a raster split; canopies are front sprites redrawn over the hero | BG1 ground, BG2 canopies (tile priority over the hero), BG3 HUD |
| Translucency | shadows and wisps flicker at 30 Hz | colour math half-adds them every frame |
| Lighting | the cave lights a band of whole lines around the hero | a circular window around the hero, darkened outside by colour subtraction |
| Iris | a box (masked lines + side sprites, 16 units on a line) | a true circle (window edges set per line by HDMA, clip to black) |
| Damage flash | palette rewrite | fixed-colour addition |
| Title | the crest can only stretch vertically and wave: no rotation, no horizontal scaling | Mode 7 spins and zooms the crest |
| Colour depth | 8 levels per channel: the sky is 14 bands | 32 levels: the HDMA sky gradient is smooth |
| Sound | six wavetable channels with 32-step waves; drums are noise | eight S-DSP voices with sampled instruments and drums |
| Sprites per line | 16 units: the iris masks fill a line | 32 sprites / 34 slivers per line; the busiest line here uses 6 sprites |
