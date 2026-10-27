//! `ppu tg16 ...`: author TurboGrafx-16 toys with the same loop as SNES ones.

use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::fs;
use std::path::{Path, PathBuf};
use tg16_core::Engine;

use crate::author::write_png;

/// `format` field of a `.tg16.json` pack.
pub const PACK_FORMAT: &str = "ppu.toys/tg16";
/// Newest pack `version` this build reads and the one it writes.
pub const PACK_VERSION: u64 = 1;
/// Largest pack (JSON text) or toy (sources + asset bytes) accepted.
pub const MAX_PACK_BYTES: usize = 32 << 20;
/// Most Lua files, and separately most assets, in one toy.
pub const MAX_ENTRIES: usize = 256;

/// A TG16 toy in memory: its Lua sources in load order and its PNG assets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toy {
    pub name: String,
    /// `(file name, source)` in load order: name order, `main.lua` last.
    pub files: Vec<(String, String)>,
    /// `(file name, raw PNG bytes)` from `assets/`, in name order.
    pub assets: Vec<(String, Vec<u8>)>,
}

/// Decode a PNG of any color type and depth to `(width, height, RGBA8)`.
pub fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().context("invalid PNG")?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).context("invalid PNG")?;
    let buf = &buf[..info.buffer_size()];
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    for p in buf.chunks_exact(info.color_type.samples()) {
        match info.color_type {
            png::ColorType::Rgba => rgba.extend_from_slice(p),
            png::ColorType::Rgb => rgba.extend_from_slice(&[p[0], p[1], p[2], 255]),
            png::ColorType::Grayscale => rgba.extend_from_slice(&[p[0], p[0], p[0], 255]),
            png::ColorType::GrayscaleAlpha => rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]]),
            png::ColorType::Indexed => bail!("PNG palette was not expanded"),
        }
    }
    Ok((info.width, info.height, rgba))
}

/// A decoded image: `(name, width, height, RGBA8)`.
pub type NamedImage = (String, u32, u32, Vec<u8>);

/// Every `assets/*.png` of a toy directory, named by file stem, in name
/// order.
pub fn load_images(dir: &Path) -> Result<Vec<NamedImage>> {
    let assets = dir.join("assets");
    if !assets.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&assets)
        .with_context(|| format!("cannot read {}", assets.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let bytes = fs::read(&p).with_context(|| format!("cannot read {}", p.display()))?;
            let (w, h, rgba) = decode_png(&bytes).with_context(|| format!("{}", p.display()))?;
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            Ok((name, w, h, rgba))
        })
        .collect()
}

/// A bare file name (no directories, no `.`/`..`, no separators) with `ext`.
fn valid_name(name: &str, ext: &str) -> bool {
    name.len() > ext.len()
        && name.ends_with(ext)
        && !name.contains(['/', '\\', '\0'])
        && Path::new(name).components().count() == 1
        && matches!(
            Path::new(name).components().next(),
            Some(std::path::Component::Normal(_))
        )
}

fn load_order(files: &mut [(String, String)]) {
    files.sort_by(|a, b| (a.0 == "main.lua", &a.0).cmp(&(b.0 == "main.lua", &b.0)));
}

fn file_name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

/// Files in `dir` (top level only) whose names end with `ext`.
fn list(dir: &Path, ext: &str) -> Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && file_name(p).ends_with(ext))
        .collect();
    out.sort();
    Ok(out)
}

#[derive(serde::Serialize)]
struct PackOut<'a> {
    format: &'a str,
    version: u64,
    name: &'a str,
    files: std::collections::BTreeMap<&'a str, &'a str>,
    assets: std::collections::BTreeMap<&'a str, String>,
}

#[derive(serde::Deserialize)]
struct PackIn {
    format: Option<String>,
    version: Option<u64>,
    #[serde(default)]
    name: String,
    files: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    assets: std::collections::BTreeMap<String, String>,
}

