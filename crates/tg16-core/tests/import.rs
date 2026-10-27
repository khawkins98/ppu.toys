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

// --- Color reduction -------------------------------------------------------

/// A colorful 64x32 image: smooth hue and brightness ramps, so most tiles
/// hold far more than 15 colors and the image needs many palettes.
fn ramps() -> Image {
    image(64, 32, |x, y| {
        let r = (x * 4) as u8;
        let g = (y * 8) as u8;
        let b = 255 - ((x + y) * 3) as u8;
        [r, g, b, 255]
    })
}

/// Distinct colors a palette really uses: the colors its cells index.
fn used_colors(b: &BgImport) -> Vec<std::collections::BTreeSet<u16>> {
    let mut used = vec![std::collections::BTreeSet::new(); b.palettes.len()];
    for &(t, p) in &b.map {
        for &i in &b.tiles[t as usize] {
            if i != 0 {
                used[p as usize].insert(b.palettes[p as usize][i as usize]);
            }
        }
    }
    used
}

#[test]
fn reduction_respects_palette_limits() {
    let img = ramps();
    assert!(
        import_bg(&img, 4, 2048).is_err(),
        "too colorful to fit exactly"
    );
    for dither in [Dither::None, Dither::Ordered, Dither::Diffuse] {
        for max in [1, 2, 4] {
            let b = import_bg_with(&img, max, 2048, Some(dither)).unwrap();
            assert!(
                b.palettes.len() <= max,
                "{dither:?} {max}: {}",
                b.palettes.len()
            );
            assert!(b.map.iter().all(|&(_, p)| (p as usize) < b.palettes.len()));
            for (p, used) in used_colors(&b).iter().enumerate() {
                assert!(used.len() <= 15, "palette {p} uses {}", used.len());
            }
            for t in &b.tiles {
                assert!(t.iter().all(|&i| i <= 15));
            }
            let q = &b.quality;
            assert!(q.colors_reduced && q.reduced(), "{q:?}");
            assert_eq!(q.merged_tiles, 0);
            assert!(q.colors > 15 * max, "{q:?}");
            assert!(q.error > 0.0 && q.error < 60.0, "{dither:?} {max}: {q:?}");
        }
    }
}

#[test]
fn more_palettes_mean_less_error() {
    let img = ramps();
    let err = |max| {
        import_bg_with(&img, max, 2048, Some(Dither::None))
            .unwrap()
            .quality
            .error
    };
    let (one, four, sixteen) = (err(1), err(4), err(16));
    assert!(one > four && four > sixteen, "{one} {four} {sixteen}");
}

#[test]
fn reduction_is_deterministic() {
    let img = ramps();
    for dither in [Dither::None, Dither::Ordered, Dither::Diffuse] {
        let a = import_bg_with(&img, 3, 2048, Some(dither)).unwrap();
        let b = import_bg_with(&img, 3, 2048, Some(dither)).unwrap();
        assert_eq!(a, b, "{dither:?}");
    }
    let a = import_sprites_with(&img, 32, 32, 2, Some(Dither::Diffuse)).unwrap();
    assert_eq!(
        a,
        import_sprites_with(&img, 32, 32, 2, Some(Dither::Diffuse)).unwrap()
    );
}

#[test]
fn images_that_fit_are_not_reduced() {
    for img in [
        scene(),
        image(16, 8, |x, y| level((y * 8 + x % 8) % 15 + 1)),
    ] {
        let exact = import_bg(&img, 16, 2048).unwrap();
        for dither in [Dither::None, Dither::Ordered, Dither::Diffuse] {
            let b = import_bg_with(&img, 16, 2048, Some(dither)).unwrap();
            assert_eq!(b, exact);
            assert!(!b.quality.reduced());
            assert_eq!(b.quality.error, 0.0);
        }
    }
}

