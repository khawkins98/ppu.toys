# TG16 toy viewer

A static page that plays a TurboGrafx-16 toy in the browser: the `tg16-core`
crate built to WebAssembly, plus `index.html` and `viewer.js`. No bundler, no
CDNs.

## Build

```sh
rustup target add wasm32-unknown-unknown
# Must match the wasm-bindgen version in Cargo.lock (build.sh checks).
cargo install wasm-bindgen-cli --version 0.2.126
viewer/tg16/build.sh
```

`build.sh` compiles `tg16-core` with the `bindings` feature for
`wasm32-unknown-unknown`, runs `wasm-bindgen --target web` into
`viewer/tg16/pkg/` (git-ignored), runs `wasm-opt` if it is installed, and packs
`crates/tg16-core/toys/sunset.lua` as `pkg/demo.tg16.json`, the demo the page
loads by default. `PROFILE=dev viewer/tg16/build.sh` makes a debug build.

## Serve

ES modules and `fetch` need http, not `file://`:

```sh
cd viewer/tg16
python3 -m http.server 8000
# open http://localhost:8000/
```

Sound uses an AudioWorklet, which browsers allow on `localhost` and https; on
other plain-http origins the page falls back to a ScriptProcessor.

## Use

- Drop a `.tg16.json` pack (from `ppu tg16 pack`) onto the page, or use
  **Open…**. A single `.lua` file, or several `.lua` and `.png` files together,
  also work: Lua files run sorted by name with `main.lua` last, and each PNG is
  added as an image named after its file stem (`hills.png` -> `hills`).
- `?toy=path/to/pack.tg16.json` loads a pack from the server instead of the demo.
- Joypad: arrow keys, X = I, Z = II, Enter = Run, Shift = Select (the SNES
  viewer's keys, with I and II where SNES A and B are).
- **Sound on** starts audio (browsers need a click first). **Pause**, **Step**
  and **Restart** (keys: space, `.`, `r`).
- The side panel shows the budget meter for the last frame (CPU estimate,
  raster lines, sprites of 64, peak sprite units on one line of 16, VRAM words
  written), its problems, engine warnings, and Lua errors. A Lua error stops the
  toy; fix it and reopen, or press Restart.

## SNES vs TG16 (`compare.html`)

`compare.html` (+ `compare.js`) plays one toy on both machines side by side:
`ppu-core` (the SNES engine) on the left, `tg16-core` on the right, both at
the same integer scale. One clock drives both: every frame each engine gets
the same `t` and `f` and the same controller, so **Pause**, **Step** and
**Restart** (space, `.`, `r`) act on both. **Sound on** plays one machine at a
time; the **SNES | TG16** switch picks which. Under each screen are that
machine's numbers: for the TG16 the budget meter; for the SNES what
`PpuCore` reports (busiest line's sprites of 32 and slivers of 34, with range
and time overflow, S-DSP voices sounding, Mode 7 lines, the echo buffer's
share of sound RAM, and VRAM claimed by `dma()`).

The picker has four groups:

- **Close ports**: the TG16 example toys and their hand-written SNES ports in
  `compare/snes/<name>/` (sunset, kit_demo, photo). Each port does the same
  thing the SNES way: real BG layers, Mode 7, colour math, windows, the
  S-DSP. The page lists what each side uses.
- **Auto-converted SNES examples**: the five SNES examples in
  `plugin/skills/creating-demos/examples/` on the left and their
  `ppu tg16 convert` output on the right, with the converter's `CONVERT.md`
  notes below.
- **TG16 only**: any example toy with no `compare/snes/<name>` port yet.
- **TG16 strengths (and limits)**: pairs written to test where the TG16 is
  said to beat the SNES (CPU speed, palettes, per-line colour), each side
  done the best way its hardware allows; `strengths.js` holds their captions
  and the extra rows under each screen (each SNES toy's own estimate from
  `sram.meter`, and a live count of unique colours). Two of the four show
  where the claim does not hold up.

`?toy=<name>` opens a toy directly. `build.sh` makes everything the page
loads, none of it committed:

| File | From |
| --- | --- |
| `pkg/snes/ppu_core.js`, `ppu_core_bg.wasm` | `ppu-core` for wasm32 with its default `bindings` feature, through `wasm-bindgen` |
| `pkg/snes/<name>.ppu.json` | `ppu pack` of `compare/snes/<name>/` and of each SNES example |
| `pkg/<name>.tg16.json` | the TG16 example toys (as before) |
| `pkg/convert/<name>.tg16.json`, `<name>.md` | `ppu tg16 convert` of each SNES example (into a temp dir), packed, and its report |
| `pkg/compare.json` | the TG16-only toy list |

`build.sh` builds the host `ppu` CLI in release mode for the packing and the
conversions (about a minute and a half more than before).

## JS API

`pkg/tg16_core.js` exports `Tg16`:

| Method | |
| --- | --- |
| `new Tg16()` | |
| `add_image(name, w, h, rgba)` | queue an RGBA8 image for the next `load()` |
| `clear_images()` | |
| `load(filesJson)` | JSON array of `[name, source]` in load order; throws an `Error` with `file`, `line`, `luaMessage` |
| `set_pad(mask)` | buttons held for the next frames: bits up, down, left, right, I, II, select, run |
| `frame(t, f)` | run and render one frame; throws like `load` |
| `width()`, `height()` | size of the last frame |
| `pixels()` | copy of the RGBA8 frame |
| `pixels_ptr()`, `pixels_len()` | zero-copy view: `new Uint8ClampedArray(memory.buffer, ptr, len)` |
| `audio()` | `Int16Array`, interleaved stereo at 44100 Hz, ~735 sample frames |
| `budget_json()`, `warnings_json()` | JSON strings |

## GitHub Pages

`.github/workflows/tg16-pages.yml` builds this folder and publishes it on every
push to `tg16-core` (turn on Settings → Pages → Source: "GitHub Actions").
Every toy in `crates/tg16-core/toys/` is packed into `pkg/`: single-file toys
(`toys/<name>.lua`) and toy directories (`toys/<name>/` with `main.lua`, other
`.lua` files and `assets/*.png`, packed with their images). So
`?toy=pkg/kit_demo.tg16.json` opens the kit demo and `?toy=pkg/photo.tg16.json`
the color-reduced photo. The workflow also rebuilds on changes to
`crates/ppu-core`, `crates/ppu-cli`, `compare/` and the SNES examples, which
`compare.html` uses.