impl Toy {
    /// Read a toy directory: every top-level `*.lua` and every `assets/*.png`.
    pub fn from_dir(dir: &Path) -> Result<Toy> {
        let name = fs::canonicalize(dir)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "toy".into());
        let mut files = Vec::new();
        for p in list(dir, ".lua")? {
            let src =
                fs::read_to_string(&p).with_context(|| format!("cannot read {}", p.display()))?;
            files.push((file_name(&p), src));
        }
        load_order(&mut files);
        let mut assets = Vec::new();
        let asset_dir = dir.join("assets");
        if asset_dir.is_dir() {
            for p in list(&asset_dir, ".png")? {
                let bytes = fs::read(&p).with_context(|| format!("cannot read {}", p.display()))?;
                assets.push((file_name(&p), bytes));
            }
        }
        let toy = Toy {
            name,
            files,
            assets,
        };
        toy.validate()
            .with_context(|| format!("in {}", dir.display()))?;
        Ok(toy)
    }

    /// Parse `.tg16.json` pack text. Unknown top-level keys are ignored.
    pub fn from_pack_json(text: &str) -> Result<Toy> {
        ensure!(
            text.len() <= MAX_PACK_BYTES,
            "pack is larger than {MAX_PACK_BYTES} bytes"
        );
        let pack: PackIn = serde_json::from_str(text).context("invalid .tg16.json pack")?;
        match pack.format.as_deref() {
            Some(PACK_FORMAT) => {}
            Some(other) => bail!("unknown pack format {other:?} (expected {PACK_FORMAT:?})"),
            None => bail!("not a TG16 pack (no \"format\" field)"),
        }
        match pack.version {
            Some(v) if (1..=PACK_VERSION).contains(&v) => {}
            Some(v) => bail!("pack version {v} is newer than this ppu reads ({PACK_VERSION})"),
            None => bail!("pack has no \"version\" field"),
        }
        let mut files: Vec<_> = pack.files.into_iter().collect();
        load_order(&mut files);
        let assets = pack
            .assets
            .into_iter()
            .map(|(name, data)| {
                let bytes = BASE64
                    .decode(data.as_bytes())
                    .with_context(|| format!("asset {name:?} is not valid base64"))?;
                Ok((name, bytes))
            })
            .collect::<Result<Vec<_>>>()?;
        let toy = Toy {
            name: pack.name,
            files,
            assets,
        };
        toy.validate()?;
        Ok(toy)
    }

    /// Serialize as `.tg16.json` pack text (keys sorted, pretty-printed).
    pub fn to_pack_json(&self) -> Result<String> {
        self.validate()?;
        let pack = PackOut {
            format: PACK_FORMAT,
            version: PACK_VERSION,
            name: &self.name,
            files: self
                .files
                .iter()
                .map(|(n, s)| (n.as_str(), s.as_str()))
                .collect(),
            assets: self
                .assets
                .iter()
                .map(|(n, b)| (n.as_str(), BASE64.encode(b)))
                .collect(),
        };
        Ok(serde_json::to_string_pretty(&pack)? + "\n")
    }

    /// Write the toy as a directory: Lua files at the top, PNGs in `assets/`.
    /// Validates every name before writing anything.
    pub fn write_dir(&self, dir: &Path) -> Result<()> {
        self.validate()?;
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        for (name, src) in &self.files {
            fs::write(dir.join(name), src).with_context(|| format!("cannot write {name}"))?;
        }
        let asset_dir = dir.join("assets");
        fs::create_dir_all(&asset_dir)
            .with_context(|| format!("cannot create {}", asset_dir.display()))?;
        for (name, bytes) in &self.assets {
            fs::write(asset_dir.join(name), bytes)
                .with_context(|| format!("cannot write assets/{name}"))?;
        }
        Ok(())
    }

    /// Names are safe bare `*.lua` / `*.png` file names, unique, within limits.
    fn validate(&self) -> Result<()> {
        ensure!(!self.files.is_empty(), "toy has no .lua files");
        ensure!(
            self.files.len() <= MAX_ENTRIES && self.assets.len() <= MAX_ENTRIES,
            "toy has more than {MAX_ENTRIES} files or assets"
        );
        let mut seen = std::collections::HashSet::new();
        for (name, _) in &self.files {
            ensure!(
                valid_name(name, ".lua"),
                "unsafe or non-.lua file name {name:?}"
            );
            ensure!(seen.insert(name.as_str()), "duplicate file {name:?}");
        }
        seen.clear();
        for (name, _) in &self.assets {
            ensure!(
                valid_name(name, ".png"),
                "unsafe or non-.png asset name {name:?}"
            );
            ensure!(seen.insert(name.as_str()), "duplicate asset {name:?}");
        }
        let total: usize = self.files.iter().map(|(_, s)| s.len()).sum::<usize>()
            + self.assets.iter().map(|(_, b)| b.len()).sum::<usize>();
        ensure!(
            total <= MAX_PACK_BYTES,
            "toy is larger than {MAX_PACK_BYTES} bytes"
        );
        Ok(())
    }
}

