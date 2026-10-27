//! `ppu tg16 convert`: SNES toys become TG16 toys that run their Lua through
//! the compatibility layer.

use ppu_cli::tg16::{engine_from_toy, Toy};
use ppu_cli::tg16_convert::{convert, ConvertOptions};
use std::fs;
use std::path::Path;

/// A tiny SNES toy written with raw VRAM pokes: a red stripe on BG1 (map row
/// 5, so lines 40..47 with scroll.y = -1), a blue backdrop that turns yellow
/// on line 10 only, and an 8x8 green sprite at (100, 100).
const TOY: &str = r#"
function init()
  mode = 1
  for r = 0, 7 do vram[0x1000 + 16 + r] = 0x00ff end
  for x = 0, 31 do vram[5 * 32 + x] = 1 end
  bg[1].char_base, bg[1].map_base = 0x1000, 0
  for r = 0, 7 do vram[0x4000 + r] = 0xff00 end
  obj.char_base, obj.size_sel = 0x4000, 0
  screen.main.bg1, screen.main.obj = true, true
  cgram[0] = rgb(0, 0, 255)
  cgram[1] = rgb(255, 0, 0)
  cgram[130] = rgb(0, 255, 0)
end

function frame(t, f)
  bg[1].scroll.y = -1
  bg[1].scroll.x = f
  local o = obj[0]
  o.on, o.x, o.y, o.tile, o.flip_x = true, 100, 100, 0, f % 2 == 1
  hdma(10, 10, function(y) cgram[0] = rgb(255, 255, 0) end)
end
"#;

fn snes_toy(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("ppu.json"),
        r#"{"title":"Stripe","files":["main.lua"],"sources":[]}"#,
    )
    .unwrap();
    fs::write(dir.join("main.lua"), TOY).unwrap();
}

fn px(rgba: &[u8], width: usize, x: usize, y: usize) -> [u8; 3] {
    let i = (y * width + x) * 4;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

#[test]
fn converts_layers_palettes_and_sprites() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("snes");
    let out = tmp.path().join("tg16");
    snes_toy(&src);
    let report = convert(
        &src,
        &out,
        &ConvertOptions {
            duration: 0.1,
            compare: vec![],
        },
    )
    .unwrap();
    assert!(report.contains("Conversion report: Stripe"), "{report}");
    for f in [
        "00_snes_data.lua",
        "01_snes_shim.lua",
        "10_main.lua",
        "main.lua",
        "CONVERT.md",
    ] {
        assert!(out.join(f).is_file(), "missing {f}");
    }
    assert_eq!(fs::read_to_string(out.join("10_main.lua")).unwrap(), TOY);

    let toy = Toy::from_dir(&out).unwrap();
    let mut e = engine_from_toy(&toy).unwrap();
    for f in 0..2u32 {
        let o = e.render(f64::from(f) / 60.0, f).unwrap();
        let w = o.width;
        let (blue, red, yellow, green) = ([0, 0, 255], [255, 0, 0], [255, 255, 0], [0, 255, 0]);
        assert_eq!(px(&o.rgba, w, 5, 9), blue, "frame {f}: backdrop");
        assert_eq!(
            px(&o.rgba, w, 5, 10),
            yellow,
            "frame {f}: hdma colour on its line"
        );
        assert_eq!(
            px(&o.rgba, w, 5, 11),
            blue,
            "frame {f}: hdma colour lasts one line"
        );
        assert_eq!(px(&o.rgba, w, 5, 39), blue, "frame {f}: above the stripe");
        assert_eq!(px(&o.rgba, w, 5, 40), red, "frame {f}: stripe top");
        assert_eq!(px(&o.rgba, w, 5, 47), red, "frame {f}: stripe bottom");
        assert_eq!(px(&o.rgba, w, 5, 48), blue, "frame {f}: below the stripe");
        // An 8x8 sprite in a 16x16 pattern stays in place when flipped.
        assert_eq!(px(&o.rgba, w, 100, 100), green, "frame {f}: sprite");
        assert_eq!(px(&o.rgba, w, 107, 107), green, "frame {f}: sprite corner");
        assert_eq!(
            px(&o.rgba, w, 108, 100),
            blue,
            "frame {f}: right of the sprite"
        );
        assert_eq!(
            px(&o.rgba, w, 99, 100),
            blue,
            "frame {f}: left of the sprite"
        );
    }
}

#[test]
fn converts_the_bundled_mirror_example() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/skills/creating-demos/examples/hdma");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("mirror");
    let report = convert(
        &example,
        &out,
        &ConvertOptions {
            duration: 0.5,
            compare: vec![0],
        },
    )
    .unwrap();
    assert!(report.contains("Colour math"), "{report}");
    assert!(report.contains("BG1"), "{report}");
    assert!(out.join("compare/frame-00000.png").is_file());
    // The converted toy is an ordinary TG16 toy: it packs and plays.
    let toy = Toy::from_dir(&out).unwrap();
    let pack = toy.to_pack_json().unwrap();
    let back = Toy::from_pack_json(&pack).unwrap();
    let mut e = engine_from_toy(&back).unwrap();
    let o = e.render(0.0, 0).unwrap();
    let first = px(&o.rgba, o.width, 0, 0);
    assert!(
        (0..o.height).any(|y| px(&o.rgba, o.width, 128, y) != first),
        "converted frame is blank"
    );
}

#[test]
fn refuses_a_non_empty_output_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("snes");
    snes_toy(&src);
    let out = tmp.path().join("busy");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("keep.txt"), "x").unwrap();
    let err = convert(
        &src,
        &out,
        &ConvertOptions {
            duration: 0.1,
            compare: vec![],
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("not empty"), "{err}");
}
