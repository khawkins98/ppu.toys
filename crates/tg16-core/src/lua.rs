//! Lua front end: runs a toy's `init()` and `frame(t, f)` against the TG16
//! core, collects its `hdma()` hooks, and produces a [`Frame`] to render.
//!
//! The Lua surface is small on purpose: VRAM, palette and sprite writes go
//! straight to native callbacks, and the friendly register names live in
//! `prelude.lua`.

use std::cell::RefCell;
use std::rc::Rc;

use piccolo::{
    Callback, CallbackReturn, Closure, Executor, Function, Lua, PrototypeError, StashedFunction,
    StaticError, Table, Value,
};

use crate::import::{import_bg_with, import_sprites_with, Dither, Image, Quality};
use crate::psg::{period_for_hz, preset_wave, Psg, CHANNELS, SAMPLES_PER_FRAME};
use crate::render::{render, Frame, LineRegs, Output, MAX_HEIGHT, MAX_WIDTH};
use crate::vce::{grb, rgb8, Vce};
use crate::vdc::{BatEntry, BatSize, Sprite, Vdc, SPRITE_COUNT, VRAM_WORDS};

const PRELUDE: &str = include_str!("prelude.lua");
const KIT: &str = include_str!("kit.lua");

/// A Lua compile or runtime error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaError {
    pub message: String,
    pub line: Option<u32>,
    pub file: Option<String>,
}

impl std::fmt::Display for LuaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.file, self.line) {
            (Some(file), Some(line)) => write!(f, "{file}:{line}: {}", self.message),
            (Some(file), None) => write!(f, "{file}: {}", self.message),
            _ => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for LuaError {}

fn static_error(e: StaticError, file: Option<&str>) -> LuaError {
    let line = if let StaticError::Runtime(rt) = &e {
        rt.downcast::<PrototypeError>().and_then(|pe| match pe {
            PrototypeError::Parser(p) => Some(p.line_number.0 as u32 + 1),
            _ => None,
        })
    } else {
        None
    };
    LuaError {
        message: e.to_string(),
        line,
        file: file.map(str::to_string),
    }
}

/// Machine state the native callbacks write into.
struct State {
    vdc: Vdc,
    /// Palette at the top of each frame. Writes from `init()`, top-level
    /// code and `frame()` land here and persist.
    vce: Vce,
    /// Set while a hook runs: palette writes go to this line instead.
    line: Option<usize>,
    line_writes: Vec<Vec<(u16, u16)>>,
    /// What last wrote each VRAM word, to catch tiles, sprite patterns and
    /// the BAT stepping on each other (they share one 64 KB VRAM).
    owner: Vec<Owner>,
    warnings: Vec<String>,
    psg: Psg,
    /// True while `load()` runs, so setup VRAM writes are not charged.
    loading: bool,
    counts: crate::budget::Counts,
    /// Images registered with `Engine::add_image`; they survive `load()`.
    images: std::collections::BTreeMap<String, Image>,
    /// Background and sprite palettes something has written colors 1..15 of,
    /// one bit each, so image imports pick free ones and warn on clashes.
    bg_pals_used: u16,
    spr_pals_used: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owner {
    None,
    Map,
    Tile(u16),
    Pattern(u16),
}

impl Owner {
    fn describe(self) -> String {
        match self {
            Owner::None => "nothing".into(),
            Owner::Map => "the BAT (map)".into(),
            Owner::Tile(n) => format!("tile {n}"),
            Owner::Pattern(n) => format!("sprite pattern {n}"),
        }
    }
}

impl State {
    /// Record that `who` wrote `words`, warning once per pair of owners when
    /// it overwrites something else.
    fn claim(&mut self, words: std::ops::Range<usize>, who: Owner) {
        const MAX_WARNINGS: usize = 20;
        for w in words {
            let w = w & (VRAM_WORDS - 1);
            let prev = self.owner[w];
            self.owner[w] = who;
            let clash = match (prev, who) {
                (Owner::None, _) => false,
                (Owner::Map, Owner::Map) => false,
                (a, b) => a != b,
            };
            if clash && self.warnings.len() < MAX_WARNINGS {
                let msg = format!(
                    "VRAM word {w:#06x}: {} overwrites {}. Tiles start at word tile*16, \
                     patterns at pattern*64, and the BAT fills words 0..bat.w*bat.h.",
                    who.describe(),
                    prev.describe()
                );
                if !self.warnings.contains(&msg) {
                    self.warnings.push(msg);
                }
                // One warning per write is plenty.
                return;
            }
        }
    }

    fn warn(&mut self, msg: String) {
        if self.warnings.len() < 20 && !self.warnings.contains(&msg) {
            self.warnings.push(msg);
        }
    }

    /// Lowest `align`-aligned run of `count` units of `unit` words, starting
    /// at or after word `start`, that nothing has written yet.
    fn find_free(&self, start: usize, unit: usize, count: usize, align: usize) -> Option<usize> {
        let mut u = start.div_ceil(unit).next_multiple_of(align);
        while (u + count) * unit <= VRAM_WORDS {
            let words = u * unit..(u + count) * unit;
            match self.owner[words.clone()]
                .iter()
                .rposition(|o| *o != Owner::None)
            {
                None => return Some(u),
                // Skip past the last used word in the run.
                Some(i) => u = (words.start + i + 1).div_ceil(unit).next_multiple_of(align),
            }
        }
        None
    }