/// Load a toy from a `.lua` file, a toy directory, or a `.tg16.json` pack.
pub fn load_toy(path: &Path) -> Result<Toy> {
    if path.is_dir() {
        return Toy::from_dir(path);
    }
    let name = file_name(path);
    if name.ends_with(".json") {
        let text =
            fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
        return Toy::from_pack_json(&text).with_context(|| format!("in {}", path.display()));
    }
    let src =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let stem = path
        .file_stem()
        .map_or("toy".into(), |s| s.to_string_lossy().into_owned());
    Ok(Toy {
        name: stem,
        files: vec![(name, src)],
        assets: Vec::new(),
    })
}

/// Lua files of a toy (see [`load_toy`]) in load order.
pub fn load_sources(path: &Path) -> Result<Vec<(String, String)>> {
    Ok(load_toy(path)?.files)
}

/// Pack a toy directory (or `.lua` file) into `.tg16.json` text.
pub fn pack(path: &Path) -> Result<String> {
    load_toy(path)?.to_pack_json()
}

/// Unpack `.tg16.json` text into a toy directory.
pub fn unpack(text: &str, dir: &Path) -> Result<()> {
    Toy::from_pack_json(text)?.write_dir(dir)
}

/// A loaded engine for `toy`: images registered, then its Lua run.
pub fn engine_from_toy(toy: &Toy) -> Result<Engine> {
    let mut e = Engine::new();
    for (name, bytes) in &toy.assets {
        let (w, h, rgba) = decode_png(bytes).with_context(|| format!("assets/{name}"))?;
        let stem = name.strip_suffix(".png").unwrap_or(name);
        e.add_image(stem, w, h, rgba);
    }
    let refs: Vec<(&str, &str)> = toy
        .files
        .iter()
        .map(|(n, s)| (n.as_str(), s.as_str()))
        .collect();
    if let Err(err) = e.load(&refs) {
        bail!("{err}");
    }
    Ok(e)
}

fn engine(path: &Path) -> Result<Engine> {
    engine_from_toy(&load_toy(path)?)
}

/// Render the requested frames (playing every frame from zero) to PNGs.
pub fn render(path: &Path, frames: &[u32], out: &Path) -> Result<()> {
    ensure!(!frames.is_empty(), "at least one sample time is required");
    let mut e = engine(path)?;
    let wanted: std::collections::BTreeSet<_> = frames.iter().copied().collect();
    fs::create_dir_all(out)?;
    for f in 0..=*wanted.last().unwrap() {
        let t = f64::from(f) / 60.0;
        let o = e
            .render(t, f)
            .map_err(|err| anyhow::anyhow!("frame {f}: {err}"))?;
        if wanted.contains(&f) {
            let output = out.join(format!("frame-{f:08}.png"));
            write_png(&output, o.width as u32, o.height as u32, &o.rgba)?;
            println!("{} ({t:.6}s)", output.display());
            if o.overflow() {
                eprintln!("warning: more than 16 sprite units on a line at frame {f}");
            }
        }
    }
    for w in e.warnings() {
        eprintln!("warning: {w}");
    }
    Ok(())
}

