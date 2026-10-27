//! PNG importers: background and sprite import, and the Lua functions.
use tg16_core::*;

const CLEAR: [u8; 4] = [0, 0, 0, 0];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// An image built from a function of the pixel position.
fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&f(x, y));
        }
    }
    Image::new(w, h, rgba).unwrap()
}

/// A distinct 9-bit-exact color for each `i` in 0..512.
fn level(i: u32) -> [u8; 4] {
    let c = to_rgb8(i as u16);
    [c[0], c[1], c[2], 255]
}

#[test]
fn bg_dedupes_identical_tiles() {
    // A 4x2 tile checkerboard of two solid tiles.
    let img = image(
        32,
        16,
        |x, y| {
            if (x / 8 + y / 8) % 2 == 0 {
                RED
            } else {
                BLUE
            }
        },
    );
    let b = import_bg(&img, 16, 2048).unwrap();
    assert_eq!((b.width, b.height), (4, 2));
    assert_eq!(b.palettes.len(), 1);
    assert_eq!(b.tiles.len(), 2);
    assert_eq!(b.map[0].0, b.map[2].0);
    assert_eq!(b.map[0].0, b.map[5].0);
    assert_ne!(b.map[0].0, b.map[1].0);
}

#[test]
fn bg_shares_tiles_across_palettes() {
    // Two tiles of 15 colors each with no colors in common, laid out in the
    // same index order: two palettes, one tile.
    let img = image(16, 8, |x, y| {
        let i = (y * 8 + x % 8) % 15;
        level(if x < 8 { i + 1 } else { i + 100 })
    });
    let b = import_bg(&img, 16, 2048).unwrap();
    assert_eq!(b.palettes.len(), 2);
    assert_eq!(b.tiles.len(), 1);
    assert_ne!(b.map[0].1, b.map[1].1);
}

#[test]
fn bg_palette_assignment_merges_when_it_fits() {
    // Tile 0 uses colors 1..10, tile 1 uses 6..15, tile 2 uses 20..30:
    // tiles 0 and 1 share one 15-color palette, tile 2 needs another.
    let ranges = [(1, 10), (6, 15), (20, 30)];
    let img = image(24, 8, |x, y| {
        let (lo, hi) = ranges[(x / 8) as usize];
        level(lo + (y * 8 + x % 8) % (hi - lo + 1))
    });
    let b = import_bg(&img, 16, 2048).unwrap();
    assert_eq!(b.palettes.len(), 2);
    assert_eq!(b.map[0].1, b.map[1].1);
    assert_ne!(b.map[0].1, b.map[2].1);
    for p in &b.palettes {
        assert_eq!(p[0], 0, "color 0 is left transparent");
    }
}

#[test]
fn transparency_maps_to_index_zero() {
    let img = image(8, 8, |x, _| {
        if x < 4 {
            [255, 255, 255, 127]
        } else {
            [255, 255, 255, 128]
        }
    });
    let b = import_bg(&img, 1, 2048).unwrap();
    assert_eq!(b.tiles[0][0], 0);
    assert_eq!(b.tiles[0][3], 0);
    assert_eq!(b.tiles[0][4], 1);
    assert_eq!(b.palettes[0][1], rgb8(255, 255, 255));
}

#[test]
fn colors_round_to_nine_bits_before_counting() {
    // 64 slightly different shades of red that round to only a few levels.
    let img = image(8, 8, |x, y| [200 + (y * 8 + x) as u8 / 4, 0, 0, 255]);
    let b = import_bg(&img, 1, 2048).unwrap();
    let used = b.palettes[0].iter().filter(|&&c| c != 0).count();
    assert!(used <= 2, "{:?}", b.palettes[0]);
}