    fn image(&self, func: &str, name: &str) -> Result<&Image, String> {
        let img = self.images.get(name).ok_or_else(|| {
            if self.images.is_empty() {
                format!(
                    "{func}: no image named {name:?}; no images are loaded (put PNGs in the \
                     toy's assets/ directory)"
                )
            } else {
                let known: Vec<_> = self.images.keys().map(|k| format!("{k:?}")).collect();
                format!(
                    "{func}: no image named {name:?}; known images: {}",
                    known.join(", ")
                )
            }
        })?;
        if img.rgba.len() != img.width as usize * img.height as usize * 4 {
            return Err(format!(
                "{func}({name:?}): {}x{} image has {} RGBA bytes, expected {}",
                img.width,
                img.height,
                img.rgba.len(),
                img.width as usize * img.height as usize * 4
            ));
        }
        Ok(img)
    }

    /// Write `palettes` (colors 1..15 of each) at palette `first` of the
    /// background (`base` 0) or sprite (`base` 256) half of the VCE.
    fn write_palettes(&mut self, func: &str, base: usize, first: usize, palettes: &[[u16; 16]]) {
        let sprite = base != 0;
        for (i, p) in palettes.iter().enumerate() {
            let n = first + i;
            let used = if sprite {
                &mut self.spr_pals_used
            } else {
                &mut self.bg_pals_used
            };
            let was_used = *used & (1 << n) != 0;
            *used |= 1 << n;
            if was_used {
                self.warn(format!(
                    "{func}: overwrites {} palette {n}, which was already set; pass pal = N to \
                     use another",
                    if sprite { "sprite" } else { "background" }
                ));
            }
            for (c, &color) in p.iter().enumerate().skip(1) {
                self.vce.set(base + n * 16 + c, color);
            }
            self.counts.palette_writes += 15;
        }
    }
}

/// Integer option `key` of an optional table.
fn opt<'gc>(
    ctx: piccolo::Context<'gc>,
    o: Option<Table<'gc>>,
    key: &'static str,
) -> Result<Option<i64>, String> {
    let Some(t) = o else { return Ok(None) };
    match t.get(ctx, key) {
        Value::Nil => Ok(None),
        v => int(v)
            .map(Some)
            .ok_or_else(|| format!("option {key} must be a number")),
    }
}

/// The `reduce` and `dither` options of the image functions: `None` when the
/// import must be exact (`reduce = false`), otherwise the dither to reduce
/// with (default ordered).
fn reduce_opts<'gc>(
    ctx: piccolo::Context<'gc>,
    o: Option<Table<'gc>>,
) -> Result<Option<Dither>, String> {
    let Some(t) = o else {
        return Ok(Some(Dither::default()));
    };
    let reduce = match t.get(ctx, "reduce") {
        Value::Nil | Value::Boolean(true) => true,
        Value::Boolean(false) => false,
        _ => return Err("option reduce must be true or false".into()),
    };
    let dither = match t.get(ctx, "dither") {
        Value::Nil => Dither::default(),
        Value::Boolean(false) => Dither::None,
        Value::String(s) => {
            let s = s.to_str_lossy();
            Dither::parse(&s).ok_or_else(|| {
                format!("option dither must be \"none\", \"ordered\" or \"fs\", not {s:?}")
            })?
        }
        _ => return Err("option dither must be \"none\", \"ordered\" or \"fs\"".into()),
    };
    Ok(reduce.then_some(dither))
}

/// Hint added to exact-import errors when the toy turned reduction off.
const STRICT_HINT: &str = " (reduce = false is set; without it the importer reduces colors to fit)";

/// The warning for an import that had to reduce, or `None` when exact.
fn reduce_warning(func: &str, q: &Quality, max_palettes: i64, dither: Dither) -> Option<String> {
    if !q.reduced() {
        return None;
    }
    let mut parts = Vec::new();
    if q.colors_reduced {
        parts.push(format!(
            "its {} colors did not fit {max_palettes} palette{} of 15, so they were reduced \
             ({} dither)",
            q.colors,
            if max_palettes == 1 { "" } else { "s" },
            dither.name()
        ));
    }
    if q.merged_tiles > 0 {
        parts.push(format!(
            "{} similar tile{} merged to fit VRAM",
            q.merged_tiles,
            if q.merged_tiles == 1 {
                " was"
            } else {
                "s were"
            }
        ));
    }
    Some(format!(
        "{func}: {}; mean color error {:.1} (8-bit RGB). Pass reduce = false to make this \
         an error.",
        parts.join("; "),
        q.error
    ))
}

/// Set the quality fields of an import's result table.
fn set_quality<'gc>(ctx: piccolo::Context<'gc>, out: Table<'gc>, q: &Quality, bg: bool) {
    out.set(ctx, "reduced", q.reduced()).unwrap();
    out.set(ctx, "colors", q.colors as i64).unwrap();
    // Two decimals keep the number stable to print and compare.
    out.set(ctx, "error", (q.error * 100.0).round() / 100.0)
        .unwrap();
    if bg {
        out.set(ctx, "merged_tiles", q.merged_tiles as i64).unwrap();
    }
}

