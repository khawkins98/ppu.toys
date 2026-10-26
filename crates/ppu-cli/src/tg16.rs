//! `ppu tg16 ...`: author TurboGrafx-16 toys with the same loop as SNES ones.

use anyhow::{bail, ensure, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use tg16_core::Engine;

use crate::author::write_png;

/// Lua files of a toy: a single `.lua` file, or every `.lua` file in a
/// directory in name order with `main.lua` last.
pub fn load_sources(path: &Path) -> Result<Vec<(String, String)>> {
    let mut files: Vec<PathBuf> = if path.is_dir() {
        fs::read_dir(path)
            .with_context(|| format!("cannot read {}", path.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "lua"))
            .collect()
    } else {
        vec![path.to_path_buf()]
    };
    ensure!(!files.is_empty(), "{} has no .lua files", path.display());
    files.sort_by_key(|p| {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        (name == "main.lua", name)
    });
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let src =
                fs::read_to_string(&p).with_context(|| format!("cannot read {}", p.display()))?;
            Ok((name, src))
        })
        .collect()
}

fn engine(path: &Path) -> Result<Engine> {
    let sources = load_sources(path)?;
    let refs: Vec<(&str, &str)> = sources
        .iter()
        .map(|(n, s)| (n.as_str(), s.as_str()))
        .collect();
    let mut e = Engine::new();
    if let Err(err) = e.load(&refs) {
        bail!("{err}");
    }
    Ok(e)
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

/// Play `duration` seconds, failing on Lua errors, and report the sprite load.
pub fn check(path: &Path, duration: f64, allow_overflow: bool) -> Result<()> {
    let end = crate::frame_number(duration)?;
    ensure!(end > 0, "duration must be at least one frame");
    let mut e = engine(path)?;
    let (mut busiest, mut busiest_at, mut overflow_frames) = (0u8, 0u32, Vec::new());
    for f in 0..=end {
        let o = e
            .render(f64::from(f) / 60.0, f)
            .map_err(|err| anyhow::anyhow!("frame {f}: {err}"))?;
        let peak = o.lines.iter().map(|l| l.units).max().unwrap_or(0);
        if peak > busiest {
            busiest = peak;
            busiest_at = f;
        }
        if o.overflow() {
            overflow_frames.push(f);
        }
    }
    let warnings = e.warnings();
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    println!(
        "ok: {} frames, busiest line wants {busiest}/16 sprite units (frame {busiest_at})",
        end + 1
    );
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

/// Create a TG16 toy directory with a starter `main.lua`.
pub fn new_project(dir: &Path) -> Result<()> {
    ensure!(!dir.exists(), "{} already exists", dir.display());
    fs::create_dir_all(dir)?;
    fs::write(dir.join("main.lua"), STARTER)?;
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
