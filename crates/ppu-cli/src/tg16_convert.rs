//! `ppu tg16 convert`: turn an SNES ppu.toys toy into a TurboGrafx-16 toy.
//!
//! The SNES toy is played in the real SNES engine first. That run supplies
//! everything the TG16 cannot compute for itself: the imported graphics as
//! they land in VRAM, which background layers are on screen where, how they
//! scroll against each other, and which sprites are used. From it this module
//! builds
//!
//! * one TG16 background that stacks the SNES layers, at the offsets they
//!   mostly keep to each other ("squashing" them),
//! * TG16 background palettes packed from the SNES colour indices each 8x8
//!   tile uses, so CGRAM writes and palette animation still work,
//! * 16x16 sprite patterns for every SNES sprite shape and tile the run used,
//!
//! and writes them, with a copy of the SNES toy's Lua and the compatibility
//! layer in `snes_shim.lua`, as an ordinary TG16 toy directory. The report
//! (`CONVERT.md`) lists what the TG16 cannot do and what was dropped.

use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ppu_core::{LuaEngine, Memory, RegRow, SourcePayload};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

const SHIM: &str = include_str!("snes_shim.lua");
const LINES: usize = 224;

/// Wraps `dma()` in the SNES engine so the converter learns what each call
/// returned; the TG16 side replays those values in the same order.
const RECORDER: &str = r#"
local __real_dma = dma
__cv_log = {}
function dma(name, opts)
  local r = __real_dma(name, opts)
  __cv_log[#__cv_log + 1] = { name, r }
  return r
end
local function ser(v)
  local t = type(v)
  if t == "table" then
    local s = "{"
    for k, x in pairs(v) do
      if type(k) == "number" then s = s .. "[" .. tostring(k) .. "]=" else s = s .. k .. "=" end
      s = s .. ser(x) .. ","
    end
    return s .. "}"
  elseif t == "string" then
    return "\"" .. v .. "\""
  else
    return tostring(v)
  end
end
function __cv_dump()
  local s = "{"
  for i = 1, #__cv_log do
    s = s .. "{name=" .. ser(__cv_log[i][1]) .. ",ret=" .. ser(__cv_log[i][2]) .. "},"
  end
  s = s .. "}"
  local out = {}
  for i = 1, #s do out[i] = string.byte(s, i) end
  return out
end
"#;

/// Options for [`convert`].
pub struct ConvertOptions {
    /// Seconds of the SNES toy to play while learning how it behaves.
    pub duration: f64,
    /// Frames to render side by side (SNES left, TG16 right) into `compare/`.
    pub compare: Vec<u32>,
}

/// A background layer configuration seen on screen: which BG, where its map
/// and tiles live, and how it is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct PlaneKey {
    n: u8,
    map_base: u16,
    char_base: u16,
    screen_size: u8,
    tile_size: u8,
    bpp: u8,
}

impl PlaneKey {
    fn lua_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}",
            self.n + 1,
            self.map_base,
            self.char_base,
            self.screen_size,
            self.tile_size,
            self.bpp
        )
    }
    fn tiles(&self) -> (u32, u32) {
        match self.screen_size {
            1 => (64, 32),
            2 => (32, 64),
            3 => (64, 64),
            _ => (32, 32),
        }
    }
    fn size(&self) -> (usize, usize) {
        let (tw, th) = self.tiles();
        (
            (tw * self.tile_size as u32) as usize,
            (th * self.tile_size as u32) as usize,
        )
    }
    fn name(&self) -> String {
        let (w, h) = self.size();
        format!(
            "BG{} ({}bpp, {w}x{h}, map {:#06x})",
            self.n + 1,
            self.bpp,
            self.map_base
        )
    }
}

struct Plane {
    key: PlaneKey,
    mode: u8,
    bg3_priority: bool,
    /// Memory when the plane was first on screen.
    mem: Box<Memory>,
    first_frame: u32,
    /// Fingerprint of its map and tile data, to notice later changes.
    print: u64,
    changed_frames: u32,
    lines: u64,
    // Filled after the run.
    w: usize,
    h: usize,
    index: Vec<u8>,
    prio: Vec<bool>,
    cov: Vec<u32>,
    dx: i64,
    dy: i64,
}

/// One SNES sprite shape as used: OBSEL settings plus the OAM tile and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct SpriteKey {
    char_base: u16,
    name_select: u8,
    size_sel: u8,
    large: bool,
    tile: u16,
}

/// One enabled layer on one line: (plane, scroll x, scroll y).
type LineLayer = (usize, i64, i64);
/// One TG16 sprite of a SNES sprite: (pattern, w, h, x offset, y offset).
type SpritePart = (usize, usize, usize, usize, usize);

const OBJ_SIZES: [[(usize, usize); 2]; 8] = [
    [(8, 8), (16, 16)],
    [(8, 8), (32, 32)],
    [(8, 8), (64, 64)],
    [(16, 16), (32, 32)],
    [(16, 16), (64, 64)],
    [(32, 32), (64, 64)],
    [(16, 32), (32, 64)],
    [(16, 32), (32, 32)],
];

/// What the run noticed that the TG16 version will not reproduce.
#[derive(Default)]
struct Notes {
    frames: u32,
    lines: u64,
    mode7_lines: u64,
    hires_lines: u64,
    math_lines: u64,
    window_lines: u64,
    mosaic_lines: u64,
    sub_only_lines: u64,
    line_brightness_lines: u64,
    offset_per_tile_lines: u64,
    direct_color_lines: u64,
    max_oam: usize,
    max_parts: usize,
    frames_over_64: u32,
    raster_rows: bool,
    audio: bool,
    builtin_sources: Vec<String>,
}

fn char_pixel_index(mem: &Memory, addr: u32, bpp: u8, fx: u32, fy: u32) -> u8 {
    let bit = 7 - (fx & 7);
    let word = |off: u32| mem.vram[((addr + off) & 0x7fff) as usize];
    let pair = |w: u16| (((w as u8) >> bit) & 1) | ((((w >> 8) as u8) >> bit) & 1) << 1;
    let mut i = pair(word(fy));
    if bpp >= 4 {
        i |= pair(word(8 + fy)) << 2;
    }
    if bpp == 8 {
        i |= pair(word(16 + fy)) << 4;
        i |= pair(word(24 + fy)) << 6;
    }
    i
}

