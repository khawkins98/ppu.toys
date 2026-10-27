//! WASM/JS bindings for the browser viewer (`viewer/tg16/`). Compiled only for
//! wasm32 with the `bindings` feature. A thin wrapper over [`Engine`]: it keeps
//! the last rendered frame so JS can read pixels without copying, and returns
//! the budget and warnings as JSON strings.
use wasm_bindgen::prelude::*;

use crate::{Engine, LuaError, HEIGHT, WIDTH};

/// An image added before `load()`, waiting for the engine to take it.
struct PendingImage {
    name: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[wasm_bindgen]
pub struct Tg16 {
    engine: Engine,
    images: Vec<PendingImage>,
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

impl Default for Tg16 {
    fn default() -> Self {
        Self::new()
    }
}

/// A Lua error as a JS `Error` whose message is `file:line: message`, with
/// `file`, `line` and `luaMessage` properties for the viewer.
fn lua_error(e: LuaError) -> JsValue {
    let err = JsError::new(&e.to_string());
    let v: JsValue = err.into();
    let set = |k: &str, val: JsValue| {
        let _ = js_sys::Reflect::set(&v, &JsValue::from_str(k), &val);
    };
    set("luaMessage", JsValue::from_str(&e.message));
    set(
        "file",
        e.file.as_deref().map_or(JsValue::NULL, JsValue::from_str),
    );
    set(
        "line",
        e.line
            .map_or(JsValue::NULL, |l| JsValue::from_f64(l as f64)),
    );
    v
}

#[wasm_bindgen]
impl Tg16 {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Tg16 {
        Tg16 {
            engine: Engine::new(),
            images: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            rgba: vec![0; WIDTH * HEIGHT * 4],
        }
    }

    /// Queue an RGBA8 image (`width * height * 4` bytes) under `name` (the
    /// asset's file stem) for the next `load()`.
    pub fn add_image(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), JsValue> {
        if rgba.len() != width as usize * height as usize * 4 {
            return Err(JsError::new(&format!(
                "image {name}: expected {}x{}x4 bytes, got {}",
                width,
                height,
                rgba.len()
            ))
            .into());
        }
        self.images.retain(|i| i.name != name);
        self.images.push(PendingImage {
            name: name.to_string(),
            width,
            height,
            rgba,
        });
        Ok(())
    }

    /// Forget images queued with `add_image`.
    pub fn clear_images(&mut self) {
        self.images.clear();
    }

    /// Load a toy. `files_json` is a JSON array of `[name, source]` pairs in
    /// load order (names sorted, `main.lua` last). Resets the machine; queued
    /// images are kept so a restart can reload the same toy.
    pub fn load(&mut self, files_json: &str) -> Result<(), JsValue> {
        let files: Vec<(String, String)> = serde_json::from_str(files_json)
            .map_err(|e| JsError::new(&format!("files_json: {e}")))?;
        let pairs: Vec<(&str, &str)> = files
            .iter()
            .map(|(n, s)| (n.as_str(), s.as_str()))
            .collect();
        // A fresh engine so images cleared with clear_images() are gone too.
        self.engine = Engine::new();
        for i in &self.images {
            self.engine
                .add_image(&i.name, i.width, i.height, i.rgba.clone());
        }
        self.engine.load(&pairs).map_err(lua_error)?;
        self.width = WIDTH;
        self.height = HEIGHT;
        self.rgba.iter_mut().for_each(|b| *b = 0);
        Ok(())
    }

    /// Run `frame(t, f)` and render it. On a Lua error the last good picture
    /// is kept and the error is thrown.
    /// Buttons held for the next frame: bits up, down, left, right, I, II,
    /// select, run (bit 0 = up).
    pub fn set_pad(&mut self, mask: u16) {
        self.engine.set_pad(mask);
    }

    pub fn frame(&mut self, t: f64, f: u32) -> Result<(), JsValue> {
        let out = self.engine.render(t, f).map_err(lua_error)?;
        self.width = out.width;
        self.height = out.height;
        self.rgba = out.rgba;
        Ok(())
    }

    pub fn width(&self) -> u32 {
        self.width as u32
    }

    pub fn height(&self) -> u32 {
        self.height as u32
    }

    /// A copy of the last frame, RGBA8 row-major (`width * height * 4`).
    pub fn pixels(&self) -> Vec<u8> {
        self.rgba.clone()
    }

    /// Byte offset of the last frame in wasm memory, for a zero-copy view:
    /// `new Uint8ClampedArray(memory.buffer, pixels_ptr(), pixels_len())`.
    /// Valid until the next `frame()` or `load()`.
    pub fn pixels_ptr(&self) -> *const u8 {
        self.rgba.as_ptr()
    }

    pub fn pixels_len(&self) -> usize {
        self.rgba.len()
    }

    /// The last frame's sound: interleaved stereo i16 at 44100 Hz, about 735
    /// sample frames (1470 values).
    pub fn audio(&self) -> Vec<i16> {
        self.engine.audio().to_vec()
    }

    /// The cost meter for the last frame, as JSON.
    pub fn budget_json(&self) -> String {
        let b = self.engine.budget();
        serde_json::json!({
            "cycles": b.cycles,
            "cyclesPerFrame": crate::CYCLES_PER_FRAME,
            "cpu": b.cpu_share(),
            "rasterLines": b.raster_lines,
            "vdcWrites": b.vdc_writes,
            "paletteWrites": b.palette_writes,
            "vramWords": b.vram_words,
            "sprites": b.sprites,
            "peakLineUnits": b.peak_line_units,
            "droppedLines": b.dropped_lines,
            "problems": b.problems(),
        })
        .to_string()
    }

    /// Warnings collected since `load()` (JSON array of strings).
    pub fn warnings_json(&self) -> String {
        serde_json::to_string(&self.engine.warnings()).unwrap_or_else(|_| "[]".into())
    }
}
