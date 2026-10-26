//! Golden framebuffer compare for a small TG16 scene: a sky gradient made of
//! per-line backdrop writes, a hill layer with per-line scroll (one-layer
//! parallax, the way TG16 games did it), and a few sprites in front of and
//! behind the hills.
use tg16_core::*;

const GOLDEN: &str = "tests/fixtures/golden_scene.png";

fn scene() -> (Vdc, Vce, Frame) {
    let mut vdc = Vdc::default();
    let mut vce = Vce::default();
    let bat = BatSize::new(64, 32).unwrap();
    let t0 = bat.first_free_tile();

    // Tiles: 0 empty, 1 hill top edge, 2 solid hill, 3 stripes for the ground.
    vdc.write_tile(t0, &[0; 64]);
    vdc.write_tile(
        t0 + 1,
        &std::array::from_fn(|i| {
            let (x, y) = (i % 8, i / 8);
            if y >= (x as i32 - 3).unsigned_abs() as usize {
                1 + (y % 2) as u8
            } else {
                0
            }
        }),
    );
    vdc.write_tile(
        t0 + 2,
        &std::array::from_fn(|i| 1 + ((i / 8 + i % 8) % 3 == 0) as u8),
    );
    vdc.write_tile(
        t0 + 3,
        &std::array::from_fn(|i| if (i / 8) < 4 { 3 } else { 4 }),
    );

    for ty in 0..32u16 {
        for tx in 0..64u16 {
            // A wavy skyline between rows 14 and 19, ground from row 22.
            let top = 16 + ((tx as f32 * 0.45).sin() * 2.5) as i32;
            let tile = if ty >= 22 {
                t0 + 3
            } else if (ty as i32) > top {
                t0 + 2
            } else if ty as i32 == top {
                t0 + 1
            } else {
                t0
            };
            let palette = if ty >= 22 { 1 } else { 0 };
            vdc.set_bat(bat, tx, ty, BatEntry { tile, palette });
        }
    }
    vce.set(0x01, grb(1, 4, 1));
    vce.set(0x02, grb(1, 5, 2));
    vce.set(0x13, grb(4, 3, 1));
    vce.set(0x14, grb(3, 2, 1));

    // A 32x32 ball sprite made of four patterns.
    let p0 = 0x100;
    for cell in 0..4u16 {
        let (cx, cy) = ((cell % 2) as i32 * 16, (cell / 2) as i32 * 16);
        vdc.write_pattern(
            p0 + cell,
            &std::array::from_fn(|i| {
                let (x, y) = (cx + (i % 16) as i32 - 16, cy + (i / 16) as i32 - 16);
                let d = x * x + y * y;
                if d < 120 {
                    2
                } else if d < 230 {
                    1
                } else {
                    0
                }
            }),
        );
    }
    for pal in 0..3u8 {
        let base = SPRITE_PALETTE_BASE + pal as usize * 16;
        vce.set(base + 1, grb(7, 2 + pal * 2, 1));
        vce.set(base + 2, grb(7, 6, 3 + pal * 2));
    }
    let ball = |x, y, palette, front| Sprite {
        x,
        y,
        pattern: p0,
        palette,
        front,
        width: 32,
        height: 32,
        ..Sprite::default()
    };
    vdc.set_sprite(0, ball(40, 60, 0, true));
    vdc.set_sprite(1, ball(120, 118, 1, false)); // partly behind the hills
    let mut flipped = ball(200, 150, 2, true);
    flipped.flip_y = true;
    vdc.set_sprite(2, flipped);

    let mut f = Frame::new(WIDTH, HEIGHT, LineRegs::default());
    f.bat = bat;
    // Sky gradient: rewrite the backdrop color every 16 lines.
    for band in 0..8u8 {
        f.lines[band as usize * 16]
            .palette_writes
            .push((0, grb(band / 2, band / 2 + 1, 7 - band / 3)));
    }
    // Hills scroll slowly, ground scrolls faster: two speeds from one layer.
    f.hdma(0, 175, |_, l| l.scroll_x = 37);
    f.hdma(176, HEIGHT - 1, |y, l| {
        l.scroll_x = 90 + (y as u16 - 176) * 2
    });
    (vdc, vce, f)
}

fn decode_png(path: &str) -> Vec<u8> {
    let decoder = png::Decoder::new(std::fs::File::open(path).unwrap());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    buf
}

#[test]
fn scene_matches_golden_png() {
    assert!(
        std::path::Path::new(GOLDEN).exists(),
        "golden missing — run: cargo test -p tg16-core regen_golden -- --ignored"
    );
    let (vdc, vce, f) = scene();
    let out = render(&vdc, &vce, &f);
    assert!(!out.overflow());
    assert_eq!(
        out.rgba,
        decode_png(GOLDEN),
        "framebuffer differs from golden PNG"
    );
}

#[test]
#[ignore = "regenerates the committed golden PNG"]
fn regen_golden() {
    let (vdc, vce, f) = scene();
    let out = render(&vdc, &vce, &f);
    std::fs::create_dir_all("tests/fixtures").unwrap();
    let file = std::fs::File::create(GOLDEN).unwrap();
    let mut enc = png::Encoder::new(file, out.width as u32, out.height as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&out.rgba)
        .unwrap();
}