/// Play `duration` seconds, failing on Lua errors, and report the sprite
/// load and the estimated CPU and VRAM cost against the console's limits.
pub fn check(path: &Path, duration: f64, allow_overflow: bool) -> Result<()> {
    let end = crate::frame_number(duration)?;
    ensure!(end > 0, "duration must be at least one frame");
    let mut e = engine(path)?;
    let mut peak = tg16_core::Budget::default();
    let (mut peak_cpu_at, mut overflow_frames, mut over_budget) = (0u32, Vec::new(), Vec::new());
    for f in 0..=end {
        let o = e
            .render(f64::from(f) / 60.0, f)
            .map_err(|err| anyhow::anyhow!("frame {f}: {err}"))?;
        let b = e.budget();
        if b.cycles > peak.cycles {
            peak.cycles = b.cycles;
            peak_cpu_at = f;
        }
        peak.peak_line_units = peak.peak_line_units.max(b.peak_line_units);
        peak.vram_words = peak.vram_words.max(b.vram_words);
        peak.raster_lines = peak.raster_lines.max(b.raster_lines);
        peak.sprites = peak.sprites.max(b.sprites);
        peak.software = peak.software.max(b.software);
        if o.overflow() {
            overflow_frames.push(f);
        }
        let problems: Vec<_> = b
            .problems()
            .into_iter()
            .filter(|p| !p.starts_with("sprites dropped"))
            .collect();
        if !problems.is_empty() {
            over_budget.push((f, problems));
        }
    }
    for w in e.warnings() {
        eprintln!("warning: {w}");
    }
    println!("ok: {} frames played", end + 1);
    println!(
        "  CPU (estimate): peak {:.0}% of a frame at frame {peak_cpu_at}",
        peak.cpu_share() * 100.0
    );
    println!("  raster lines:   up to {} per frame", peak.raster_lines);
    if peak.software > 0 {
        println!("  software (cpu): up to {} cycles per frame", peak.software);
    }
    println!(
        "  sprites:        up to {}/64, busiest line {}/16 units",
        peak.sprites, peak.peak_line_units
    );
    println!(
        "  VRAM per frame: up to {} words (about {} fit in vertical blank)",
        peak.vram_words,
        tg16_core::budget::VBLANK_VRAM_WORDS
    );
    if let Some((f, problems)) = over_budget.first() {
        eprintln!(
            "warning: over budget on {} frames; first at frame {f}: {}",
            over_budget.len(),
            problems.join("; ")
        );
    }
    if !overflow_frames.is_empty() {
        let msg = format!(
            "sprite line limit exceeded on {} frames, first at frame {}",
            overflow_frames.len(),
            overflow_frames[0]
        );
        ensure!(
            allow_overflow,
            "{msg} (pass --allow-overflow to accept the dropout)"
        );
        eprintln!("warning: {msg}");
    }
    Ok(())
}

const STARTER: &str = include_str!("../../tg16-core/toys/sunset.lua");

/// Create a TG16 toy directory with a starter `main.lua` and an empty
/// `assets/` directory for PNGs.
pub fn new_project(dir: &Path) -> Result<()> {
    ensure!(!dir.exists(), "{} already exists", dir.display());
    fs::create_dir_all(dir)?;
    fs::write(dir.join("main.lua"), STARTER)?;
    fs::create_dir_all(dir.join("assets"))?;
    Ok(())
}

/// Play `duration` seconds and write the PSG output as a 16-bit stereo WAV.
pub fn audio(path: &Path, duration: f64, out: &Path) -> Result<()> {
    let end = crate::frame_number(duration)?;
    ensure!(end > 0, "duration must be at least one frame");
    let mut e = engine(path)?;
    let mut samples: Vec<i16> = Vec::new();
    for f in 0..end {
        e.frame(f64::from(f) / 60.0, f)
            .map_err(|err| anyhow::anyhow!("frame {f}: {err}"))?;
        samples.extend_from_slice(e.audio());
    }
    let rate = tg16_core::SAMPLE_RATE;
    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&2u16.to_le_bytes()); // stereo
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    fs::write(out, wav).with_context(|| format!("cannot write {}", out.display()))?;
    println!("{} ({duration}s)", out.display());
    Ok(())
}

/// Record `duration` seconds of a toy and write a HuCard ROM (`.pce`) that
/// replays it on a PC Engine / TurboGrafx-16. Returns the build stats; prints
/// a summary and warnings for frames that will not keep up on hardware.
pub fn rom(path: &Path, duration: f64, looping: bool, out: &Path) -> Result<tg16_core::RomStats> {
    let frames = crate::frame_number(duration)?;
    ensure!(frames > 0, "duration must be at least one frame");
    let mut e = engine(path)?;
    let rom = tg16_core::rom::export(&mut e, &tg16_core::RomOptions { frames, looping })
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    for w in e.warnings() {
        eprintln!("warning: {w}");
    }
    fs::write(out, &rom.bytes).with_context(|| format!("cannot write {}", out.display()))?;
    let s = &rom.stats;
    println!(
        "{} ({} frames, {}; {} KB ROM, stream {} bytes)",
        out.display(),
        s.frames,
        if looping { "looping" } else { "then holds" },
        s.rom_bytes / 1024,
        s.stream_bytes
    );
    println!(
        "  vertical blank: busiest frame {} needs ~{} of {} cycles; up to {} VRAM words",
        s.peak_vblank_frame, s.peak_vblank_cycles, s.vblank_cycles, s.peak_vram_words
    );
    println!("  raster events:  up to {} per frame", s.peak_raster_events);
    for w in rom.warnings() {
        eprintln!("warning: {w}");
    }
    Ok(rom.stats)
}
