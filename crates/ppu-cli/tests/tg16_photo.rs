//! The `photo` example toy: a real PNG with thousands of colors, reduced by
//! the importer and scrolled with raster effects. Golden-tested so changes to
//! the reduction show up as a diff.
use std::path::{Path, PathBuf};
use std::process::Command;

use ppu_cli::tg16::{decode_png, engine_from_toy, Toy};

const GOLDEN: &str = "tests/fixtures/tg16_photo_f90.png";

fn toy_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tg16-core/toys/photo")
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
    out
}

/// Frame 90 (1.5 s in: scrolled, with the lake ripple) and the warnings.
fn render_f90() -> (u32, u32, Vec<u8>, Vec<String>) {
    let toy = Toy::from_dir(&toy_dir()).unwrap();
    let mut e = engine_from_toy(&toy).unwrap();
    let mut last = None;
    for f in 0..=90 {
        last = Some(e.render(f64::from(f) / 60.0, f).unwrap());
    }
    let o = last.unwrap();
    (o.width as u32, o.height as u32, o.rgba, e.warnings())
}

#[test]
fn photo_toy_matches_golden() {
    let (w, h, rgba, warnings) = render_f90();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("image_bg(\"lake\"): its ")
            && warnings[0].contains("so they were reduced (ordered dither)"),
        "{warnings:?}"
    );
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, encode_png(w, h, &rgba)).unwrap();
    }
    let want = std::fs::read(&golden)
        .expect("golden missing: run with UPDATE_GOLDEN=1 cargo test -p ppu-cli --test tg16_photo");
    assert_eq!(
        decode_png(&want).unwrap(),
        (w, h, rgba),
        "frame 90 differs from {GOLDEN} (UPDATE_GOLDEN=1 regenerates it)"
    );
}

#[test]
fn photo_png_is_small_and_too_colorful_to_import_exactly() {
    let bytes = std::fs::read(toy_dir().join("assets/lake.png")).unwrap();
    assert!(bytes.len() < 100 * 1024, "{} bytes", bytes.len());
    let (w, h, rgba) = decode_png(&bytes).unwrap();
    assert_eq!((w, h), (512, 224));
    let img = tg16_core::Image::new(w, h, rgba).unwrap();
    assert!(tg16_core::import_bg(&img, 16, 1920).is_err());
    let b = tg16_core::import_bg_with(&img, 16, 1920, Some(tg16_core::Dither::Ordered)).unwrap();
    assert!(b.quality.colors_reduced && b.palettes.len() <= 16);
    assert!(b.quality.error < 4.0, "{:?}", b.quality);
}

#[test]
fn check_reports_the_reduction() {
    let o = Command::new(env!("CARGO_BIN_EXE_ppu"))
        .args(["tg16", "check"])
        .arg(toy_dir())
        .args(["--duration", "0.1"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("warning: image_bg(\"lake\")"), "{err}");
    assert!(err.contains("mean color error"), "{err}");
}
