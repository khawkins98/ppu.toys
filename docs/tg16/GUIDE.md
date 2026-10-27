# Writing TurboGrafx-16 toys

`ppu tg16` toys are Lua, like SNES toys, but they drive a model of the PC
Engine / TurboGrafx-16: one background layer, 64 sprites, a 512-color
palette and a six-channel wavetable sound chip. Effects the SNES does in
hardware (more layers, Mode 7, color math, windows) are done here the way TG16
games did them, with raster tricks and sprites. The `kit` helpers package the
common ones, and `ppu tg16 check` tells you what they cost.

```sh
ppu tg16 new my-toy
ppu tg16 render my-toy --at 0,1,2 -o previews
ppu tg16 check my-toy --duration 10
ppu tg16 audio my-toy --duration 8 -o my-toy.wav
```

A toy is a `.lua` file, a directory of them (run in name order, `main.lua`
last) with PNGs in `assets/`, or a `.tg16.json` pack (see [Packing](#packing)). `init()` runs once; `frame(t, f)` runs every 60 Hz frame with the time in
seconds and the frame number.

## Video memory

VRAM is 32K 16-bit words shared by three things. Keep them apart: the engine
warns when one overwrites another.

| What | Where | Lua |
| --- | --- | --- |
| BAT (the background map) | words `0 .. bat.w*bat.h - 1` | `map(tx, ty, tile, pal)` |
| 8x8 tiles, 16 colors | words `tile*16 .. +15` | `tile(n, pixels)` |
| 16x16 sprite patterns | words `pattern*64 .. +63` | `pattern(n, pixels)` |

`pixels` is a string of hex digits (`"0123..."`, 64 or 256 of them; `.` means
0) or a table of numbers. Color 0 is transparent. `vram(addr, word)` writes a
raw word and `vram(addr)` reads one.

With a 64x32 BAT (2048 words) the first free tile is 128 and the first free
pattern is 32. Set the map size in `init()`: `bat.w` is 32, 64 or 128 tiles,
`bat.h` is 32 or 64.

## Palette

512 entries of 9-bit color: 16 background palettes (entries 0..255) and 16
sprite palettes (256..511), 16 colors each. Entry 0 is the backdrop, shown
wherever nothing else is.

- `rgb(r, g, b)` takes 0..255 channels; `rgb3(r, g, b)` takes the hardware's 0..7.
- `pal(p, i, c)` sets background palette `p` color `i`; `spal(p, i, c)` the sprite one.
- `color(entry, c)` writes any entry; `color(entry)` reads it.

There is no brightness register: fade by rewriting palettes.

## Background

- `bg.scroll.x`, `bg.scroll.y`: scroll the layer (wraps around the BAT).
- `show.bg`, `show.sprites`: the enable bits.
- `display.width` (256, up to 512 with the faster dot clocks) and
  `display.height` (224 or 240).

## Sprites

`spr(i, x, y, pattern, {pal, w, h, front, flip_x, flip_y})` places sprite
`i` (0..63) for this frame. The sprite table is cleared before each `frame()`.
`w` is 16 or 32, `h` is 16, 32 or 64; bigger sprites use neighbouring
patterns (a 32x32 sprite at pattern `n` uses `n`, `n+1`, `n+2`, `n+3` with the
low bits of `n` ignored). `front = false` puts the sprite behind opaque
background pixels. Lower `i` wins where sprites overlap.

A line holds 16 sprite units: a 16-wide sprite is one unit, a 32-wide sprite
two. Past that, sprites drop out on that line, as on the console.

## Scanline effects

`hdma(y0, y1, function(y) ... end)` runs the function before each line from
`y0` to `y1`, like a raster interrupt. Register it inside `frame()` every frame.
Inside it you can change `bg.scroll.x`, `bg.scroll.y`, `show.*` and the palette.

- Changes to `bg.scroll` and `show` last for that line only.
- Palette writes last from that line to the end of the frame, as on hardware,
  and the next frame starts from the palette `frame()` left.
- `bg.scroll.y = v` inside a hook means "this line shows map row `v + y`", the
  same offset meaning SNES toys use. Assign the raw `BYR` instead for the
  hardware behaviour: the line shows row `BYR`, and the lines below continue
  from there. That is how floors and stretches are drawn.
- Sprites cannot change mid-frame.

Each line a hook runs on costs a raster interrupt in the CPU estimate, so give
hooks the narrowest range that works.

## Sound

Six channels (1..6), each playing a 32-step waveform of 0..31 values.

- `wave(ch, "sine" | "square" | "pulse25" | "pulse12" | "saw" | "triangle")`, or
  a table of 32 numbers.
- `psg(ch, {on, note, hz, period, vol, left, right, noise, dda})` changes only
  the fields you give. `note` is a MIDI number or a name like `"A4"` or `"C#3"`.
  `vol` is 0..31 (1.5 dB steps), `left`/`right` 0..15 (3 dB steps).
- `noise = 0..31` turns channel 5 or 6 into noise at that rate; `noise = false`
  turns it back.
- `dda = 0..31` makes the channel output that level directly (sample playback);
  `dda = false` turns it back.
- `lfo(freq, mode)`: channel 2 wobbles channel 1's pitch; `mode` 1..3 sets the
  depth, 0 turns it off. Channel 2 is silent while the LFO runs.
- `master(left, right)`: master balance, 0..15.

Sound registers are read once per frame, after `frame()` returns.

## Images

Put PNGs in the toy directory's `assets/` folder and import them by file
stem: `assets/hills.png` is `"hills"`. Any PNG works (palette, gray, RGB,
RGBA). Colors are rounded to the 9-bit palette; pixels with alpha below 128
are transparent (color 0). An image that fits the hardware as it is (at most
15 colors per tile or sprite frame, few enough palettes and tiles) is imported
exactly. One that does not is reduced to fit, and the import says so in a
warning (see [Color reduction](#color-reduction)); pass `reduce = false` to
get an error instead that says what does not fit.

```lua
function init()
  bat.w = 64                       -- set the map size first
  local bg = image_bg("hills", { x = 0, y = 4 })
  -- bg.tiles, bg.palettes, bg.w, bg.h (in tiles), bg.tile, bg.pal,
  -- bg.reduced, bg.colors, bg.error, bg.merged_tiles
  hero = image_sprites("hero", { w = 32, h = 32 })
  -- hero.patterns, hero.palettes, hero.frames, hero.pattern, hero.step, hero.pal, hero.pals
end

function frame(t, f)
  local k = f // 8 % hero.frames   -- frame k, counted from 0
  spr(0, 100, 120, hero.pattern + k * hero.step, { w = 32, h = 32, pal = hero.pals[k + 1] })
end
```

`image_bg(name, opts)` cuts the image (a multiple of 8 pixels each way) into
8x8 tiles, gives each tile one of up to 16 palettes of 15 colors, stores
identical tiles once (the BAT has no flip bits, so only exact repeats share),
and writes the tiles to VRAM, the palettes to the VCE and the map into the
BAT. Options, all optional:

| Option | Default | Meaning |
| --- | --- | --- |
| `x`, `y` | 0 | map cell of the image's top-left corner (wraps around the BAT) |
| `tile` | first free run above the BAT | first tile number |
| `pal` | lowest background palette nothing has set yet | first palette |
| `pals` | `16 - pal` | most palettes it may use |
| `reduce` | `true` | reduce colors and merge tiles when the image does not fit; `false` makes that an error |
| `dither` | `"ordered"` | how reduced colors are drawn: `"ordered"`, `"fs"` or `"none"` |

The image must fit in `bat.w` x `bat.h` tiles. Besides the counts and
placement, the result has `reduced` (true when anything was lost), `colors`
(distinct 9-bit colors in the image), `error` (mean color error, 0 when exact)
and `merged_tiles`.

`image_sprites(name, opts)` cuts a sheet into frames of `w` x `h` pixels
(default 16x16; `w` 16 or 32, `h` 16, 32 or 64), read left to right, top to
bottom, and writes each frame's 16x16 patterns in the order a sprite of that
size reads them. Frame `k` starts at pattern `pattern + k * step`. All cells
of a frame share one palette; `pals[k + 1]` is frame `k`'s palette (with one
palette they are all `pal`). Options: `pattern` (default: the first free run
above the BAT, aligned for the frame size), `pal` (lowest sprite palette not
yet set), `pals` (default `16 - pal`), `w`, `h`, `reduce`, `dither`. The
result has `reduced`, `colors` and `error` like `image_bg`'s.

Defaults never overlap each other or anything `tile()`, `pattern()` and
`map()` wrote earlier, and palettes that `pal()`/`spal()` set are skipped.
Explicit numbers that overlap get the usual VRAM or palette warning. Imports
in `init()` are free in the budget meter; in `frame()` they count as VRAM
uploads.

### Color reduction

When an image has more colors than its tiles can show, the importer:

1. dithers the image to the full 512-color space (with the chosen `dither`),
   so every pixel has the 9-bit color it would show if any color were
   allowed;
2. groups the tiles (or sprite frames) into at most `pals` palettes and picks
   15 colors for each: each new palette is made for the tile drawn worst so
   far, then tiles move to the palette that draws them best and palettes are
   rebuilt from their tiles (median cut, refined by k-means) until nothing
   changes;
3. redraws every pixel with its tile's palette and the same dither;
4. for backgrounds, if more unique tiles remain than fit in VRAM (the free run
   at `tile`, up to tile 2047), merges the most similar ones: a tile's cells
   switch to the tile that draws them with the least error. Tiles whose
   transparent pixels differ never merge.

| `dither` | Looks | VRAM |
| --- | --- | --- |
| `"ordered"` (default) | an 8x8 Bayer pattern; the classic console look | flat areas still make identical tiles, because the pattern is aligned to the image |
| `"fs"` | Floyd–Steinberg error diffusion: smoothest gradients, grainier | noisy tiles rarely repeat, so it needs more tiles (and merges sooner) |
| `"none"` | nearest color: flat areas stay flat, gradients band | fewest tiles |

Sprite frames are separate sprites, so diffused error never crosses from one
frame into the next.

The result is deterministic: the same PNG and options always give the same
tiles and palettes. Each reduced import adds a warning, which `ppu tg16 check`
and `render` print:

```text
warning: image_bg("lake"): its 65 colors did not fit 16 palettes of 15, so they were reduced (ordered dither); mean color error 1.6 (8-bit RGB). Pass reduce = false to make this an error.
```

`error` is the mean distance, in 8-bit RGB units (0..441), between what each
pixel shows and what the dithered image of step 1 wanted, both averaged over
3x3 pixels so dither noise that evens out on screen does not count. Under 3 is
hard to tell apart from the full-palette dither; 5 to 10 shows as blotches or
color shifts in places; much more means the image needs more palettes or
fewer colors. "Distinct colors" counts 9-bit colors, which is why a photo with
thousands of colors may report a few dozen: the problem is usually that a
tile has more than 15 of them, or that tiles need too many different
palettes, not the total.

`crates/tg16-core/toys/photo/` is an example: a painted 512x224 panorama
(about 24,000 colors) imported with ordered dither and scrolled with raster
parallax.

## Kit helpers

All are plain Lua in `crates/tg16-core/src/kit.lua`; read it to see the trick.

| Helper | SNES feature it stands in for | How |
| --- | --- | --- |
| `kit.parallax(cam_x, {{y0, y1, speed}, ...})` | several scrolling layers | per-band horizontal scroll; `{y0, y1, s0, s1}` ramps the speed |
| `kit.sprite_layer{first, count, pattern, w, h, y, x, spacing, pal, front}` | an extra layer | a wrapping row of sprites; returns the next free sprite |
| `kit.floor{horizon, bottom, z, x, height, curve, row0, rows}` | Mode 7 floors and roads | a `BYR` reload and a sideways shift on every floor line |
| `kit.flicker(f, phase)`, `kit.flicker_spr(f, i, x, y, pattern, opts)` | color-math transparency | draw on alternate frames |
| `kit.mask_lines(y0, y1, color)` | windows (horizontal) | hide the layers on those lines and flatten the backdrop |
| `kit.mask_columns(first, x, width, y0, y1, pattern, pal)` | windows (vertical) | solid front sprites |

## The budget meter

`ppu tg16 check` plays the toy and reports, for its busiest frame:

- **CPU (estimate)**: cycles a hand-written game would spend on the same work
  (raster interrupts, VDC and palette writes, sprite setup, VRAM uploads, sound
  writes) as a share of the 119,318 cycles in a frame. It is an estimate: no
  HuC6280 code runs.
- **Raster lines** with a hook.
- **Sprites** used and the busiest line's units.
- **VRAM words** written during `frame()` (uploads in `init()` are free); about
  1400 fit in vertical blank.

It warns when a frame goes over, and fails on dropped sprites unless you pass
`--allow-overflow`.

## Packing

`ppu tg16 pack my-toy` writes the whole toy to one file, `my-toy.tg16.json`
(`-o` picks another name); `ppu tg16 unpack my-toy.tg16.json dir` writes it
back out. `render`, `check` and `audio` accept a pack wherever they accept a
directory, and the browser viewer loads the same file.

```json
{
  "format": "ppu.toys/tg16",
  "version": 1,
  "name": "my-toy",
  "files": { "main.lua": "<lua source text>", "helpers.lua": "..." },
  "assets": { "hills.png": "<base64 of the PNG bytes>" }
}
```

- `files` holds every `*.lua` at the top of the toy directory. They run in
  name order with `main.lua` last, whatever order the keys are in.
- `assets` holds every `*.png` in `assets/`, base64 encoded (standard
  alphabet, with padding). It may be omitted when empty.
- Names must be plain file names (no directories, `..` or separators) ending
  in `.lua` or `.png`; a pack that breaks this is rejected before anything is
  written. Packs are limited to 32 MiB and 256 files and 256 assets.
- Readers ignore unknown top-level keys and reject another `format` or a
  `version` newer than they know.

## Not modelled yet

SuperGrafx, CD audio, the HuC6280 CPU itself, ROM export, color reduction or
dithering in the image importers, the interrupt timing of mid-line writes, and
the exact first-line BYR offset.