fn map_entry_addr(map_base: u16, screen_size: u8, tx: u32, ty: u32) -> usize {
    let screen = match screen_size {
        1 => tx / 32,
        2 => ty / 32,
        3 => (ty / 32) * 2 + tx / 32,
        _ => 0,
    };
    ((map_base as u32 + screen * 0x400 + (ty % 32) * 32 + (tx % 32)) & 0x7fff) as usize
}

/// Front-to-back rank of a BG pixel (lower is in front), by mode, layer and
/// the tile's priority bit.
fn rank(mode: u8, bg3_priority: bool, n: u8, hi: bool) -> u8 {
    match (mode, n) {
        (0, 0) => {
            if hi {
                1
            } else {
                3
            }
        }
        (0, 1) => {
            if hi {
                2
            } else {
                4
            }
        }
        (0, 2) => {
            if hi {
                5
            } else {
                7
            }
        }
        (0, 3) => {
            if hi {
                6
            } else {
                8
            }
        }
        (1, 2) => {
            if hi {
                if bg3_priority {
                    0
                } else {
                    5
                }
            } else {
                6
            }
        }
        (_, 0) => {
            if hi {
                1
            } else {
                3
            }
        }
        (_, 1) => {
            if hi {
                2
            } else {
                4
            }
        }
        _ => 9,
    }
}

fn fingerprint(mem: &Memory, key: &PlaneKey) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let (tw, th) = key.tiles();
    let map_words = (tw * th) as usize;
    for i in 0..map_words {
        mem.vram[(key.map_base as usize + i) & 0x7fff].hash(&mut h);
    }
    let char_words = 1024 * key.bpp as usize * 4;
    for i in 0..char_words.min(0x8000) {
        mem.vram[(key.char_base as usize + i) & 0x7fff].hash(&mut h);
    }
    h.finish()
}

/// Decode a whole plane into CGRAM indices (0 = transparent) and tile
/// priority bits.
fn decode_plane(p: &mut Plane) {
    let k = p.key;
    let (w, h) = k.size();
    let ts = k.tile_size as u32;
    let band = if p.mode == 0 && k.bpp == 2 {
        k.n as u16 * 32
    } else {
        0
    };
    let mut index = vec![0u8; w * h];
    let mut prio = vec![false; w * h];
    for wy in 0..h as u32 {
        for wx in 0..w as u32 {
            let entry = p.mem.vram[map_entry_addr(k.map_base, k.screen_size, wx / ts, wy / ts)];
            let (mut fx, mut fy) = (wx % ts, wy % ts);
            if entry & 0x4000 != 0 {
                fx = ts - 1 - fx;
            }
            if entry & 0x8000 != 0 {
                fy = ts - 1 - fy;
            }
            let ch = ((entry & 0x3ff) as u32 + fx / 8 + (fy / 8) * 16) & 0x3ff;
            let addr = (k.char_base as u32 + ch * k.bpp as u32 * 4) & 0x7fff;
            let i = char_pixel_index(&p.mem, addr, k.bpp, fx % 8, fy % 8);
            if i == 0 {
                continue;
            }
            let ci = if k.bpp == 8 {
                i as u16
            } else {
                band + ((entry >> 10) & 7) * (1 << k.bpp) + i as u16
            };
            let at = (wy as usize) * w + wx as usize;
            index[at] = (ci & 0xff) as u8;
            prio[at] = entry & 0x2000 != 0;
        }
    }
    let cov = (0..h)
        .map(|y| {
            let opaque = index[y * w..(y + 1) * w]
                .iter()
                .filter(|&&i| i != 0)
                .count();
            (opaque * 256 / w) as u32
        })
        .collect();
    p.w = w;
    p.h = h;
    p.index = index;
    p.prio = prio;
    p.cov = cov;
}

/// A SNES sprite's pixels (OBJ palette indices 0..15), `w` x `h`, unflipped.
fn decode_sprite(mem: &Memory, k: &SpriteKey) -> (usize, usize, Vec<u8>) {
    let (w, h) = OBJ_SIZES[k.size_sel as usize & 7][k.large as usize];
    let mut px = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let (col, row) = ((x / 8) as u32, (y / 8) as u32);
            let row_tile = (k.tile as u32 + row * 16) & 0x1ff;
            let name = (row_tile & 0x1f0) | ((row_tile + col) & 0x0f);
            let addr = if name < 256 {
                k.char_base as u32 + name * 16
            } else {
                k.char_base as u32 + (k.name_select as u32 + 1) * 0x1000 + (name - 256) * 16
            };
            px[y * w + x] = char_pixel_index(mem, addr & 0x7fff, 4, (x % 8) as u32, (y % 8) as u32);
        }
    }
    (w, h, px)
}

fn rgb_of(c: u16) -> [i32; 3] {
    [
        (c & 31) as i32,
        ((c >> 5) & 31) as i32,
        ((c >> 10) & 31) as i32,
    ]
}