/// `(name, opts)` arguments of the image functions.
fn image_args<'gc>(
    ctx: piccolo::Context<'gc>,
    stack: &piccolo::Stack<'gc, '_>,
    func: &str,
) -> Result<(String, Option<Table<'gc>>), piccolo::Error<'gc>> {
    let Value::String(name) = stack.get(0) else {
        return Err(err(
            ctx,
            &format!("{func}(name, opts): name must be a string"),
        ));
    };
    let opts = match stack.get(1) {
        Value::Nil => None,
        Value::Table(t) => Some(t),
        _ => {
            return Err(err(
                ctx,
                &format!("{func}(name, opts): opts must be a table"),
            ))
        }
    };
    Ok((name.to_str_lossy().into_owned(), opts))
}

/// Lowest palette not marked in `used`, or 0 when all are.
fn free_palette(used: u16) -> i64 {
    (0..16).find(|p| used & (1 << p) == 0).unwrap_or(0)
}

const MAX_TILES: usize = VRAM_WORDS / 16;
const MAX_PATTERNS: usize = VRAM_WORDS / 64;

/// `image_bg(name, {tile, pal, pals, x, y})`.
fn image_bg_call<'gc>(
    ctx: piccolo::Context<'gc>,
    st: &RefCell<State>,
    name: &str,
    o: Option<Table<'gc>>,
) -> Result<Table<'gc>, String> {
    let size = bat_size(ctx);
    let mut s = st.borrow_mut();
    let func = format!("image_bg({name:?})");
    let img = s.image("image_bg", name)?;
    let e = |m: String| format!("{func}: {m}");
    let tile = opt(ctx, o, "tile").map_err(e)?;
    let pal = match opt(ctx, o, "pal").map_err(e)? {
        Some(p) => p,
        None => free_palette(s.bg_pals_used),
    };
    if !(0..16).contains(&pal) {
        return Err(e("pal must be 0..15".into()));
    }
    let pals = opt(ctx, o, "pals").map_err(e)?.unwrap_or(16 - pal);
    if !(1..=16 - pal).contains(&pals) {
        return Err(e(format!("pals must be 1..{} when pal is {pal}", 16 - pal)));
    }
    let x = opt(ctx, o, "x").map_err(e)?.unwrap_or(0);
    let y = opt(ctx, o, "y").map_err(e)?.unwrap_or(0);
    let max_tiles = match tile {
        Some(t) if !(0..MAX_TILES as i64).contains(&t) => {
            return Err(e(format!("tile must be 0..{}", MAX_TILES - 1)))
        }
        Some(t) => MAX_TILES - t as usize,
        None => MAX_TILES - size.first_free_tile() as usize,
    };
    let reduce = reduce_opts(ctx, o).map_err(e)?;
    let imp = import_bg_with(img, pals as usize, max_tiles, reduce).map_err(|m| {
        e(format!(
            "{m}{}",
            if reduce.is_none() { STRICT_HINT } else { "" }
        ))
    })?;
    if imp.width > size.width as usize || imp.height > size.height as usize {
        return Err(e(format!(
            "image is {}x{} tiles but the BAT is {}x{}; set bat.w / bat.h (32, 64 or 128 by \
             32 or 64) before importing",
            imp.width, imp.height, size.width, size.height
        )));
    }
    let n = imp.tiles.len();
    let first = match tile {
        Some(t) => t as usize,
        None => s.find_free(size.words(), 16, n, 1).ok_or_else(|| {
            e(format!(
                "no free VRAM run for {n} tiles above the {}-word BAT; pass tile = N",
                size.words()
            ))
        })?,
    };
    for (i, px) in imp.tiles.iter().enumerate() {
        let t = first + i;
        s.claim(t * 16..t * 16 + 16, Owner::Tile(t as u16));
        s.vdc.write_tile(t as u16, px);
    }
    s.write_palettes(&func, 0, pal as usize, &imp.palettes);
    for (i, &(t, p)) in imp.map.iter().enumerate() {
        let tx = (x + (i % imp.width) as i64).rem_euclid(size.width as i64) as usize;
        let ty = (y + (i / imp.width) as i64).rem_euclid(size.height as i64) as usize;
        let w = ty * size.width as usize + tx;
        s.claim(w..w + 1, Owner::Map);
        let entry = BatEntry {
            tile: (first + t as usize) as u16,
            palette: pal as u8 + p,
        };
        s.vdc.set_bat(size, tx as u16, ty as u16, entry);
    }
    if !s.loading {
        s.counts.vram_words += (n * 16 + imp.map.len()) as u32;
    }
    let out = Table::new(&ctx);
    let set = |k: &'static str, v: usize| out.set(ctx, k, v as i64).unwrap();
    set("tiles", n);
    set("palettes", imp.palettes.len());
    set("w", imp.width);
    set("h", imp.height);
    set("tile", first);
    set("pal", pal as usize);
    set_quality(ctx, out, &imp.quality, true);
    if let Some(w) = reduce_warning(&func, &imp.quality, pals, reduce.unwrap_or_default()) {
        s.warn(w);
    }
    Ok(out)
}

