//! `ppu tg16 rom` against an emulator: build ROMs, play them in Mednafen
//! (its accurate `pce` module), and compare the frames it shows with the
//! model's render, pixel by pixel after reducing both to the VCE's 3-bit
//! channels.
//!
//! Skipped, with a note, unless `mednafen` and `ffmpeg` are installed (and
//! when `PPU_TG16_SKIP_EMU` is set). Mednafen runs headless through SDL's
//! dummy video driver, records a QuickTime movie with PNG frames, and
//! `ffmpeg` splits it. Set `PPU_TG16_ROM_OUT=<dir>` to keep the ROMs, sample
//! captures, model renders, diff images and a report there.
//!
//! The comparison finds how many frames the ROM takes to boot (the frame
//! offset) as the one that best fits the whole recording, then holds it for
//! every frame. The capture is
//! cropped at row 11: with the exporter's 224-line timing (VDS 23) and
//! `pce.slstart 0`, Mednafen's first displayed line is there.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use ppu_cli::tg16::{engine_from_toy, load_toy};

const W: usize = 256;
const H: usize = 224;
/// Mednafen frame row of display line 0 (see the module comment).
const CAPTURE_TOP: usize = 11;

fn tool(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain([PathBuf::from("/usr/games")])
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 3-bit channels, the VCE's resolution, from 8-bit RGB.
fn q(c: u8) -> u8 {
    ((c as u32 * 7 + 127) / 255) as u8
}

type Picture = Vec<[u8; 3]>; // W * H, quantized

fn quantize(rgb: impl Iterator<Item = [u8; 3]>) -> Picture {
    rgb.map(|p| [q(p[0]), q(p[1]), q(p[2])]).collect()
}

/// Play `rom` in Mednafen and return its first `frames` frames, cropped to
/// the display and quantized.
fn capture(mednafen: &Path, ffmpeg: &Path, rom: &Path, work: &Path, frames: usize) -> Vec<Picture> {
    let home = work.join("mednafen-home");
    std::fs::create_dir_all(&home).unwrap();
    let movie = work.join("capture.mov");
    let _ = std::fs::remove_file(&movie);
    // Mednafen runs unthrottled (about 200 frames a second here); wait
    // longer if the movie comes up short.
    let mut wait_ms = 1500;
    loop {
        let mut child = Command::new(mednafen)
            .env("MEDNAFEN_HOME", &home)
            .env("SDL_VIDEODRIVER", "dummy")
            .args(["-force_module", "pce", "-sound", "0", "-nothrottle", "1"])
            .args(["-video.driver", "softfb", "-qtrecord.vcodec", "png"])
            .args(["-qtrecord.h_double_threshold", "0"])
            .args(["-qtrecord.w_double_threshold", "0"])
            .args(["-pce.slstart", "0", "-pce.slend", "239"])
            .arg("-qtrecord")
            .arg(&movie)
            .arg(rom)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start mednafen");
        std::thread::sleep(Duration::from_millis(wait_ms));
        // SIGTERM lets Mednafen finish the movie file.
        let _ = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status();
        child.wait().unwrap();
        // ffmpeg crops to the display, drops the horizontal oversampling
        // (Mednafen draws 256-dot lines 1024 wide) and writes raw RGB.
        let raw = Command::new(ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(&movie)
            .args([
                "-vf",
                &format!(
                    "select=lt(n\\,{frames}),crop=iw:{H}:0:{CAPTURE_TOP},scale={W}:{H}:flags=neighbor"
                ),
                "-vsync",
                "0",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-",
            ])
            .output()
            .expect("run ffmpeg");
        assert!(
            raw.status.success(),
            "ffmpeg could not read {}",
            movie.display()
        );
        let out: Vec<Picture> = raw
            .stdout
            .chunks_exact(W * H * 3)
            .map(|f| quantize(f.chunks_exact(3).map(|p| [p[0], p[1], p[2]])))
            .collect();
        if out.len() >= frames || wait_ms >= 12_000 {
            assert!(out.len() >= frames, "Mednafen gave {} frames", out.len());
            return out;
        }
        wait_ms *= 2;
    }
}

fn mismatch(a: &Picture, b: &Picture) -> f64 {
    a.iter().zip(b).filter(|(x, y)| x != y).count() as f64 / (W * H) as f64
}

fn write_png(path: &Path, pic: &Picture) {
    let rgba: Vec<u8> = pic
        .iter()
        .flat_map(|p| {
            let e = |v: u8| (v << 5) | (v << 2) | (v >> 1);
            [e(p[0]), e(p[1]), e(p[2]), 255]
        })
        .collect();
    let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut enc = png::Encoder::new(file, W as u32, H as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
}

/// Model pixels in red where `a` and `b` differ, dimmed elsewhere.
fn diff_png(path: &Path, a: &Picture, b: &Picture) {
    let pic: Picture = a
        .iter()
        .zip(b)
        .map(|(x, y)| if x != y { [7, 0, 0] } else { x.map(|v| v / 3) })
        .collect();
    write_png(path, &pic);
}

struct Case {
    name: &'static str,
    toy: PathBuf,
    seconds: f64,
    looping: bool,
    /// Frames to compare (past the recording, to check loop or hold).
    check: usize,
    max_mean: f64,
    max_frame: f64,
}

struct Outcome {
    offset: usize,
    mean: f64,
    worst: (f64, usize),
}

fn run_case(c: &Case, mednafen: &Path, ffmpeg: &Path, work: &Path, keep: Option<&Path>) -> Outcome {
    let rom = work.join(format!("{}.pce", c.name));
    let stats = ppu_cli::tg16::rom(&c.toy, c.seconds, c.looping, &rom).unwrap();
    let recorded = stats.frames as usize;

    // Model frames 0..recorded.
    let toy = load_toy(&c.toy).unwrap();
    let mut e = engine_from_toy(&toy).unwrap();
    let model: Vec<Picture> = (0..recorded as u32)
        .map(|f| {
            let o = e.render(f64::from(f) / 60.0, f).unwrap();
            quantize(o.rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2]]))
        })
        .collect();
    let expected = |i: usize| -> &Picture {
        if i < recorded {
            &model[i]
        } else if c.looping {
            // The loop replays frame 0, then carries on from frame 1.
            &model[(i - recorded) % recorded]
        } else {
            &model[recorded - 1]
        }
    };

    const MAX_BOOT: usize = 20;
    let caps = capture(mednafen, ffmpeg, &rom, work, c.check + MAX_BOOT);
    // Many frames repeat (slow scrolls), so score each candidate boot offset
    // over the whole recording, not one frame.
    let offset_scores: Vec<f64> = (0..MAX_BOOT)
        .map(|k| {
            (0..recorded)
                .map(|i| mismatch(&caps[i + k], &model[i]))
                .sum()
        })
        .collect();
    let offset = (0..MAX_BOOT)
        .min_by(|&a, &b| offset_scores[a].total_cmp(&offset_scores[b]))
        .unwrap();
    let probe = recorded / 2;
    let scores: Vec<f64> = (0..c.check)
        .map(|i| mismatch(&caps[i + offset], expected(i)))
        .collect();
    let mean = scores.iter().sum::<f64>() / scores.len() as f64;
    let worst = scores
        .iter()
        .enumerate()
        .map(|(i, &s)| (s, i))
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap();

    if let Some(dir) = keep {
        std::fs::copy(&rom, dir.join(format!("{}.pce", c.name))).unwrap();
        for i in [probe, worst.1, c.check - 1] {
            let base = dir.join(format!("{}-frame{i:04}", c.name));
            write_png(&base.with_extension("emu.png"), &caps[i + offset]);
            write_png(&base.with_extension("model.png"), expected(i));
            diff_png(
                &base.with_extension("diff.png"),
                expected(i),
                &caps[i + offset],
            );
        }
    }
    Outcome {
        offset,
        mean,
        worst,
    }
}

