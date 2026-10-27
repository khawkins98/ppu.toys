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
- **Sound on** starts audio (browsers need a click first). **Pause**, **Step**
  and **Restart** (keys: space, `.`, `r`).
- The side panel shows the budget meter for the last frame (CPU estimate,
  raster lines, sprites of 64, peak sprite units on one line of 16, VRAM words
  written), its problems, engine warnings, and Lua errors. A Lua error stops the
  toy; fix it and reopen, or press Restart.

## JS API

`pkg/tg16_core.js` exports `Tg16`:

| Method | |
| --- | --- |
| `new Tg16()` | |
| `add_image(name, w, h, rgba)` | queue an RGBA8 image for the next `load()` |
| `clear_images()` | |
| `load(filesJson)` | JSON array of `[name, source]` in load order; throws an `Error` with `file`, `line`, `luaMessage` |
| `frame(t, f)` | run and render one frame; throws like `load` |
| `width()`, `height()` | size of the last frame |
| `pixels()` | copy of the RGBA8 frame |
| `pixels_ptr()`, `pixels_len()` | zero-copy view: `new Uint8ClampedArray(memory.buffer, ptr, len)` |
| `audio()` | `Int16Array`, interleaved stereo at 44100 Hz, ~735 sample frames |
| `budget_json()`, `warnings_json()` | JSON strings |