/// `image_sprites(name, {pattern, pal, pals, w, h})`.
fn image_sprites_call<'gc>(
    ctx: piccolo::Context<'gc>,
    st: &RefCell<State>,
    name: &str,
    o: Option<Table<'gc>>,
) -> Result<Table<'gc>, String> {
    let size = bat_size(ctx);
    let mut s = st.borrow_mut();
    let func = format!("image_sprites({name:?})");
    let img = s.image("image_sprites", name)?;
    let e = |m: String| format!("{func}: {m}");
    let pattern = opt(ctx, o, "pattern").map_err(e)?;
    let pal = match opt(ctx, o, "pal").map_err(e)? {
        Some(p) => p,
        None => free_palette(s.spr_pals_used),
    };
    if !(0..16).contains(&pal) {
        return Err(e("pal must be 0..15".into()));
    }
    let pals = opt(ctx, o, "pals").map_err(e)?.unwrap_or(16 - pal);
    if !(1..=16 - pal).contains(&pals) {
        return Err(e(format!("pals must be 1..{} when pal is {pal}", 16 - pal)));
    }
    let fw = opt(ctx, o, "w").map_err(e)?.unwrap_or(16);
    let fh = opt(ctx, o, "h").map_err(e)?.unwrap_or(16);
    let reduce = reduce_opts(ctx, o).map_err(e)?;
    let imp = import_sprites_with(
        img,
        fw.max(0) as u32,
        fh.max(0) as u32,
        pals as usize,
        reduce,
    )
    .map_err(|m| {
        e(format!(
            "{m}{}",
            if reduce.is_none() { STRICT_HINT } else { "" }
        ))
    })?;
    let n = imp.patterns.len();
    let first = match pattern {
        Some(p) => {
            if p < 0 || p as usize + n > MAX_PATTERNS {
                return Err(e(format!(
                    "{n} patterns from pattern {p} do not fit; patterns are 0..{}",
                    MAX_PATTERNS - 1
                )));
            }
            if !(p as usize).is_multiple_of(imp.cells) {
                return Err(e(format!(
                    "{fw}x{fh} sprites need a pattern number that is a multiple of {}",
                    imp.cells
                )));
            }
            p as usize
        }
        None => s.find_free(size.words(), 64, n, imp.cells).ok_or_else(|| {
            e(format!(
                "no free VRAM run for {n} sprite patterns above the {}-word BAT; pass \
                     pattern = N",
                size.words()
            ))
        })?,
    };
    for (i, px) in imp.patterns.iter().enumerate() {
        let p = first + i;
        s.claim(p * 64..p * 64 + 64, Owner::Pattern(p as u16));
        s.vdc.write_pattern(p as u16, px);
    }
    s.write_palettes(&func, 256, pal as usize, &imp.palettes);
    if !s.loading {
        s.counts.vram_words += (n * 64) as u32;
    }
    let out = Table::new(&ctx);
    let set = |k: &'static str, v: usize| out.set(ctx, k, v as i64).unwrap();
    set("patterns", n);
    set("palettes", imp.palettes.len());
    set("frames", imp.frames());
    set("pattern", first);
    set("step", imp.cells);
    set("pal", pal as usize);
    let frame_pals = Table::new(&ctx);
    for (i, &p) in imp.frame_palettes.iter().enumerate() {
        frame_pals.set(ctx, i as i64 + 1, pal + p as i64).unwrap();
    }
    out.set(ctx, "pals", frame_pals).unwrap();
    set_quality(ctx, out, &imp.quality, false);
    if let Some(w) = reduce_warning(&func, &imp.quality, pals, reduce.unwrap_or_default()) {
        s.warn(w);
    }
    Ok(out)
}

/// A loaded toy.
pub struct Engine {
    lua: Lua,
    state: Rc<RefCell<State>>,
    init_fn: Option<StashedFunction>,
    frame_fn: Option<StashedFunction>,
    frame_file: Option<String>,
    audio: Vec<i16>,
    /// Lines where a hook ran in the last frame.
    touched: Vec<bool>,
    budget: crate::budget::Budget,
    /// Fractional output samples carried between frames.
    audio_carry: f64,
}

/// MIDI note number from a number or a name like "A4", "C#3", "Bb2".
fn note_number(v: Value<'_>) -> Option<f64> {
    match v {
        Value::Integer(i) => Some(i as f64),
        Value::Number(n) => Some(n),
        Value::String(s) => {
            let b = s.as_bytes();
            let base = match b.first()?.to_ascii_uppercase() {
                b'C' => 0,
                b'D' => 2,
                b'E' => 4,
                b'F' => 5,
                b'G' => 7,
                b'A' => 9,
                b'B' => 11,
                _ => return None,
            };
            let (shift, rest) = match b.get(1) {
                Some(b'#') => (1, &b[2..]),
                Some(b'b') => (-1, &b[2..]),
                _ => (0, &b[1..]),
            };
            let octave: i32 = std::str::from_utf8(rest).ok()?.parse().ok()?;
            Some(((octave + 1) * 12 + base + shift) as f64)
        }
        _ => None,
    }
}

fn int(v: Value<'_>) -> Option<i64> {
    match v {
        Value::Integer(i) => Some(i),
        Value::Number(f) if f.is_finite() => Some(f.floor() as i64),
        _ => None,
    }
}

fn truthy(v: Value<'_>) -> bool {
    !matches!(v, Value::Nil | Value::Boolean(false))
}

fn err<'gc>(ctx: piccolo::Context<'gc>, msg: &str) -> piccolo::Error<'gc> {
    Value::String(ctx.intern(msg.as_bytes())).into()
}

/// Read `n` color indices from a hex string ("0123...") or a 1-based table.
fn pixels<'gc>(ctx: piccolo::Context<'gc>, v: Value<'gc>, n: usize) -> Result<Vec<u8>, String> {
    match v {
        Value::String(s) => {
            let digits: Vec<u8> = s
                .as_bytes()
                .iter()
                .filter(|b| !b.is_ascii_whitespace())
                .map(|&b| match b {
                    b'0'..=b'9' => Ok(b - b'0'),
                    b'a'..=b'f' => Ok(b - b'a' + 10),
                    b'A'..=b'F' => Ok(b - b'A' + 10),
                    b'.' => Ok(0),
                    _ => Err(format!("bad pixel character '{}'", b as char)),
                })
                .collect::<Result<_, _>>()?;
            if digits.len() != n {
                return Err(format!("expected {n} pixels, got {}", digits.len()));
            }
            Ok(digits)
        }
        Value::Table(t) => (1..=n as i64)
            .map(|i| int(t.get(ctx, i)).map(|c| (c & 15) as u8).ok_or(()))
            .collect::<Result<_, _>>()
            .map_err(|_| format!("expected a table of {n} numbers")),
        _ => Err("pixels must be a hex string or a table".into()),
    }
}

