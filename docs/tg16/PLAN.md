# TurboGrafx-16 port of ppu.toys: build plan

Goal: a `tg16` target that feels like ppu.toys (write Lua, render PNGs, check,
pack, play in a browser) but whose core is strictly the PC Engine / TurboGrafx-16
video and sound hardware. SNES-only tricks (parallax layers, Mode 7, color math,
windows) are not in the core. They come back as Lua kit helpers that fake them the
way real TG16 games did, with a meter that shows CPU and sprite cost.

## What carries over and what is replaced

| Reused as-is or lightly adapted | Replaced |
| --- | --- |
| piccolo Lua VM (`vendor/piccolo`) | SNES PPU (`bg.rs`, `mode7.rs`, `sprite.rs`, `compositor.rs`, `window.rs`) |
| `init()` / `frame(t, f)` / `hdma(y)` hook model | SNES register model (`registers.rs`, `linetable.rs`) |
| Kit helpers: `ramp`, `lerp`, `ease`, `rgb`, `hsl` | S-DSP + BRR (`dsp/`) |
| Pack format (`.ppu.json`, plus a `target` field) | Importers emit TG16 tile/BAT/sprite formats |
| `ppu` CLI: new, render, check, pack, docs | Docs rewritten for VDC/VCE/PSG |
| WASM shim pattern (`wasm.rs`) | |
| Golden-image test harness | |

The Studio web app is not in this repository, so browser play needs either the
LioraLabs team or a small standalone viewer. That is the last phase.

## Architecture

A new crate, `crates/tg16-core`, sits next to `ppu-core` and depends on nothing
SNES-specific. Nothing in `ppu-core` changes, so the SNES side cannot regress.

- `vdc.rs`: HuC6270 model. 64 KB VRAM (32K words), BAT, 4bpp planar tiles,
  64-entry SAT, MWR virtual screen sizes (32/64/128 x 32/64 tiles), BXR/BYR
  scroll with the hardware's BYR reload rule, CR background/sprite enables,
  the 16-sprites-per-line limit and overflow flag.
- `vce.rs`: HuC6260 model. 512 entries of 9-bit GRB color (256 BG, 256 sprite),
  entry 0 as backdrop, 3-bit-to-8-bit conversion, dot-clock widths
  (256 / 336 / 512 wide) and 224 or 240 lines.
- `render.rs`: per-scanline renderer that takes VRAM + SAT + VCE + a per-line
  register table and returns RGBA plus per-line stats (sprite count, dropped
  sprites).
- `psg.rs` (phase 3): six wavetable channels (32 x 5-bit samples), noise on
  channels 5 and 6, LFO (channel 2 modulates channel 1), DDA sample playback,
  per-channel and master balance.
- `lua.rs` (phase 2): the friendly Lua surface, built on piccolo the same way
  `ppu-core` does it.

## Lua surface (draft)

```lua
function init()
  display.width = 256           -- 256, 336 or 512; height 224 or 240
  bat.size = {64, 32}           -- MWR virtual screen, in tiles
  vce.bg[0][0] = rgb(16, 16, 48) -- backdrop
  local hills = load("hills")   -- importer output: tiles, BAT and palettes
end

function frame(t, f)
  bg.scroll.x = t * 30
  spr[0] = {x = 120, y = 100, pattern = 4, pal = 0, w = 32, h = 32, front = true}
  hdma(160, 223, function(y)      -- line-by-line, like a TG16 raster IRQ
    bg.scroll.x = t * 90           -- faster near the bottom: one-layer parallax
  end)
end
```

`hdma()` stays as the name (with `raster()` as an alias) so existing toys read
the same. On the TG16 it stands for a raster-compare interrupt each line.

## Kit helpers (SNES-style effects, honestly faked)

Each helper reports its cost to a budget meter: sprites per line (limit 16),
sprite cells per frame (64), and an estimate of HuC6280 cycles per frame spent
in raster interrupts and VRAM updates.

- `kit.parallax(bands)`: split-screen scroll bands via per-line BXR.
- `kit.sprite_layer(...)`: a second "background" made of sprites.
- `kit.soft_mode7(...)`: pre-scaled tile rows plus per-line scroll, the way
  TG16 racers did it. Costs a lot of the budget, and the meter says so.
- `kit.flicker_alpha(...)`: transparency by alternating frames.
- `kit.mask(...)`: window-like masks using sprite edges or palette swaps.

## Phases

1. **Core video renderer** (this branch, started): VDC + VCE model, renderer,
   golden-image tests. About 1 week.
2. **Lua bindings and CLI**: `ppu new --target tg16`, `render`, `check`,
   importer from PNG to tiles/BAT/sprites/palettes. About 1 to 1.5 weeks.
3. **PSG audio**: wavetable channels, noise, LFO, DDA, a simple song format
   reusing the existing score/timeline ideas. About 1 week.
4. **Kit helpers and budget meter**, plus docs. About 1 week.
5. **WASM build and a minimal browser viewer** (or hand off to the Studio).
   About 0.5 to 1 week.

Total: 4 to 5 weeks, in line with the earlier estimate.

## Out of scope for now

SuperGrafx (second VDC and priority mixer), CD-ROM audio, HuC6280 CPU emulation,
and ROM export. The budget meter estimates CPU cost; it does not run 6502 code.
ROM export (like the SNES panel has) is a follow-up once the core is stable.