#[test]
fn roms_match_the_model_in_mednafen() {
    if std::env::var_os("PPU_TG16_SKIP_EMU").is_some() {
        eprintln!("skipped: PPU_TG16_SKIP_EMU is set");
        return;
    }
    let (Some(mednafen), Some(ffmpeg)) = (tool("mednafen"), tool("ffmpeg")) else {
        eprintln!("skipped: needs mednafen and ffmpeg");
        return;
    };
    let toys = root().join("crates/tg16-core/toys");
    let cases = [
        Case {
            name: "rom_timing",
            toy: root().join("crates/ppu-cli/tests/fixtures/tg16/rom_timing.lua"),
            seconds: 1.5,
            looping: false,
            check: 100,
            max_mean: 0.0005,
            max_frame: 0.001,
        },
        Case {
            name: "sunset",
            toy: toys.join("sunset.lua"),
            seconds: 1.0,
            looping: true,
            check: 150,
            max_mean: 0.003,
            max_frame: 0.006,
        },
        Case {
            name: "kit_demo",
            toy: toys.join("kit_demo.lua"),
            seconds: 1.0,
            looping: false,
            check: 100,
            max_mean: 0.004,
            max_frame: 0.008,
        },
    ];
    let work = tempfile::tempdir().unwrap();
    let keep = std::env::var_os("PPU_TG16_ROM_OUT").map(PathBuf::from);
    if let Some(k) = &keep {
        std::fs::create_dir_all(k).unwrap();
    }
    let mut report = String::new();
    let mut failures = Vec::new();
    for c in &cases {
        let o = run_case(c, &mednafen, &ffmpeg, work.path(), keep.as_deref());
        let line = format!(
            "{}: {} frames compared ({}), boot offset {} frames, mismatch mean {:.3}%, \
             worst {:.3}% at frame {}",
            c.name,
            c.check,
            if c.looping { "loops" } else { "holds" },
            o.offset,
            o.mean * 100.0,
            o.worst.0 * 100.0,
            o.worst.1
        );
        eprintln!("{line}");
        report += &line;
        report.push('\n');
        if o.mean > c.max_mean || o.worst.0 > c.max_frame {
            failures.push(line);
        }
    }
    if let Some(k) = &keep {
        std::fs::write(k.join("report.txt"), &report).unwrap();
    }
    assert!(
        failures.is_empty(),
        "ROM output differs from the model:\n{}",
        failures.join("\n")
    );
}