/// A PSG channel argument, 1..6 in Lua, 0..5 here.
fn channel<'gc>(ctx: piccolo::Context<'gc>, v: Value<'gc>) -> Result<usize, piccolo::Error<'gc>> {
    match int(v) {
        Some(c) if (1..=CHANNELS as i64).contains(&c) => Ok(c as usize - 1),
        _ => Err(err(ctx, "PSG channel must be 1..6")),
    }
}

fn bat_size(ctx: piccolo::Context<'_>) -> BatSize {
    if let Value::Table(b) = ctx.get_global("bat") {
        let w = int(b.get(ctx, "w")).unwrap_or(32) as u16;
        let h = int(b.get(ctx, "h")).unwrap_or(32) as u16;
        BatSize::new(w, h).unwrap_or_default()
    } else {
        BatSize::default()
    }
}

fn install<'gc>(ctx: piccolo::Context<'gc>, state: &Rc<RefCell<State>>) {
    let set = |name: &'static str, f: Callback<'gc>| {
        ctx.set_global(name, f).unwrap();
    };
    // piccolo's base library has no _G; the prelude needs it.
    ctx.set_global("_G", ctx.globals()).unwrap();

    // vram(addr, word): raw VRAM write. vram(addr) reads.
    let st = state.clone();
    set(
        "vram",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let addr = int(stack.get(0)).ok_or_else(|| err(ctx, "vram(addr, word)"))? as usize;
            let addr = addr & (VRAM_WORDS - 1);
            match int(stack.get(1)) {
                Some(w) => {
                    let mut s = st.borrow_mut();
                    s.vdc.vram[addr] = w as u16;
                    if !s.loading {
                        s.counts.vram_words += 1;
                    }
                    stack.clear();
                }
                None => {
                    let w = st.borrow().vdc.vram[addr];
                    stack.replace(ctx, w as i64);
                }
            }
            Ok(CallbackReturn::Return)
        }),
    );

    // tile(n, pixels): write an 8x8 4bpp tile.
    let st = state.clone();
    set(
        "tile",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let n = int(stack.get(0)).ok_or_else(|| err(ctx, "tile(n, pixels)"))?;
            let px = pixels(ctx, stack.get(1), 64).map_err(|m| err(ctx, &format!("tile: {m}")))?;
            let mut s = st.borrow_mut();
            let base = (n as usize & 0x7ff) * 16;
            s.claim(base..base + 16, Owner::Tile(n as u16 & 0x7ff));
            if !s.loading {
                s.counts.vram_words += 16;
            }
            s.vdc.write_tile(n as u16, &px.try_into().unwrap());
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // pattern(n, pixels): write a 16x16 sprite pattern.
    let st = state.clone();
    set(
        "pattern",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let n = int(stack.get(0)).ok_or_else(|| err(ctx, "pattern(n, pixels)"))?;
            let px =
                pixels(ctx, stack.get(1), 256).map_err(|m| err(ctx, &format!("pattern: {m}")))?;
            let mut s = st.borrow_mut();
            let base = (n as usize & 0x3ff) * 64;
            s.claim(base..base + 64, Owner::Pattern(n as u16 & 0x3ff));
            if !s.loading {
                s.counts.vram_words += 64;
            }
            s.vdc.write_pattern(n as u16, &px.try_into().unwrap());
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // map(tx, ty, tile, pal): write one BAT cell, using bat.w / bat.h.
    let st = state.clone();
    set(
        "map",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let a: Vec<_> = (0..4).map(|i| int(stack.get(i))).collect();
            let [Some(tx), Some(ty), Some(tile)] = [a[0], a[1], a[2]] else {
                return Err(err(ctx, "map(tx, ty, tile, pal)"));
            };
            let e = BatEntry {
                tile: tile as u16,
                palette: a[3].unwrap_or(0) as u8,
            };
            let size = bat_size(ctx);
            let (tx, ty) = (
                tx.rem_euclid(size.width as i64) as usize,
                ty.rem_euclid(size.height as i64) as usize,
            );
            let mut s = st.borrow_mut();
            let w = ty * size.width as usize + tx;
            s.claim(w..w + 1, Owner::Map);
            if !s.loading {
                s.counts.vram_words += 1;
            }
            s.vdc.set_bat(size, tx as u16, ty as u16, e);
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // color(entry, value): VCE write. Inside hdma() it applies from that line
    // to the end of the frame; elsewhere it persists.
    let st = state.clone();
    set(
        "color",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let Some(i) = int(stack.get(0)) else {
                return Err(err(ctx, "color(entry, value)"));
            };
            let mut s = st.borrow_mut();
            let Some(c) = int(stack.get(1)) else {
                // color(entry) reads the frame's starting value.
                let v = s.vce.colors[(i & 511) as usize];
                stack.replace(ctx, v as i64);
                return Ok(CallbackReturn::Return);
            };
            s.counts.palette_writes += 1;
            match s.line {
                Some(y) => s.line_writes[y].push(((i & 511) as u16, (c & 0x1ff) as u16)),
                None => {
                    s.vce.set(i as usize, c as u16);
                    let e = (i & 511) as usize;
                    if e & 15 != 0 {
                        let bit = 1 << ((e >> 4) & 15);
                        if e < 256 {
                            s.bg_pals_used |= bit;
                        } else {
                            s.spr_pals_used |= bit;
                        }
                    }
                }
            }
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // spr(i, x, y, pattern, {pal, w, h, front, flip_x, flip_y})
    let st = state.clone();
    set(
        "spr",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let usage = "spr(i, x, y, pattern, opts)";
            let i = int(stack.get(0)).ok_or_else(|| err(ctx, usage))?;
            if !(0..SPRITE_COUNT as i64).contains(&i) {
                return Err(err(ctx, "spr: index must be 0..63"));
            }
            let x = int(stack.get(1)).ok_or_else(|| err(ctx, usage))?;
            let y = int(stack.get(2)).ok_or_else(|| err(ctx, usage))?;
            let pattern = int(stack.get(3)).ok_or_else(|| err(ctx, usage))?;
            let mut s = Sprite {
                x: x as i32,
                y: y as i32,
                pattern: pattern as u16,
                width: 16,
                height: 16,
                front: true,
                ..Sprite::default()
            };
            if let Value::Table(o) = stack.get(4) {
                s.palette = int(o.get(ctx, "pal")).unwrap_or(0) as u8 & 15;
                s.width = match int(o.get(ctx, "w")).unwrap_or(16) {
                    16 => 16,
                    32 => 32,
                    _ => return Err(err(ctx, "spr: w must be 16 or 32")),
                };
                s.height = match int(o.get(ctx, "h")).unwrap_or(16) {
                    16 => 16,
                    32 => 32,
                    64 => 64,
                    _ => return Err(err(ctx, "spr: h must be 16, 32 or 64")),
                };
                if !matches!(o.get(ctx, "front"), Value::Nil) {
                    s.front = truthy(o.get(ctx, "front"));
                }
                s.flip_x = truthy(o.get(ctx, "flip_x"));
                s.flip_y = truthy(o.get(ctx, "flip_y"));
            }
            let mut st = st.borrow_mut();
            st.vdc.set_sprite(i as usize, s);
            st.counts.sprites += 1;
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // rgb(r, g, b) with 0..255 channels; grb3(r, g, b) with 0..7 channels.
    set(
        "rgb",
        Callback::from_fn(&ctx, |ctx, _, mut stack| {
            let ch = |v: Value<'_>| int(v).unwrap_or(0).clamp(0, 255) as u8;
            let c = rgb8(ch(stack.get(0)), ch(stack.get(1)), ch(stack.get(2)));
            stack.replace(ctx, c as i64);
            Ok(CallbackReturn::Return)
        }),
    );
    set(
        "rgb3",
        Callback::from_fn(&ctx, |ctx, _, mut stack| {
            let ch = |v: Value<'_>| int(v).unwrap_or(0).clamp(0, 7) as u8;
            let c = grb(ch(stack.get(0)), ch(stack.get(1)), ch(stack.get(2)));
            stack.replace(ctx, c as i64);
            Ok(CallbackReturn::Return)
        }),
    );

    // wave(ch, preset_or_table): load a 32-step waveform into channel 1..6.
    let st = state.clone();
    set(
        "wave",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let ch = channel(ctx, stack.get(0))?;
            let w: [u8; 32] = match stack.get(1) {
                Value::String(name) => {
                    let name = name.to_str_lossy();
                    preset_wave(&name).ok_or_else(|| {
                        err(ctx, &format!("wave: unknown preset {name:?} (sine, square, pulse25, pulse12, saw, triangle)"))
                    })?
                }
                Value::Table(t) => {
                    let mut w = [0u8; 32];
                    for (i, s) in w.iter_mut().enumerate() {
                        *s = int(t.get(ctx, i as i64 + 1))
                            .ok_or_else(|| err(ctx, "wave: table needs 32 numbers 0..31"))?
                            .clamp(0, 31) as u8;
                    }
                    w
                }
                _ => return Err(err(ctx, "wave(ch, preset or table)")),
            };
            let mut s = st.borrow_mut();
            s.psg.channels[ch].wave = w;
            s.psg.reset_phase(ch);
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // psg(ch, {on, note, hz, period, vol, left, right, noise, dda}): set the
    // given registers of channel 1..6; the rest keep their values.
    let st = state.clone();
    set(
        "psg",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let ch = channel(ctx, stack.get(0))?;
            let Value::Table(o) = stack.get(1) else {
                return Err(err(
                    ctx,
                    "psg(ch, {on, note, hz, period, vol, left, right, noise, dda})",
                ));
            };
            let mut s = st.borrow_mut();
            // Charge one register write per field given.
            let mut fields = 0;
            let mut k = Value::Nil;
            while let piccolo::table::NextValue::Found { key, .. } = o.next(k) {
                fields += 1;
                k = key;
            }
            s.counts.psg_writes += fields;
            let c = &mut s.psg.channels[ch];
            let get = |k: &'static str| o.get(ctx, k);
            if !matches!(get("on"), Value::Nil) {
                c.on = truthy(get("on"));
            }
            match get("note") {
                Value::Nil => {}
                v => {
                    let n = note_number(v).ok_or_else(|| {
                        err(ctx, "psg: note must be a number or a name like \"A4\"")
                    })?;
                    c.period = period_for_hz(440.0 * 2f64.powf((n - 69.0) / 12.0));
                }
            }
            if let Value::Integer(_) | Value::Number(_) = get("hz") {
                let hz = match get("hz") {
                    Value::Integer(i) => i as f64,
                    Value::Number(n) => n,
                    _ => 0.0,
                };
                c.period = period_for_hz(hz);
            }
            if let Some(p) = int(get("period")) {
                c.period = p.clamp(0, 4095) as u16;
            }
            if let Some(v) = int(get("vol")) {
                c.volume = v.clamp(0, 31) as u8;
            }
            if let Some(v) = int(get("left")) {
                c.left = v.clamp(0, 15) as u8;
            }
            if let Some(v) = int(get("right")) {
                c.right = v.clamp(0, 15) as u8;
            }
            match get("noise") {
                Value::Nil => {}
                Value::Boolean(b) => c.noise = b,
                v => {
                    if ch < 4 {
                        return Err(err(ctx, "psg: only channels 5 and 6 have noise"));
                    }
                    c.noise = true;
                    c.noise_freq = int(v).unwrap_or(0).clamp(0, 31) as u8;
                }
            }
            match get("dda") {
                Value::Nil => {}
                Value::Boolean(b) => c.dda = b,
                v => {
                    c.dda = true;
                    c.dda_level = int(v).unwrap_or(16).clamp(0, 31) as u8;
                }
            }
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // master(left, right): master balance, 0..15 each.
    let st = state.clone();
    set(
        "master",
        Callback::from_fn(&ctx, move |_, _, mut stack| {
            let l = int(stack.get(0)).unwrap_or(15).clamp(0, 15) as u8;
            let r = int(stack.get(1)).map_or(l, |r| r.clamp(0, 15) as u8);
            let mut s = st.borrow_mut();
            s.psg.master_left = l;
            s.psg.master_right = r;
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // lfo(freq, mode): channel 2 bends channel 1. mode 0 turns it off.
    let st = state.clone();
    set(
        "lfo",
        Callback::from_fn(&ctx, move |_, _, mut stack| {
            let f = int(stack.get(0)).unwrap_or(0).clamp(0, 255) as u8;
            let m = int(stack.get(1)).unwrap_or(1).clamp(0, 3) as u8;
            st.borrow_mut().psg.lfo = crate::psg::Lfo { freq: f, mode: m };
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // image_bg(name, opts) / image_sprites(name, opts): place an image
    // registered with Engine::add_image into VRAM, the VCE and the BAT.
    let st = state.clone();
    set(
        "image_bg",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let (name, opts) = image_args(ctx, &stack, "image_bg")?;
            let t = image_bg_call(ctx, &st, &name, opts).map_err(|m| err(ctx, &m))?;
            stack.replace(ctx, t);
            Ok(CallbackReturn::Return)
        }),
    );
    let st = state.clone();
    set(
        "image_sprites",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let (name, opts) = image_args(ctx, &stack, "image_sprites")?;
            let t = image_sprites_call(ctx, &st, &name, opts).map_err(|m| err(ctx, &m))?;
            stack.replace(ctx, t);
            Ok(CallbackReturn::Return)
        }),
    );

    // __line(y): the resolver marks which line its hooks are running on.
    let st = state.clone();
    set(
        "__line",
        Callback::from_fn(&ctx, move |_, _, mut stack| {
            let y = int(stack.get(0)).unwrap_or(-1);
            let mut s = st.borrow_mut();
            s.line = (y >= 0 && (y as usize) < s.line_writes.len()).then_some(y as usize);
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// A fresh machine with nothing loaded.
    pub fn new() -> Engine {
        let state = Rc::new(RefCell::new(State {
            vdc: Vdc::default(),
            vce: Vce::default(),
            line: None,
            line_writes: vec![Vec::new(); MAX_HEIGHT],
            owner: vec![Owner::None; VRAM_WORDS],
            warnings: Vec::new(),
            psg: Psg::default(),
            loading: false,
            counts: Default::default(),
            images: Default::default(),
            bg_pals_used: 0,
            spr_pals_used: 0,
        }));
        Engine {
            lua: Lua::core(),
            state,
            init_fn: None,
            frame_fn: None,
            frame_file: None,
            audio: Vec::new(),
            touched: vec![false; MAX_HEIGHT],
            budget: Default::default(),
            audio_carry: 0.0,
        }
    }

    /// Register an RGBA8 image (row-major, 4 bytes per pixel) under `name`
    /// for the toy's `image_bg` and `image_sprites` calls. Add images before
    /// `load()`; they are kept across loads. A bad buffer size is reported
    /// when the toy uses the image.
    pub fn add_image(&mut self, name: &str, width: u32, height: u32, rgba: Vec<u8>) {
        let img = Image {
            width,
            height,
            rgba,
        };
        self.state.borrow_mut().images.insert(name.to_string(), img);
    }

    /// The cost meter for the most recent `render()`.
    pub fn budget(&self) -> crate::budget::Budget {
        self.budget
    }

    /// Interleaved stereo i16 samples at [`crate::SAMPLE_RATE`] for the most
    /// recent `frame()`: about 735 frames' worth per 60 Hz frame.
    pub fn audio(&self) -> &[i16] {
        &self.audio
    }

    pub fn psg(&self) -> std::cell::Ref<'_, Psg> {
        std::cell::Ref::map(self.state.borrow(), |s| &s.psg)
    }

    pub fn vdc(&self) -> std::cell::Ref<'_, Vdc> {
        std::cell::Ref::map(self.state.borrow(), |s| &s.vdc)
    }

    pub fn vce(&self) -> std::cell::Ref<'_, Vce> {
        std::cell::Ref::map(self.state.borrow(), |s| &s.vce)
    }

    /// Problems worth fixing that are not errors, such as tile data that
    /// overlaps the BAT. Collected since `load()`.
    pub fn warnings(&self) -> Vec<String> {
        self.state.borrow().warnings.clone()
    }

    fn run_chunk(&mut self, name: &str, src: &str) -> Result<(), LuaError> {
        let ex = self
            .lua
            .try_enter(|ctx| {
                let closure = Closure::load(ctx, Some(name), src.as_bytes())?;
                Ok(ctx.stash(Executor::start(ctx, closure.into(), ())))
            })
            .map_err(|e| static_error(e, Some(name)))?;
        self.lua
            .execute::<()>(&ex)
            .map_err(|e| static_error(e, Some(name)))
    }

    fn call(
        &mut self,
        f: &StashedFunction,
        args: (f64, i64),
        file: Option<&str>,
    ) -> Result<(), LuaError> {
        let ex = self.lua.enter(|ctx| {
            let func = ctx.fetch(f);
            ctx.stash(Executor::start(ctx, func, args))
        });
        self.lua
            .execute::<()>(&ex)
            .map_err(|e| static_error(e, file))
    }

    /// Load a toy from `(file name, source)` chunks, run in order into one
    /// global environment, then run `init()` once if it exists. Resets the
    /// machine first.
    ///
    /// Images added with [`Engine::add_image`] are kept.
    pub fn load(&mut self, files: &[(&str, &str)]) -> Result<(), LuaError> {
        let images = std::mem::take(&mut self.state.borrow_mut().images);
        *self = Engine::new();
        self.state.borrow_mut().images = images;
        let state = self.state.clone();
        state.borrow_mut().loading = true;
        let r = self.load_inner(files);
        state.borrow_mut().loading = false;
        r
    }

    fn load_inner(&mut self, files: &[(&str, &str)]) -> Result<(), LuaError> {
        let state = self.state.clone();
        self.lua.enter(|ctx| install(ctx, &state));
        self.run_chunk("prelude", PRELUDE)?;
        self.run_chunk("kit", KIT)?;
        for (name, src) in files {
            self.run_chunk(name, src)?;
        }
        let find = |lua: &mut Lua, name: &'static str| {
            lua.enter(|ctx| match ctx.get_global(name) {
                Value::Function(f) => Some(ctx.stash(f)),
                _ => None,
            })
        };
        self.init_fn = find(&mut self.lua, "init");
        self.frame_fn = find(&mut self.lua, "frame");
        self.frame_file = self.lua.enter(|ctx| match ctx.get_global("frame") {
            Value::Function(Function::Closure(c)) => {
                Some(c.prototype().chunk_name.to_str_lossy().into_owned())
            }
            _ => None,
        });
        if let Some(init) = self.init_fn.clone() {
            self.call(&init, (0.0, 0), files.last().map(|f| f.0))?;
        }
        Ok(())
    }

    /// Run `frame(t, f)` and the scanline hooks, and return the register
    /// table for the renderer. Sprites are cleared before `frame()` runs, so
    /// it places every sprite it wants on screen.
    pub fn frame(&mut self, t: f64, f: u32) -> Result<Frame, LuaError> {
        {
            let mut s = self.state.borrow_mut();
            s.vdc.sat = [0; SPRITE_COUNT * 4];
            s.line_writes.iter_mut().for_each(Vec::clear);
            s.line = None;
            s.counts = Default::default();
        }
        self.lua.enter(|ctx| {
            ctx.set_global("__hooks", Table::new(&ctx)).unwrap();
        });
        if let Some(func) = self.frame_fn.clone() {
            let file = self.frame_file.clone();
            self.call(&func, (t, f as i64), file.as_deref())?;
        }

        let (width, height, bat) = self.lua.enter(|ctx| {
            let (mut w, mut h) = (256i64, 224i64);
            if let Value::Table(d) = ctx.get_global("display") {
                w = int(d.get(ctx, "width")).unwrap_or(256);
                h = int(d.get(ctx, "height")).unwrap_or(224);
            }
            (
                (w.clamp(8, MAX_WIDTH as i64) as usize) / 8 * 8,
                h.clamp(1, MAX_HEIGHT as i64) as usize,
                bat_size(ctx),
            )
        });

        let ex = self.lua.enter(|ctx| {
            let Value::Function(resolve) = ctx.get_global("__resolve") else {
                unreachable!("prelude defines __resolve")
            };
            ctx.stash(Executor::start(ctx, resolve, height as i64))
        });
        let file = self.frame_file.clone();
        self.lua
            .execute::<()>(&ex)
            .map_err(|e| static_error(e, file.as_deref()))?;

        let mut frame = Frame::new(width, height, LineRegs::default());
        frame.bat = bat;
        self.touched.iter_mut().for_each(|t| *t = false);
        self.lua.enter(|ctx| {
            frame.scroll_y = int(ctx.get_global("__frame_y")).unwrap_or(0) as u16;
            if let Value::Table(t) = ctx.get_global("__lines") {
                for (y, line) in frame.lines.iter_mut().enumerate() {
                    let at = |k: usize| int(t.get(ctx, (y * 5 + k + 1) as i64)).unwrap_or(0);
                    line.scroll_x = at(0) as u16;
                    let reload = at(1);
                    line.scroll_y_write = (reload >= 0).then_some(reload as u16);
                    line.bg_on = at(2) != 0;
                    line.sprites_on = at(3) != 0;
                    self.touched[y] = at(4) != 0;
                }
            }
        });
        let mut s = self.state.borrow_mut();
        for (line, writes) in frame.lines.iter_mut().zip(&s.line_writes) {
            line.palette_writes.clone_from(writes);
        }
        // Sound for this frame, from the PSG registers as frame() left them.
        let want = SAMPLES_PER_FRAME + self.audio_carry;
        let n = want.floor();
        self.audio_carry = want - n;
        self.audio.clear();
        s.psg.render(n as usize, &mut self.audio);
        Ok(frame)
    }

    /// `frame()` then render.
    pub fn render(&mut self, t: f64, f: u32) -> Result<Output, LuaError> {
        let frame = self.frame(t, f)?;
        let s = self.state.borrow();
        let out = render(&s.vdc, &s.vce, &frame);
        self.budget =
            crate::budget::Budget::measure(&frame, &self.touched[..frame.height], s.counts, &out);
        Ok(out)
    }
}