fn dist(a: u16, b: u16) -> i32 {
    let (a, b) = (rgb_of(a), rgb_of(b));
    (0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum()
}

/// TG16 background palettes: each is up to 15 SNES CGRAM indices.
struct Palettes {
    sets: Vec<Vec<u8>>,
    lossy_tiles: usize,
    merged_tiles: usize,
}

impl Palettes {
    /// Choose a palette for a tile using colour set `s`; returns the palette
    /// and, for each index in `s`, the slot (1..15) it maps to.
    fn place(&mut self, s: &[u8], cgram: &[u16; 256]) -> (usize, HashMap<u8, u8>) {
        let slots = |p: &Vec<u8>| -> HashMap<u8, u8> {
            p.iter()
                .enumerate()
                .map(|(i, &c)| (c, i as u8 + 1))
                .collect()
        };
        // A palette that already holds every colour.
        if let Some(p) = self
            .sets
            .iter()
            .position(|p| s.iter().all(|c| p.contains(c)))
        {
            return (p, slots(&self.sets[p]));
        }
        // The palette that can take the colours with the most overlap.
        let mut best: Option<(usize, usize, usize)> = None;
        for (i, p) in self.sets.iter().enumerate() {
            let missing = s.iter().filter(|c| !p.contains(c)).count();
            if p.len() + missing <= 15 {
                let overlap = s.len() - missing;
                let better = match best {
                    None => true,
                    Some((_, o, l)) => overlap > o || (overlap == o && p.len() < l),
                };
                if better {
                    best = Some((i, overlap, p.len()));
                }
            }
        }
        if let Some((i, overlap, _)) = best {
            if overlap > 0 || self.sets.len() >= 16 {
                for &c in s {
                    if !self.sets[i].contains(&c) {
                        self.sets[i].push(c);
                    }
                }
                return (i, slots(&self.sets[i]));
            }
        }
        if self.sets.len() < 16 {
            self.sets.push(s.to_vec());
            let i = self.sets.len() - 1;
            return (i, slots(&self.sets[i]));
        }
        // Out of palettes: use the closest one, filling its free slots first.
        self.lossy_tiles += 1;
        let i = (0..self.sets.len())
            .max_by_key(|&i| s.iter().filter(|c| self.sets[i].contains(c)).count())
            .unwrap();
        for &c in s {
            if self.sets[i].len() < 15 && !self.sets[i].contains(&c) {
                self.sets[i].push(c);
            }
        }
        let mut map = slots(&self.sets[i]);
        let set = &self.sets[i];
        for &c in s {
            map.entry(c).or_insert_with(|| {
                let (slot, _) = set
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, &o)| dist(cgram[c as usize], cgram[o as usize]))
                    .unwrap();
                slot as u8 + 1
            });
        }
        (i, map)
    }
}

/// Reduce a tile's colour set to 15 by merging the closest colours.
fn reduce_set(s: &[u8], cgram: &[u16; 256]) -> (Vec<u8>, HashMap<u8, u8>) {
    let mut keep = s.to_vec();
    let mut alias = HashMap::new();
    while keep.len() > 15 {
        let mut best = (0, 1, i32::MAX);
        for a in 0..keep.len() {
            for b in a + 1..keep.len() {
                let d = dist(cgram[keep[a] as usize], cgram[keep[b] as usize]);
                if d < best.2 {
                    best = (a, b, d);
                }
            }
        }
        let gone = keep.remove(best.1);
        alias.insert(gone, keep[best.0]);
    }
    // Follow alias chains.
    let resolved = alias
        .keys()
        .map(|&k| {
            let mut v = alias[&k];
            while let Some(&n) = alias.get(&v) {
                v = n;
            }
            (k, v)
        })
        .collect();
    (keep, resolved)
}

/// The middle offset of the observations, per axis: layers that sway
/// against each other are placed at the centre of their swing.
fn mode_of(v: &HashMap<(i64, i64), u64>) -> Option<(i64, i64)> {
    if v.is_empty() {
        return None;
    }
    let total: u64 = v.values().sum();
    let median = |axis: fn(&(i64, i64)) -> i64| {
        let mut items: Vec<(i64, u64)> = v.iter().map(|(k, n)| (axis(k), *n)).collect();
        items.sort();
        let mut seen = 0;
        for (x, n) in items {
            seen += n;
            if seen * 2 >= total {
                return x;
            }
        }
        0
    };
    Some((median(|k| k.0), median(|k| k.1)))
}

fn lua_str_list(out: &mut String, items: &[String]) {
    out.push_str("{\n");
    for s in items {
        let _ = writeln!(out, "  \"{s}\",");
    }
    out.push('}');
}

fn hex_pixels(px: &[u8]) -> String {
    px.iter()
        .map(|&p| char::from_digit(p as u32 & 15, 16).unwrap())
        .collect()
}