#[test]
fn bg_errors_say_what_does_not_fit() {
    let e = import_bg(&image(12, 8, |_, _| RED), 16, 2048).unwrap_err();
    assert!(e.0.contains("multiple of 8"), "{e}");

    // 16 colors in the tile at pixel (8, 0).
    let img16 = image(16, 8, |x, y| {
        if x < 8 {
            RED
        } else {
            level(((y * 8 + x % 8) % 16) + 1)
        }
    });
    let e = import_bg(&img16, 16, 2048).unwrap_err();
    assert!(e.0.contains("tile at pixel (8, 0) has 16 colors"), "{e}");

    // Two disjoint 15-color tiles but only one palette allowed.
    let img = image(16, 8, |x, y| {
        let i = (y * 8 + x % 8) % 15;
        level(if x < 8 { i + 1 } else { i + 100 })
    });
    let e = import_bg(&img, 1, 2048).unwrap_err();
    assert!(e.0.contains("more than 1 palette"), "{e}");

    // Four unique tiles, room for three.
    let img = image(32, 8, |x, _| level(x / 8 + 1));
    let e = import_bg(&img, 16, 3).unwrap_err();
    assert!(e.0.contains("4 unique tiles but only 3 fit"), "{e}");
}

#[test]
fn sprites_cut_cells_in_row_major_order() {
    // A 32x32 sheet of four 16x16 cells: red, green / blue, white.
    let img = image(32, 32, |x, y| match (x / 16, y / 16) {
        (0, 0) => RED,
        (1, 0) => GREEN,
        (0, 1) => BLUE,
        _ => WHITE,
    });
    let s = import_sprites(&img, 16, 16, 1).unwrap();
    assert_eq!(s.patterns.len(), 4);
    assert_eq!(s.frames(), 4);
    assert_eq!(s.palettes.len(), 1);
    let color = |p: usize| s.palettes[0][s.patterns[p][0] as usize];
    assert_eq!(color(0), rgb8(255, 0, 0));
    assert_eq!(color(1), rgb8(0, 255, 0));
    assert_eq!(color(2), rgb8(0, 0, 255));
    assert_eq!(color(3), rgb8(255, 255, 255));
}

#[test]
fn sprite_frames_use_hardware_cell_order() {
    // Two 32x32 frames side by side on a 64x32 sheet. Within a frame the
    // cells go left, right, then the next row, as the VDC reads them.
    let img = image(64, 32, |x, y| {
        let cell = (x % 32) / 16 + (y / 16) * 2;
        if x < 32 {
            level(cell + 1)
        } else {
            level(cell + 5)
        }
    });
    let s = import_sprites(&img, 32, 32, 1).unwrap();
    assert_eq!((s.cells, s.frames(), s.patterns.len()), (4, 2, 8));
    for (p, pat) in s.patterns.iter().enumerate() {
        assert_eq!(s.palettes[0][pat[0] as usize], p as u16 + 1, "pattern {p}");
    }
}

#[test]
fn sprite_palettes_split_by_frame() {
    let img = image(32, 16, |x, y| {
        let i = (y * 16 + x % 16) % 15;
        level(if x < 16 { i + 1 } else { i + 200 })
    });
    assert!(import_sprites(&img, 16, 16, 1)
        .unwrap_err()
        .0
        .contains("more than 1 palette"));
    let s = import_sprites(&img, 16, 16, 2).unwrap();
    assert_eq!(s.frame_palettes, vec![0, 1]);
}

#[test]
fn sprite_transparency_and_size_errors() {
    let img = image(16, 16, |x, _| if x < 8 { CLEAR } else { RED });
    let s = import_sprites(&img, 16, 16, 1).unwrap();
    assert_eq!(s.patterns[0][0], 0);
    assert_eq!(s.patterns[0][8], 1);
    let e = import_sprites(&image(24, 16, |_, _| RED), 16, 16, 1).unwrap_err();
    assert!(e.0.contains("multiple of the 16x16 frame size"), "{e}");
    let e = import_sprites(&img, 24, 16, 1).unwrap_err();
    assert!(e.0.contains("16 or 32 wide"), "{e}");
}

// --- Lua -------------------------------------------------------------------

fn px(o: &Output, x: usize, y: usize) -> [u8; 3] {
    let i = (y * o.width + x) * 4;
    [o.rgba[i], o.rgba[i + 1], o.rgba[i + 2]]
}

fn engine_with(images: &[(&str, &Image)], src: &str) -> Result<Engine, LuaError> {
    let mut e = Engine::new();
    for (name, img) in images {
        e.add_image(name, img.width, img.height, img.rgba.clone());
    }
    e.load(&[("main.lua", src)])?;
    Ok(e)
}

