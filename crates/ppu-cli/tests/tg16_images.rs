//! `ppu tg16`: PNGs in a toy's assets/ directory become images for Lua.
use std::{fs, process::Command};

use ppu_cli::tg16::decode_png;

fn encode(w: u32, h: u32, color: png::ColorType, depth: png::BitDepth, data: &[u8]) -> Vec<u8> {
    encode_with(w, h, color, depth, data, None, None)
}

fn encode_with(
    w: u32,
    h: u32,
    color: png::ColorType,
    depth: png::BitDepth,
    data: &[u8],
    palette: Option<&[u8]>,
    trns: Option<&[u8]>,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(color);
    e.set_depth(depth);
    if let Some(p) = palette {
        e.set_palette(p.to_vec());
    }
    if let Some(t) = trns {
        e.set_trns(t.to_vec());
    }
    e.write_header().unwrap().write_image_data(data).unwrap();
    out
}

#[test]
fn decodes_every_color_type_to_rgba() {
    use png::{BitDepth::*, ColorType::*};
    let rgba = decode_png(&encode(1, 1, Rgba, Eight, &[1, 2, 3, 4])).unwrap();
    assert_eq!(rgba, (1, 1, vec![1, 2, 3, 4]));
    let rgb = decode_png(&encode(1, 1, Rgb, Eight, &[1, 2, 3])).unwrap();
    assert_eq!(rgb.2, vec![1, 2, 3, 255]);
    let gray = decode_png(&encode(1, 1, Grayscale, Eight, &[9])).unwrap();
    assert_eq!(gray.2, vec![9, 9, 9, 255]);
    let ga = decode_png(&encode(1, 1, GrayscaleAlpha, Eight, &[9, 7])).unwrap();
    assert_eq!(ga.2, vec![9, 9, 9, 7]);
    let wide = decode_png(&encode(1, 1, Rgb, Sixteen, &[1, 0, 2, 0, 3, 0])).unwrap();
    assert_eq!(wide.2, vec![1, 2, 3, 255]);
    // 2-bit indexed, two pixels, the second transparent via tRNS.
    let pal = [10, 20, 30, 40, 50, 60];
    let idx = decode_png(&encode_with(
        2,
        1,
        Indexed,
        Two,
        &[0b0001_0000],
        Some(&pal),
        Some(&[255, 0]),
    ))
    .unwrap();
    assert_eq!(idx, (2, 1, vec![10, 20, 30, 255, 40, 50, 60, 0]));
    assert!(decode_png(b"not a png").is_err());
}

#[test]
fn toy_directory_assets_reach_lua() {
    let tmp = tempfile::tempdir().unwrap();
    let toy = tmp.path().join("toy");
    fs::create_dir_all(toy.join("assets")).unwrap();
    // An 8x8 solid red background as an indexed PNG.
    let png = encode_with(
        8,
        8,
        png::ColorType::Indexed,
        png::BitDepth::Eight,
        &[0; 64],
        Some(&[255, 0, 0]),
        None,
    );
    fs::write(toy.join("assets/hills.png"), png).unwrap();
    fs::write(
        toy.join("main.lua"),
        "function init()\n  local b = image_bg(\"hills\")\n  assert(b.tiles == 1)\nend\n",
    )
    .unwrap();
    let out = tmp.path().join("out");
    let o = Command::new(env!("CARGO_BIN_EXE_ppu"))
        .args(["tg16", "render"])
        .arg(&toy)
        .args(["--at", "0", "-o"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let frame = fs::read(out.join("frame-00000000.png")).unwrap();
    let (_, _, rgba) = decode_png(&frame).unwrap();
    assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
    assert_eq!(
        &rgba[8 * 4..8 * 4 + 4],
        &[0, 0, 0, 255],
        "pixel 8 is outside"
    );

    // A missing name lists what is there.
    fs::write(
        toy.join("main.lua"),
        "function init() image_bg(\"sky\") end\n",
    )
    .unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_ppu"))
        .args(["tg16", "check"])
        .arg(&toy)
        .output()
        .unwrap();
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("known images: \"hills\""), "{err}");
}