fn safe_lua_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Convert the SNES toy at `input` (a directory with `ppu.json`, or a
/// `.ppu.json` pack) into a TG16 toy directory `out`. Returns the report.
pub fn convert(input: &Path, out: &Path, opts: &ConvertOptions) -> Result<String> {
    let body = crate::author::load_body(input)?;
    let end = crate::frame_number(opts.duration)?;
    ensure!(end > 0, "duration must be at least one frame");
    ensure!(
        !out.exists()
            || out
                .read_dir()
                .map(|mut d| d.next().is_none())
                .unwrap_or(false),
        "{} already exists and is not empty",
        out.display()
    );

    let mut notes = Notes::default();

    // Source payloads, for the CGRAM the dma() calls lay down.
    let mut payloads: HashMap<String, SourcePayload> = HashMap::new();
    let mut engine = LuaEngine::new();
    for source in &body.sources {
        let bytes = BASE64
            .decode(&source.payload)
            .with_context(|| format!("invalid payload for {:?}", source.name))?;
        engine
            .add_source(&source.name, &bytes)
            .map_err(|e| anyhow!("source {:?}: {e:?}", source.name))?;
        if let Ok(p) = SourcePayload::decode(&bytes) {
            if matches!(p, SourcePayload::Song(_) | SourcePayload::Sample(_)) {
                notes.audio = true;
            }
            payloads.insert(source.name.clone(), p);
        }
    }
    let mut files: Vec<(&str, &str)> = vec![("__convert.lua", RECORDER)];
    files.extend(
        body.files
            .iter()
            .map(|f| (f.name.as_str(), f.source.as_str())),
    );
    engine
        .set_sources(&files)
        .map_err(crate::author::lua_error)?;
    let dump = engine
        .call_ints("__cv_dump", &[])
        .map_err(crate::author::lua_error)?;
    let dma_lua: String = dump.iter().map(|&b| b as u8 as char).collect();

    // CGRAM as the dma() calls leave it.
    let mut placed = Memory::new();
    for p in engine.memory_map().placements {
        match payloads.get(&p.name) {
            Some(SourcePayload::Bg(src)) => {
                ppu_core::place_bg(src, &mut placed, p.map, p.char, p.pal as usize)
            }
            Some(SourcePayload::Sheet(src)) => {
                ppu_core::place_sheet(src, &mut placed, p.char, p.pal as usize)
            }
            Some(SourcePayload::Obj(src)) => {
                ppu_core::place_obj(src, &mut placed, p.char, p.pal as usize)
            }
            Some(SourcePayload::M7(src)) => ppu_core::place_m7(src, &mut placed),
            Some(_) => {}
            None => notes.builtin_sources.push(p.name.clone()),
        }
    }
    let all_text: String = body.files.iter().map(|f| f.source.as_str()).collect();
    if ["score{", "score {", "kon(", "voice[", "timer("]
        .iter()
        .any(|s| all_text.contains(s))
    {
        notes.audio = true;
    }
    if all_text.contains("raster(") {
        notes.raster_rows = true;
    }

    // Play the SNES toy and watch it.
    let mut planes: Vec<Plane> = Vec::new();
    let mut plane_ids: HashMap<PlaneKey, usize> = HashMap::new();
    // Per line of every frame: (plane, scroll x, scroll y) of each enabled layer.
    let mut seen_lines: Vec<(usize, Vec<LineLayer>)> = Vec::new();
    let mut sprites: BTreeMap<SpriteKey, (usize, usize, Vec<u8>)> = BTreeMap::new();
    let mut snes_frames: HashMap<u32, Vec<u8>> = HashMap::new();
    let compare: HashSet<u32> = opts.compare.iter().copied().collect();

    for f in 0..=end {
        let t = f64::from(f) / 60.0;
        let lt = engine
            .frame(t, f)
            .map_err(crate::author::lua_error)
            .with_context(|| format!("SNES frame {f} at {t:.3}s"))?;
        let mem = engine.memory();
        if compare.contains(&f) {
            snes_frames.insert(f, ppu_core::render_frame(&lt, mem));
        }
        notes.frames += 1;
        let rows: &[RegRow] = &lt.rows;
        let b0 = rows.first().map_or(15, |r| r.brightness);
        for (y, row) in rows.iter().take(LINES).enumerate() {
            notes.lines += 1;
            let blank = row.force_blank;
            if row.mode == 7 && row.tm & 1 != 0 {
                notes.mode7_lines += 1;
            }
            if matches!(row.mode, 5 | 6) {
                notes.hires_lines += 1;
            }
            if row.cgadsub & 0x3f != 0 && (row.tm != 0) {
                notes.math_lines += 1;
            }
            if row.tmw & row.tm != 0 {
                notes.window_lines += 1;
            }
            if row.mosaic_size > 0 && row.mosaic_enable.iter().any(|&e| e) {
                notes.mosaic_lines += 1;
            }
            if row.ts & !row.tm & 0x1f != 0 {
                notes.sub_only_lines += 1;
            }
            if row.brightness != b0 {
                notes.line_brightness_lines += 1;
            }
            if matches!(row.mode, 2 | 4) {
                notes.offset_per_tile_lines += 1;
            }
            let mut here = Vec::new();
            if !blank && row.mode != 7 {
                for n in 0..4u8 {
                    let b = &row.bg[n as usize];
                    if row.tm & (1 << n) == 0 || !b.visible || b.bpp == 0 {
                        continue;
                    }
                    if b.bpp == 8 && b.direct_color {
                        notes.direct_color_lines += 1;
                    }
                    let key = PlaneKey {
                        n,
                        map_base: b.map_base,
                        char_base: b.char_base,
                        screen_size: b.screen_size & 3,
                        tile_size: b.tile_size,
                        bpp: b.bpp,
                    };
                    let id = match plane_ids.get(&key) {
                        Some(&id) => {
                            let p = &mut planes[id];
                            if p.first_frame != f && fingerprint(mem, &key) != p.print {
                                p.changed_frames += 1;
                            }
                            id
                        }
                        None => {
                            planes.push(Plane {
                                key,
                                mode: row.mode,
                                bg3_priority: row.bg3_priority,
                                mem: Box::new(mem.clone()),
                                first_frame: f,
                                print: fingerprint(mem, &key),
                                changed_frames: 0,
                                lines: 0,
                                w: 0,
                                h: 0,
                                index: Vec::new(),
                                prio: Vec::new(),
                                cov: Vec::new(),
                                dx: 0,
                                dy: 0,
                            });
                            plane_ids.insert(key, planes.len() - 1);
                            planes.len() - 1
                        }
                    };
                    planes[id].lines += 1;
                    here.push((id, b.scroll_x as i64, b.scroll_y as i64));
                }
            }
            seen_lines.push((y, here));
        }
        // Sprites (OAM is frame-global).
        let mut oam = 0;
        let mut parts = 0;
        for o in mem.oam.iter() {
            if !o.on {
                continue;
            }
            let (w, _) = OBJ_SIZES[mem.obsel.size_sel as usize & 7][o.large as usize];
            let (x, y) = (o.x as i64, o.y as i64);
            if y >= LINES as i64 || x >= 256 || x + w as i64 <= 0 {
                continue;
            }
            oam += 1;
            parts += if w == 64 { 2 } else { 1 };
            let key = SpriteKey {
                char_base: mem.obsel.char_base,
                name_select: mem.obsel.name_select & 3,
                size_sel: mem.obsel.size_sel & 7,
                large: o.large,
                tile: o.tile & 0x1ff,
            };
            sprites
                .entry(key)
                .or_insert_with(|| decode_sprite(mem, &key));
        }
        notes.max_oam = notes.max_oam.max(oam);
        notes.max_parts = notes.max_parts.max(parts);
        if parts > 64 {
            notes.frames_over_64 += 1;
        }
    }

    // Decode planes, pick the base plane and everyone's offset from it.
    for p in planes.iter_mut() {
        decode_plane(p);
    }
    let mut driver_lines = vec![0u64; planes.len()];
    for (y, here) in &seen_lines {
        let best = here
            .iter()
            .enumerate()
            .max_by_key(|(_, &(id, _, sy))| {
                let p = &planes[id];
                // (coverage, front-most first)
                let r = ((*y as i64 + 1 + sy).rem_euclid(p.h as i64)) as usize;
                (p.cov[r], std::cmp::Reverse(p.key.n))
            })
            .map(|(_, &(id, _, _))| id);
        if let Some(id) = best {
            driver_lines[id] += 1;
        }
    }
    let mut order: Vec<usize> = (0..planes.len()).collect();
    order.sort_by_key(|&i| {
        (
            std::cmp::Reverse(driver_lines[i]),
            std::cmp::Reverse(planes[i].lines),
            planes[i].key,
        )
    });
    let mut positioned: HashMap<usize, (i64, i64)> = HashMap::new();
    let mut drift: HashMap<usize, i64> = HashMap::new();
    let mut alone: Vec<usize> = Vec::new();
    if let Some(&base) = order.first() {
        positioned.insert(base, (0, 0));
        drift.insert(base, 0);
    }
    loop {
        let mut progress = false;
        for &id in &order {
            if positioned.contains_key(&id) {
                continue;
            }
            // Offsets against already placed planes it shares lines with.
            let mut obs: HashMap<(i64, i64), u64> = HashMap::new();
            for (_, here) in &seen_lines {
                let Some(&(_, sx, sy)) = here.iter().find(|e| e.0 == id) else {
                    continue;
                };
                if let Some(&(q, qx, qy)) = here.iter().find(|e| positioned.contains_key(&e.0)) {
                    let (ox, oy) = positioned[&q];
                    *obs.entry((sx - qx + ox, sy - qy + oy)).or_default() += 1;
                }
            }
            if let Some(off) = mode_of(&obs) {
                let worst = obs
                    .keys()
                    .map(|&(x, y)| (x - off.0).abs().max((y - off.1).abs()))
                    .max()
                    .unwrap_or(0);
                positioned.insert(id, off);
                drift.insert(id, worst);
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    for &id in &order {
        if let std::collections::hash_map::Entry::Vacant(e) = positioned.entry(id) {
            e.insert((0, 0));
            alone.push(id);
        }
    }

    // The squashed background: big enough for every plane, within the BAT.
    let mut cw = planes.iter().map(|p| p.w).max().unwrap_or(256).max(256);
    let mut ch = planes.iter().map(|p| p.h).max().unwrap_or(256).max(256);
    let mut cropped = false;
    if cw > 1024 {
        cw = 1024;
        cropped = true;
    }
    if ch > 512 {
        ch = 512;
        cropped = true;
    }
    for p in planes.iter_mut() {
        let (ox, oy) = positioned[&plane_ids[&p.key]];
        p.dx = ox;
        p.dy = oy;
    }
    let mut composite = vec![0u8; cw * ch];
    {
        let mut best_rank = vec![u8::MAX; cw * ch];
        for p in &planes {
            for cy in 0..ch {
                let py = ((cy as i64 + p.dy).rem_euclid(p.h as i64)) as usize;
                for cx in 0..cw {
                    let px = ((cx as i64 + p.dx).rem_euclid(p.w as i64)) as usize;
                    let i = p.index[py * p.w + px];
                    if i == 0 {
                        continue;
                    }
                    let r = rank(p.mode, p.bg3_priority, p.key.n, p.prio[py * p.w + px]);
                    let at = cy * cw + cx;
                    if r < best_rank[at] {
                        best_rank[at] = r;
                        composite[at] = i;
                    }
                }
            }
        }
    }

    // Reference colours for any lossy palette decisions.
    let reference: [u16; 256] = planes.first().map(|p| p.mem.cgram).unwrap_or(placed.cgram);

    // Tiles and palettes.
    let (tw, th) = (cw / 8, ch / 8);
    let mut tile_sets: Vec<Vec<u8>> = Vec::with_capacity(tw * th);
    for ty in 0..th {
        for tx in 0..tw {
            let mut s: Vec<u8> = Vec::new();
            for y in 0..8 {
                for x in 0..8 {
                    let i = composite[(ty * 8 + y) * cw + tx * 8 + x];
                    if i != 0 && !s.contains(&i) {
                        s.push(i);
                    }
                }
            }
            s.sort_unstable();
            tile_sets.push(s);
        }
    }
    let mut pals = Palettes {
        sets: Vec::new(),
        lossy_tiles: 0,
        merged_tiles: 0,
    };
    // Place big sets first so small ones can share their palettes.
    let mut by_size: Vec<usize> = (0..tile_sets.len())
        .filter(|&i| !tile_sets[i].is_empty())
        .collect();
    by_size.sort_by_key(|&i| (std::cmp::Reverse(tile_sets[i].len()), tile_sets[i].clone()));
    let mut tile_pal: Vec<(usize, HashMap<u8, u8>)> = vec![(0, HashMap::new()); tile_sets.len()];
    for &i in &by_size {
        let (set, alias) = if tile_sets[i].len() > 15 {
            pals.merged_tiles += 1;
            reduce_set(&tile_sets[i], &reference)
        } else {
            (tile_sets[i].clone(), HashMap::new())
        };
        let (p, mut map) = pals.place(&set, &reference);
        for (gone, kept) in alias {
            let slot = map[&kept];
            map.insert(gone, slot);
        }
        tile_pal[i] = (p, map);
    }
    let mut tiles: Vec<[u8; 64]> = vec![[0; 64]];
    let mut tile_ids: HashMap<[u8; 64], usize> = HashMap::new();
    tile_ids.insert([0; 64], 0);
    let mut bat_cells: Vec<(usize, usize)> = Vec::with_capacity(tw * th);
    for ty in 0..th {
        for tx in 0..tw {
            let ti = ty * tw + tx;
            let (pal, map) = &tile_pal[ti];
            let mut px = [0u8; 64];
            for y in 0..8 {
                for x in 0..8 {
                    let i = composite[(ty * 8 + y) * cw + tx * 8 + x];
                    if i != 0 {
                        px[y * 8 + x] = map[&i];
                    }
                }
            }
            let id = *tile_ids.entry(px).or_insert_with(|| {
                tiles.push(px);
                tiles.len() - 1
            });
            bat_cells.push((id, if id == 0 { 0 } else { *pal }));
        }
    }

    // VRAM: BAT, then tiles, then sprite patterns.
    let bat_words = tw * th;
    let first_tile = bat_words.div_ceil(16);
    let tiles_end = (first_tile + tiles.len()) * 16;
    let first_pattern = tiles_end.div_ceil(64);
    let mut used = vec![false; 1024];
    let mut patterns: Vec<(usize, [u8; 256])> = Vec::new();
    let mut part_ids: HashMap<(usize, usize, Vec<u8>), usize> = HashMap::new();
    let mut sprite_parts: BTreeMap<SpriteKey, Vec<SpritePart>> = BTreeMap::new();
    let mut out_of_patterns = 0usize;
    for (key, (w, h, px)) in &sprites {
        let (pw, ph) = ((*w).clamp(16, 32), (*h).max(16));
        let cols = if *w == 64 { 2 } else { 1 };
        let mut list = Vec::new();
        for c in 0..cols {
            let ox = c * 32;
            // The part's pixels, padded to its TG16 size.
            let mut img = vec![0u8; pw * ph];
            for y in 0..ph.min(*h) {
                for x in 0..pw.min(*w - ox) {
                    img[y * pw + x] = px[y * *w + ox + x];
                }
            }
            let id_key = (pw, ph, img.clone());
            let base = if let Some(&b) = part_ids.get(&id_key) {
                b
            } else {
                let mask = (if pw == 32 { 1 } else { 0 })
                    | match ph {
                        32 => 2,
                        64 => 6,
                        _ => 0,
                    };
                let cells: Vec<(usize, usize)> = (0..ph / 16)
                    .flat_map(|cy| (0..pw / 16).map(move |cx| (cx, cy)))
                    .collect();
                let found = (first_pattern..1024).find(|&b| {
                    b & mask == 0
                        && cells
                            .iter()
                            .all(|&(cx, cy)| b + cx + cy * 2 < 1024 && !used[b + cx + cy * 2])
                });
                let Some(b) = found else {
                    out_of_patterns += 1;
                    continue;
                };
                for &(cx, cy) in &cells {
                    used[b + cx + cy * 2] = true;
                    let mut pat = [0u8; 256];
                    for y in 0..16 {
                        for x in 0..16 {
                            pat[y * 16 + x] = img[(cy * 16 + y) * pw + cx * 16 + x];
                        }
                    }
                    patterns.push((b + cx + cy * 2, pat));
                }
                part_ids.insert(id_key, b);
                b
            };
            list.push((base, pw, ph, ox, 0));
        }
        sprite_parts.insert(*key, list);
    }
    patterns.sort_by_key(|p| p.0);
    let last_pattern = patterns.last().map_or(first_pattern, |p| p.0 + 1);
    let vram_end = last_pattern * 64;
    ensure!(
        tiles_end <= 0x8000,
        "the squashed background needs {} tiles, more than TG16 VRAM holds",
        tiles.len()
    );

    // Palette map: SNES CGRAM index -> TG16 VCE entries.
    let mut pal_map: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    pal_map.entry(0).or_default().push(0);
    for (p, set) in pals.sets.iter().enumerate() {
        for (slot, &c) in set.iter().enumerate() {
            pal_map
                .entry(c as usize)
                .or_default()
                .push(p * 16 + slot + 1);
        }
    }
    for k in 1..128 {
        if k % 16 != 0 {
            pal_map.entry(128 + k).or_default().push(256 + k);
        }
    }

    // Write the TG16 toy.
    fs::create_dir_all(out).with_context(|| format!("cannot create {}", out.display()))?;
    let mut data = String::new();
    let _ = writeln!(
        data,
        "-- Data for the SNES compatibility layer, written by ppu tg16 convert."
    );
    let _ = writeln!(
        data,
        "-- Graphics, palette map and dma() results captured from the SNES engine."
    );
    let _ = writeln!(data, "__snes = {{}}");
    let _ = writeln!(data, "__snes.w, __snes.h = {cw}, {ch}");
    let _ = writeln!(
        data,
        "__snes.first_tile, __snes.first_pattern = {first_tile}, {first_pattern}"
    );
    // Tile 0 of the list is blank; tile i is TG16 tile first_tile + i.
    data.push_str("__snes.tiles = ");
    lua_str_list(
        &mut data,
        &tiles.iter().map(|t| hex_pixels(t)).collect::<Vec<_>>(),
    );
    data.push('\n');
    data.push_str("__snes.bat = {");
    for (i, &(id, pal)) in bat_cells.iter().enumerate() {
        if i % 16 == 0 {
            data.push_str("\n  ");
        }
        let _ = write!(data, "{},", ((first_tile + id) & 0xfff) | (pal << 12));
    }
    data.push_str("\n}\n");
    // Patterns are contiguous from first_pattern; gaps are blank.
    data.push_str("__snes.patterns = {\n");
    let mut next = first_pattern;
    for (n, pat) in &patterns {
        while next < *n {
            let _ = writeln!(data, "  string.rep(\"0\", 256),");
            next += 1;
        }
        let _ = writeln!(data, "  \"{}\",", hex_pixels(pat));
        next += 1;
    }
    data.push_str("}\n");
    data.push_str("__snes.cgram = {");
    for (i, c) in placed.cgram.iter().enumerate() {
        if i % 16 == 0 {
            data.push_str("\n  ");
        }
        let _ = write!(data, "{c},");
    }
    data.push_str("\n}\n");
    data.push_str("__snes.pal = {\n");
    for (i, entries) in &pal_map {
        let list: Vec<String> = entries.iter().map(|e| e.to_string()).collect();
        let _ = writeln!(data, "  [{i}] = {{{}}},", list.join(","));
    }
    data.push_str("}\n");
    data.push_str("__snes.planes = {\n");
    for p in &planes {
        let r = rank(p.mode, false, p.key.n, false);
        let cov: Vec<String> = p.cov.iter().map(|c| c.to_string()).collect();
        let _ = writeln!(
            data,
            "  [\"{}\"] = {{ dx = {}, dy = {}, h = {}, rank = {r}, cov = {{{}}} }},",
            p.key.lua_key(),
            p.dx,
            p.dy,
            p.h,
            cov.join(",")
        );
    }
    data.push_str("}\n");
    data.push_str("__snes.sprites = {\n");
    for (k, parts) in &sprite_parts {
        let list: Vec<String> = parts
            .iter()
            .map(|(b, w, h, ox, oy)| format!("{{{b},{w},{h},{ox},{oy}}}"))
            .collect();
        let _ = writeln!(
            data,
            "  [\"{}:{}:{}:{}:{}\"] = {{{}}},",
            k.char_base,
            k.name_select,
            k.size_sel,
            k.large as u8,
            k.tile,
            list.join(",")
        );
    }
    data.push_str("}\n");
    let _ = writeln!(data, "__snes.dma = {dma_lua}");

    fs::write(out.join("00_snes_data.lua"), data)?;
    fs::write(out.join("01_snes_shim.lua"), SHIM)?;
    let mut names = Vec::new();
    for (i, f) in body.files.iter().enumerate() {
        let stem = f.name.strip_suffix(".lua").unwrap_or(&f.name);
        let name = format!("{:02}_{}.lua", 10 + i, safe_lua_name(stem));
        fs::write(out.join(&name), &f.source)?;
        names.push((f.name.clone(), name));
    }
    let glue = format!(
        "-- Converted from the SNES toy \"{}\" by ppu tg16 convert.\n\
         -- The SNES Lua is in the numbered files, unchanged. 01_snes_shim.lua runs\n\
         -- it on the TG16; CONVERT.md says what the conversion dropped.\n\
         local snes_init, snes_frame = init, frame\n\
         function init() __snes_init(snes_init) end\n\
         function frame(t, f) __snes_frame(snes_frame, t, f) end\n",
        body.title.replace('"', "'")
    );
    fs::write(out.join("main.lua"), glue)?;

    // Play the TG16 version: errors fail the conversion; the meter goes in the report.
    let toy = crate::tg16::Toy::from_dir(out)?;
    let mut tg = crate::tg16::engine_from_toy(&toy).context("the converted toy does not load")?;
    let mut peak = tg16_core::Budget::default();
    let mut over_frames = 0u32;
    let mut overflow_frames = 0u32;
    let mut first_problem: Option<(u32, String)> = None;
    for f in 0..=end {
        let t = f64::from(f) / 60.0;
        let o = tg
            .render(t, f)
            .map_err(|e| anyhow!("converted toy, frame {f}: {e}"))?;
        let b = tg.budget();
        peak.cycles = peak.cycles.max(b.cycles);
        peak.raster_lines = peak.raster_lines.max(b.raster_lines);
        peak.palette_writes = peak.palette_writes.max(b.palette_writes);
        peak.vram_words = peak.vram_words.max(b.vram_words);
        peak.sprites = peak.sprites.max(b.sprites);
        peak.peak_line_units = peak.peak_line_units.max(b.peak_line_units);
        if o.overflow() {
            overflow_frames += 1;
        }
        let problems: Vec<_> = b
            .problems()
            .into_iter()
            .filter(|p| !p.starts_with("sprites dropped"))
            .collect();
        if !problems.is_empty() {
            over_frames += 1;
            if first_problem.is_none() {
                first_problem = Some((f, problems.join("; ")));
            }
        }
        if let Some(snes) = snes_frames.get(&f) {
            let dir = out.join("compare");
            fs::create_dir_all(&dir)?;
            let mut img = vec![0u8; 512 * LINES * 4];
            for y in 0..LINES {
                img[y * 2048..y * 2048 + 1024].copy_from_slice(&snes[y * 1024..y * 1024 + 1024]);
                if y < o.height {
                    let w = o.width.min(256);
                    let src = &o.rgba[y * o.width * 4..y * o.width * 4 + w * 4];
                    img[y * 2048 + 1024..y * 2048 + 1024 + w * 4].copy_from_slice(src);
                }
            }
            crate::author::write_png(
                &dir.join(format!("frame-{f:05}.png")),
                512,
                LINES as u32,
                &img,
            )?;
        }
    }
    let warnings = tg.warnings();

    // The report.
    let pct = |n: u64| {
        if notes.lines == 0 {
            0.0
        } else {
            n as f64 * 100.0 / notes.lines as f64
        }
    };
    let mut r = String::new();
    let _ = writeln!(r, "# Conversion report: {}\n", body.title);
    let _ = writeln!(
        r,
        "Converted by `ppu tg16 convert` from {} seconds ({} frames) of the SNES toy.\n",
        opts.duration, notes.frames
    );
    let _ = writeln!(r, "## Files\n");
    let _ = writeln!(r, "| File | What it is |\n| --- | --- |");
    let _ = writeln!(r, "| `00_snes_data.lua` | Graphics, palette map and `dma()` results captured from the SNES run |");
    let _ = writeln!(
        r,
        "| `01_snes_shim.lua` | The SNES compatibility layer that runs the SNES Lua on the TG16 |"
    );
    for (from, to) in &names {
        let _ = writeln!(r, "| `{to}` | `{from}` from the SNES toy, unchanged |");
    }
    let _ = writeln!(
        r,
        "| `main.lua` | Hooks the SNES `init()` and `frame()` into the TG16 engine |\n"
    );

    let _ = writeln!(r, "## Background\n");
    if planes.is_empty() {
        let _ = writeln!(r, "No tile layer was on the main screen during the run, so the TG16 background is empty.\n");
    } else {
        let _ = writeln!(
            r,
            "{} SNES layer setup{} became one {}x{} TG16 background ({} unique tiles, {} of 16 palettes). \
             Each line scrolls with the layer that covers most of it; the other layers on that line move with it.\n",
            planes.len(),
            if planes.len() == 1 { "" } else { "s" },
            cw,
            ch,
            tiles.len() - 1,
            pals.sets.len()
        );
        let _ = writeln!(r, "| Layer | On screen | Leads lines | Offset in the squashed layer | Moves against it by up to |\n| --- | --- | --- | --- | --- |");
        for &id in &order {
            let p = &planes[id];
            let lead = if driver_lines[id] == 0 {
                "0%".to_string()
            } else {
                format!("{:.0}%", pct(driver_lines[id]))
            };
            let moves = if alone.contains(&id) {
                "never on screen with the others".to_string()
            } else if id == order[0] {
                "(base layer)".to_string()
            } else {
                let d = drift.get(&id).copied().unwrap_or(0);
                if d as usize >= p.w.min(p.h) / 2 {
                    "scrolls on its own; frozen against the base layer on lines it does not lead"
                        .to_string()
                } else {
                    format!("{d} px")
                }
            };
            let _ = writeln!(
                r,
                "| {} | {:.0}% of lines | {lead} | {}, {} | {moves} |",
                p.key.name(),
                pct(p.lines),
                p.dx,
                p.dy
            );
        }
        r.push('\n');
    }

    let mut dropped_list: Vec<String> = Vec::new();
    let mut add = |cond: bool, s: String| {
        if cond {
            dropped_list.push(s);
        }
    };
    add(notes.mode7_lines > 0, format!("**Mode 7** on {:.0}% of lines. The TG16 cannot rotate or scale a layer; those lines show no background.", pct(notes.mode7_lines)));
    add(notes.math_lines > 0, format!("**Colour math** (add/subtract/half) on {:.0}% of lines. Dropped; lines show the main screen only.", pct(notes.math_lines)));
    add(notes.sub_only_lines > 0, format!("**Sub-screen-only layers** on {:.0}% of lines. Without colour math there is nothing to blend them into, so they are not shown.", pct(notes.sub_only_lines)));
    add(
        notes.window_lines > 0,
        format!(
            "**Windows** on {:.0}% of lines. Dropped; masked layers show everywhere.",
            pct(notes.window_lines)
        ),
    );
    add(
        notes.mosaic_lines > 0,
        format!(
            "**Mosaic** on {:.0}% of lines. Dropped.",
            pct(notes.mosaic_lines)
        ),
    );
    add(notes.line_brightness_lines > 0, format!("**Per-line brightness** on {:.0}% of lines. The TG16 has no brightness register; only the frame-wide brightness is kept (as palette scaling).", pct(notes.line_brightness_lines)));
    add(
        notes.offset_per_tile_lines > 0,
        format!(
            "**Offset-per-tile** (modes 2/4) on {:.0}% of lines. Columns do not scroll separately.",
            pct(notes.offset_per_tile_lines)
        ),
    );
    add(
        notes.hires_lines > 0,
        format!(
            "**Hi-res modes 5/6** on {:.0}% of lines. Not converted.",
            pct(notes.hires_lines)
        ),
    );
    add(
        notes.direct_color_lines > 0,
        "**Direct colour** 8bpp layers. Their colours are wrong on the TG16.".to_string(),
    );
    add(
        cropped,
        "The SNES map is bigger than the TG16 map (1024x512); the rest is cut off.".to_string(),
    );
    add(!alone.is_empty(), format!("{} layer setup(s) were never on screen with the base layer; they are stacked on top of it at offset 0 and may cover it.", alone.len()));
    for p in &planes {
        add(p.changed_frames > 0, format!("{} changes its map or tiles during the run ({} frames). The TG16 version keeps the graphics from {:.2}s.", p.key.name(), p.changed_frames, f64::from(p.first_frame) / 60.0));
    }
    add(pals.merged_tiles > 0, format!("{} tiles use more than 15 colours where layers overlap; their closest colours were merged.", pals.merged_tiles));
    add(pals.lossy_tiles > 0, format!("{} tiles did not fit the 16 background palettes; they use the nearest colours of another palette.", pals.lossy_tiles));
    add(notes.max_parts > 64 || notes.frames_over_64 > 0, format!("**Sprites**: up to {} SNES sprites on screen ({} TG16 sprites needed, 64 exist). Over 64 on {} frames; the highest OAM entries are left out there.", notes.max_oam, notes.max_parts, notes.frames_over_64));
    add(
        out_of_patterns > 0,
        format!("{out_of_patterns} sprite shapes did not fit in VRAM and are not shown."),
    );
    add(notes.audio, "**Sound**: the S-DSP music and samples are not converted. The TG16 PSG is a different instrument; write its part with `wave()` and `psg()`.".to_string());
    add(
        notes.raster_rows,
        "`raster()` register rows are ignored.".to_string(),
    );
    add(
        !notes.builtin_sources.is_empty(),
        format!(
            "Built-in sources ({}) are not converted.",
            notes.builtin_sources.join(", ")
        ),
    );
    add(true, "Background and sprite graphics are fixed at conversion: later `vram[]` or `bg[n].map` writes do not change the picture. Palettes, scroll, layer switches and sprites stay live.".to_string());
    add(
        true,
        "Controller input is not wired up yet (`pad` reads as released).".to_string(),
    );

    let _ = writeln!(r, "## Not converted\n");
    for s in &dropped_list {
        let _ = writeln!(r, "- {s}");
    }
    r.push('\n');

    let _ = writeln!(r, "## TG16 budget meter\n");
    let _ = writeln!(r, "| Measure | Peak | Limit |\n| --- | --- | --- |");
    let _ = writeln!(
        r,
        "| CPU (estimate) | {:.0}% | 100% of a frame |",
        peak.cpu_share() * 100.0
    );
    let _ = writeln!(r, "| Raster lines | {} | 224 |", peak.raster_lines);
    let _ = writeln!(
        r,
        "| Palette writes | {} | (each costs CPU) |",
        peak.palette_writes
    );
    let _ = writeln!(r, "| Sprites | {} | 64 |", peak.sprites);
    let _ = writeln!(
        r,
        "| Busiest sprite line | {} units | 16 |",
        peak.peak_line_units
    );
    let _ = writeln!(
        r,
        "| VRAM written per frame | {} words | about {} |",
        peak.vram_words,
        tg16_core::budget::VBLANK_VRAM_WORDS
    );
    let _ = writeln!(
        r,
        "| VRAM used | {} of 32768 words | BAT {}, tiles {}, sprite patterns {} |",
        vram_end.max(tiles_end),
        bat_words,
        tiles_end - first_tile * 16,
        vram_end.saturating_sub(first_pattern * 64)
    );
    r.push('\n');
    if let Some((f, p)) = &first_problem {
        let _ = writeln!(
            r,
            "Over budget on {over_frames} frames; first at frame {f}: {p}.\n"
        );
    } else {
        let _ = writeln!(r, "Within budget on every frame.\n");
    }
    if overflow_frames > 0 {
        let _ = writeln!(r, "More than 16 sprite units on a line on {overflow_frames} frames; sprites drop out there, as on the console.\n");
    }
    for w in &warnings {
        let _ = writeln!(r, "- warning: {w}");
    }
    if !opts.compare.is_empty() {
        let _ = writeln!(
            r,
            "\nSide-by-side frames (SNES left, TG16 right) are in `compare/`."
        );
    }
    fs::write(out.join("CONVERT.md"), &r)?;
    if out.join("compare").exists() && opts.compare.is_empty() {
        bail!("unexpected compare directory");
    }
    Ok(r)
}