/// A 32x16 scene: left half red with a blue 8x8 square, right half
/// transparent, so the backdrop shows through.
fn scene() -> Image {
    image(32, 16, |x, y| {
        if x >= 16 {
            CLEAR
        } else if (4..12).contains(&x) && (4..12).contains(&y) {
            BLUE
        } else {
            RED
        }
    })
}

#[test]
fn lua_image_bg_round_trip() {
    let src = r#"
function init()
  color(0, rgb(0, 255, 0))
  info = image_bg("scene", { x = 2, y = 1 })
end
"#;
    let mut e = engine_with(&[("scene", &scene())], src).unwrap();
    assert!(e.warnings().is_empty(), "{:?}", e.warnings());
    let o = e.render(0.0, 0).unwrap();
    // Map placed at tile (2, 1) = pixel (16, 8).
    assert_eq!(px(&o, 15, 8), [0, 255, 0], "outside the image: backdrop");
    assert_eq!(px(&o, 16, 8), [255, 0, 0]);
    assert_eq!(px(&o, 16 + 4, 8 + 4), [0, 0, 255]);
    assert_eq!(px(&o, 16 + 11, 8 + 11), [0, 0, 255]);
    assert_eq!(px(&o, 16 + 12, 8 + 12), [255, 0, 0]);
    assert_eq!(px(&o, 16 + 20, 8 + 2), [0, 255, 0], "transparent pixels");
    // Tiles went above the 32x32 BAT (1024 words = tile 64).
    let vdc = e.vdc();
    let cell = vdc.bat(BatSize::default(), 2, 1);
    assert!(cell.tile >= 64, "{cell:?}");
}

#[test]
fn lua_image_bg_returns_counts_and_respects_options() {
    let src = r#"
function init()
  bat.w = 64
  local b = image_bg("scene", { tile = 300, pal = 5 })
  assert(b.tiles == 5, "tiles " .. b.tiles) -- 4 blue corners, clear
  assert(b.palettes == 1)
  assert(b.w == 4 and b.h == 2)
  assert(b.tile == 300 and b.pal == 5)
end
"#;
    let e = engine_with(&[("scene", &scene())], src).unwrap_or_else(|e| panic!("{e}"));
    let vdc = e.vdc();
    let size = BatSize::new(64, 32).unwrap();
    let c = vdc.bat(size, 0, 0);
    assert_eq!(c.palette, 5);
    assert!((300..305).contains(&c.tile));
    // Palette colors are sorted by 9-bit value: blue (0o007) before red (0o070).
    assert_eq!(e.vce().bg(5, 1), rgb8(0, 0, 255));
    assert_eq!(e.vce().bg(5, 2), rgb8(255, 0, 0));
}

#[test]
fn lua_default_placement_avoids_earlier_imports() {
    let src = r#"
function init()
  a = image_bg("scene")
  s = image_sprites("guy")
  b = image_bg("scene", { y = 4 })
  assert(a.tile == 64, "first tile " .. a.tile)
  assert(s.pattern * 4 >= a.tile + a.tiles, "pattern " .. s.pattern)
  assert(b.tile * 16 >= (s.pattern + s.patterns) * 64, "second tile " .. b.tile)
  assert(a.pal == 0 and b.pal == 1, "palettes " .. a.pal .. " " .. b.pal)
end
"#;
    let guy = image(16, 16, |_, _| WHITE);
    let e = engine_with(&[("scene", &scene()), ("guy", &guy)], src);
    let e = e.unwrap_or_else(|e| panic!("{e}"));
    assert!(e.warnings().is_empty(), "{:?}", e.warnings());
}

#[test]
fn lua_image_sprites_round_trip() {
    let sheet = image(64, 32, |x, y| {
        if x < 32 {
            if (x % 32) < 16 && y < 16 {
                RED
            } else {
                BLUE
            }
        } else {
            CLEAR
        }
    });
    let src = r#"
function init()
  color(0, rgb(0, 0, 0))
  s = image_sprites("hero", { w = 32, h = 32, pal = 3 })
  assert(s.frames == 2 and s.patterns == 8 and s.step == 4)
  assert(s.pattern % 4 == 0)
  assert(s.pals[1] == 3 and s.pals[2] == 3)
end
function frame()
  spr(0, 40, 50, s.pattern, { w = 32, h = 32, pal = s.pals[1] })
  spr(1, 100, 50, s.pattern + s.step, { w = 32, h = 32, pal = s.pals[2] })
end
"#;
    let mut e = engine_with(&[("hero", &sheet)], src).unwrap_or_else(|e| panic!("{e}"));
    let o = e.render(0.0, 0).unwrap();
    assert_eq!(px(&o, 40, 50), [255, 0, 0]);
    assert_eq!(px(&o, 40 + 16, 50), [0, 0, 255]);
    assert_eq!(px(&o, 40 + 2, 50 + 20), [0, 0, 255]);
    assert_eq!(px(&o, 100, 50), [0, 0, 0], "blank frame shows the backdrop");
}