#[test]
fn dither_modes_differ_and_ordered_keeps_flat_tiles_shared() {
    // Left half: a ramp that needs reducing; right half: one flat color
    // between two 9-bit levels, so dithering patterns it.
    let img = image(64, 16, |x, y| {
        if x < 32 {
            [(x * 8) as u8, (y * 16) as u8, 128, 255]
        } else {
            [90, 150, 200, 255]
        }
    });
    let none = import_bg_with(&img, 1, 2048, Some(Dither::None)).unwrap();
    let ordered = import_bg_with(&img, 1, 2048, Some(Dither::Ordered)).unwrap();
    let fs = import_bg_with(&img, 1, 2048, Some(Dither::Diffuse)).unwrap();
    assert_ne!(none.tiles, ordered.tiles);
    assert_ne!(ordered.tiles, fs.tiles);
    // The flat right half (tiles 4..8 of each row) is one tile under
    // nearest-color and ordered dither: the Bayer pattern is aligned to
    // the image, so every flat 8x8 block dithers the same way.
    for b in [&none, &ordered] {
        let flat: std::collections::BTreeSet<u16> =
            (0..16).filter(|i| i % 8 >= 4).map(|i| b.map[i].0).collect();
        assert_eq!(flat.len(), 1, "{flat:?}");
    }
    let flat_tile = &ordered.tiles[ordered.map[4].0 as usize];
    let distinct: std::collections::BTreeSet<u8> = flat_tile.iter().copied().collect();
    assert!(distinct.len() > 1, "ordered dither patterns the flat color");
}

#[test]
fn diffusion_does_not_cross_sprite_frames() {
    // Frame 0 is a ramp that dithers, frame 1 a single flat color in its
    // palette: no error may leak into frame 1.
    let img = image(32, 16, |x, y| {
        if x < 16 {
            [(x * 16) as u8, (y * 16) as u8, 60, 255]
        } else {
            [255, 255, 255, 255]
        }
    });
    let s = import_sprites_with(&img, 16, 16, 1, Some(Dither::Diffuse)).unwrap();
    assert!(s.quality.colors_reduced);
    let p = &s.patterns[1];
    assert!(p.iter().all(|&i| i == p[0]), "frame 1 stays flat");
    assert_eq!(s.palettes[0][p[0] as usize], rgb8(255, 255, 255));
}

#[test]
fn sprite_reduction_fits_one_palette_per_frame() {
    let img = ramps();
    assert!(import_sprites(&img, 32, 32, 1).is_err());
    let s = import_sprites_with(&img, 32, 32, 1, Some(Dither::Ordered)).unwrap();
    assert_eq!(s.palettes.len(), 1);
    assert_eq!(s.frames(), 2);
    assert_eq!(s.patterns.len(), 8);
    let used: std::collections::BTreeSet<u8> = s.patterns.iter().flatten().copied().collect();
    assert!(used.len() <= 15 && !used.contains(&0), "{used:?}");
    assert!(s.quality.reduced() && s.quality.error > 0.0);
    // Two palettes: each frame may get its own.
    let s2 = import_sprites_with(&img, 32, 32, 2, Some(Dither::Ordered)).unwrap();
    assert!(s2.quality.error < s.quality.error);
}

#[test]
fn merging_tiles_fits_the_tile_budget() {
    // Eight tiles: four solid colors, each also with a variant that has one
    // pixel slightly off. Merging the variants costs least.
    let base = [RED, GREEN, BLUE, WHITE];
    let img = image(64, 8, |x, y| {
        let t = (x / 8) as usize;
        let c = base[t % 4];
        if t >= 4 && x % 8 == 3 && y == 3 {
            [c[0] / 2 + 60, c[1] / 2 + 60, c[2] / 2 + 60, 255]
        } else {
            c
        }
    });
    let exact = import_bg(&img, 16, 8).unwrap();
    assert_eq!(exact.tiles.len(), 8);
    let e = import_bg(&img, 16, 4).unwrap_err();
    assert!(e.0.contains("8 unique tiles but only 4 fit"), "{e}");
    let b = import_bg_with(&img, 16, 4, Some(Dither::Ordered)).unwrap();
    assert_eq!(b.tiles.len(), 4);
    assert_eq!(b.quality.merged_tiles, 4);
    assert!(!b.quality.colors_reduced);
    assert!(b.quality.reduced());
    assert!(
        b.quality.error > 0.0 && b.quality.error < 5.0,
        "{:?}",
        b.quality
    );
    for t in 0..4 {
        assert_eq!(
            b.map[t].0,
            b.map[t + 4].0,
            "variant {t} merged into its base"
        );
    }
    assert!(
        b.tiles.iter().all(|t| t.iter().all(|&i| i == t[0])),
        "solid tiles kept"
    );

    // Tiles whose transparent pixels differ never merge.
    let holes = image(32, 8, |x, y| {
        if y as usize == (x / 8) as usize {
            CLEAR
        } else {
            RED
        }
    });
    let e = import_bg_with(&holes, 16, 2, Some(Dither::Ordered)).unwrap_err();
    assert!(e.0.contains("even after merging"), "{e}");
}