#[test]
fn lua_unknown_image_lists_known_names() {
    let err = engine_with(
        &[("scene", &scene())],
        "function init() image_bg(\"hills\") end",
    )
    .err()
    .unwrap();
    assert!(err.message.contains("no image named \"hills\""), "{err}");
    assert!(err.message.contains("known images: \"scene\""), "{err}");

    let err = engine_with(&[], "function init() image_sprites(\"x\") end")
        .err()
        .unwrap();
    assert!(err.message.contains("no images are loaded"), "{err}");
}

#[test]
fn lua_errors_name_the_image() {
    let wide = image(33 * 8, 8, |x, _| level(x / 8 % 3 + 1));
    let err = engine_with(&[("wide", &wide)], "function init() image_bg(\"wide\") end")
        .err()
        .unwrap();
    assert!(err.message.contains("image_bg(\"wide\")"), "{err}");
    assert!(err.message.contains("set bat.w"), "{err}");

    let src = "function init() image_sprites(\"wide\", { w = 32, h = 32, pattern = 3 }) end";
    let sheet = image(32, 32, |_, _| RED);
    let err = engine_with(&[("wide", &sheet)], src).err().unwrap();
    assert!(err.message.contains("multiple of 4"), "{err}");

    let bad = Image {
        width: 8,
        height: 8,
        rgba: vec![0; 10],
    };
    let err = engine_with(&[("bad", &bad)], "function init() image_bg(\"bad\") end")
        .err()
        .unwrap();
    assert!(err.message.contains("RGBA bytes"), "{err}");
}

#[test]
fn lua_overlaps_warn() {
    let src = r#"
function init()
  local b = image_bg("scene", { tile = 0 })
end
"#;
    let e = engine_with(&[("scene", &scene())], src).unwrap();
    assert!(
        e.warnings().iter().any(|w| w.contains("overwrites")),
        "{:?}",
        e.warnings()
    );
}

#[test]
fn lua_palette_clash_warns() {
    let src = r#"
function init()
  pal(0, 1, rgb(255, 255, 255))
  local b = image_bg("scene", { pal = 0 })
end
"#;
    let e = engine_with(&[("scene", &scene())], src).unwrap();
    assert!(
        e.warnings()
            .iter()
            .any(|w| w.contains("background palette 0")),
        "{:?}",
        e.warnings()
    );
    // Without pal, the import skips the palette pal() already used.
    let src = "function init() pal(0, 1, 7); b = image_bg(\"scene\"); assert(b.pal == 1) end";
    let e = engine_with(&[("scene", &scene())], src).unwrap_or_else(|e| panic!("{e}"));
    assert!(e.warnings().is_empty(), "{:?}", e.warnings());
}

#[test]
fn images_survive_load_and_reload() {
    let mut e = Engine::new();
    let img = scene();
    e.add_image("scene", img.width, img.height, img.rgba.clone());
    let src = "function init() image_bg(\"scene\") end";
    e.load(&[("main.lua", src)]).unwrap();
    e.load(&[("main.lua", src)]).unwrap();
}

#[test]
fn imports_in_init_are_free_but_charged_in_frame() {
    let src = r#"
function init() image_bg("scene") end
function frame(t, f) if f == 1 then image_bg("scene", { y = 8 }) end end
"#;
    let mut e = engine_with(&[("scene", &scene())], src).unwrap();
    e.render(0.0, 0).unwrap();
    assert_eq!(e.budget().vram_words, 0);
    e.render(0.0, 1).unwrap();
    assert!(e.budget().vram_words > 0);
}