#[test]
fn dither_names_parse() {
    assert_eq!(Dither::parse("none"), Some(Dither::None));
    assert_eq!(Dither::parse("ordered"), Some(Dither::Ordered));
    assert_eq!(Dither::parse("fs"), Some(Dither::Diffuse));
    assert_eq!(Dither::parse("wobble"), None);
    assert_eq!(Dither::default(), Dither::Ordered);
}

#[test]
fn lua_reduces_by_default_and_reports_quality() {
    let src = r#"
function init()
  bat.w = 64
  b = image_bg("ramps", { pals = 2 })
  assert(b.reduced == true, "reduced")
  assert(b.palettes <= 2, "palettes " .. b.palettes)
  assert(b.colors > 30, "colors " .. b.colors)
  assert(b.error > 0 and b.error < 60, "error " .. b.error)
  assert(b.merged_tiles == 0, "merged " .. b.merged_tiles)
  s = image_sprites("ramps", { w = 32, h = 32, pals = 1, dither = "fs" })
  assert(s.reduced == true and s.palettes == 1 and s.merged_tiles == nil)
  e = image_bg("scene", { y = 8 })
  assert(e.reduced == false and e.error == 0 and e.merged_tiles == 0)
end
"#;
    let e = engine_with(&[("ramps", &ramps()), ("scene", &scene())], src)
        .unwrap_or_else(|e| panic!("{e}"));
    let w = e.warnings();
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(
        w[0].starts_with("image_bg(\"ramps\"): its ")
            && w[0].contains("did not fit 2 palettes of 15")
            && w[0].contains("(ordered dither)")
            && w[0].contains("mean color error"),
        "{w:?}"
    );
    assert!(w[1].contains("image_sprites(\"ramps\")") && w[1].contains("(fs dither)"));
}

#[test]
fn lua_reduction_is_deterministic_on_screen() {
    let src = r#"
function init() image_bg("ramps", { pals = 3, dither = "ordered" }) end
"#;
    let render = || {
        let mut e = engine_with(&[("ramps", &ramps())], src).unwrap();
        e.render(0.0, 0).unwrap().rgba
    };
    assert_eq!(render(), render());
}

#[test]
fn lua_reduce_false_keeps_the_strict_error() {
    let src = "function init() image_bg(\"ramps\", { pals = 2, reduce = false }) end";
    let err = engine_with(&[("ramps", &ramps())], src).err().unwrap();
    assert!(err.message.contains("image_bg(\"ramps\")"), "{err}");
    assert!(err.message.contains("reduce = false is set"), "{err}");

    let src = "function init() image_sprites(\"ramps\", { w = 32, h = 32, pals = 1, reduce = false }) end";
    let err = engine_with(&[("ramps", &ramps())], src).err().unwrap();
    assert!(err.message.contains("colors"), "{err}");

    let bad = [
        ("{ dither = \"wobble\" }", "option dither must be"),
        ("{ dither = 3 }", "option dither must be"),
        ("{ reduce = 1 }", "option reduce must be true or false"),
    ];
    for (opts, want) in bad {
        let src = format!("function init() image_bg(\"scene\", {opts}) end");
        let err = engine_with(&[("scene", &scene())], &src).err().unwrap();
        assert!(err.message.contains(want), "{opts}: {err}");
    }
}

#[test]
fn lua_merged_tiles_are_reported() {
    // Room for only 4 tiles at the top of VRAM: the 8-tile image merges.
    let base = [RED, GREEN, BLUE, WHITE];
    let img = image(64, 8, |x, y| {
        let t = (x / 8) as usize;
        let c = base[t % 4];
        if t >= 4 && x % 8 == 3 && y == 3 {
            [c[0] / 2 + 60, c[1] / 2 + 60, c[2] / 2 + 60, 255]
        } else {
            c
        }
    });
    let src = r#"
function init()
  b = image_bg("tiles", { tile = 2044 })
  assert(b.tiles == 4 and b.merged_tiles == 4 and b.reduced, "tiles " .. b.tiles)
end
"#;
    let e = engine_with(&[("tiles", &img)], src).unwrap_or_else(|e| panic!("{e}"));
    let w = e.warnings();
    assert!(
        w.iter()
            .any(|w| w.contains("4 similar tiles were merged to fit VRAM")
                && !w.contains("did not fit")),
        "{w:?}"
    );
}
