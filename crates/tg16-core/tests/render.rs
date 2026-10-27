//! Renderer behavior: priority, transparency, the sprite line limit, flips,
//! scroll wrap, the BYR reload rule, and per-line palette writes.
use tg16_core::*;

const RED: u16 = grb(7, 0, 0);
const GREEN: u16 = grb(0, 7, 0);
const BLUE: u16 = grb(0, 0, 7);
const WHITE: u16 = grb(7, 7, 7);

fn px(o: &Output, x: usize, y: usize) -> [u8; 3] {
    let i = (y * o.width + x) * 4;
    [o.rgba[i], o.rgba[i + 1], o.rgba[i + 2]]
}

fn c(v: u16) -> [u8; 3] {
    to_rgb8(v)
}

/// Tile data lives above the BAT (which fills VRAM from word 0), as on hardware.
const T1: u16 = 0x100;
const T2: u16 = 0x101;
const P1: u16 = 0x100;
const P2: u16 = 0x101;

/// T1 solid color 1; T2 left half color 2, right half transparent.
/// P1 solid sprite color 3; P2 left half sprite color 4.
fn setup() -> (Vdc, Vce) {
    let mut vdc = Vdc::default();
    vdc.write_tile(T1, &[1; 64]);
    vdc.write_tile(T2, &std::array::from_fn(|i| if i % 8 < 4 { 2 } else { 0 }));
    vdc.write_pattern(P1, &[3; 256]);
    vdc.write_pattern(P2, &std::array::from_fn(|i| if i % 16 < 8 { 4 } else { 0 }));
    let mut vce = Vce::default();
    vce.set(0, BLUE); // backdrop
    vce.set(1, RED);
    vce.set(2, GREEN);
    vce.set(SPRITE_PALETTE_BASE + 3, WHITE);
    vce.set(SPRITE_PALETTE_BASE + 4, GREEN);
    (vdc, vce)
}

fn frame() -> Frame {
    Frame::new(WIDTH, HEIGHT, LineRegs::default())
}

#[test]
fn empty_screen_shows_backdrop() {
    let (vdc, vce) = setup();
    let o = render(&vdc, &vce, &frame());
    assert_eq!(o.rgba.len(), WIDTH * HEIGHT * 4);
    assert_eq!(px(&o, 0, 0), c(BLUE));
    assert_eq!(px(&o, 255, 223), c(BLUE));
}

#[test]
fn bg_color_zero_is_transparent_in_every_palette() {
    let (mut vdc, mut vce) = setup();
    vce.set(0x30 | 2, GREEN); // palette 3, color 2
    vce.set(0x30, RED); // palette 3 color 0 must never show
    vdc.set_bat(
        BatSize::default(),
        0,
        0,
        BatEntry {
            tile: T2,
            palette: 3,
        },
    );
    let o = render(&vdc, &vce, &frame());
    assert_eq!(px(&o, 0, 0), c(GREEN));
    assert_eq!(px(&o, 5, 0), c(BLUE));
}

#[test]
fn bg_scroll_wraps_around_the_virtual_screen() {
    let (mut vdc, vce) = setup();
    let bat = BatSize::new(32, 32).unwrap();
    vdc.set_bat(
        bat,
        31,
        31,
        BatEntry {
            tile: T1,
            palette: 0,
        },
    );
    let mut f = frame();
    f.lines.iter_mut().for_each(|l| l.scroll_x = 248);
    f.scroll_y = 248;
    let o = render(&vdc, &vce, &f);
    assert_eq!(px(&o, 0, 0), c(RED));
    assert_eq!(px(&o, 8, 0), c(BLUE));
    assert_eq!(px(&o, 0, 8), c(BLUE));
}

#[test]
fn wide_bat_uses_mwr_width() {
    let (mut vdc, vce) = setup();
    let bat = BatSize::new(64, 32).unwrap();
    vdc.set_bat(
        bat,
        40,
        0,
        BatEntry {
            tile: T1,
            palette: 0,
        },
    );
    let mut f = frame();
    f.bat = bat;
    f.lines.iter_mut().for_each(|l| l.scroll_x = 320);
    let o = render(&vdc, &vce, &f);
    assert_eq!(px(&o, 0, 0), c(RED));
}

#[test]
fn byr_write_reloads_the_row_counter() {
    let (mut vdc, vce) = setup();
    let bat = BatSize::default();
    // Map row 0 is solid; everything else is empty.
    for tx in 0..32 {
        vdc.set_bat(
            bat,
            tx,
            0,
            BatEntry {
                tile: T1,
                palette: 0,
            },
        );
    }
    let mut f = frame();
    f.lines[100].scroll_y_write = Some(0);
    let o = render(&vdc, &vce, &f);
    assert_eq!(px(&o, 0, 7), c(RED));
    assert_eq!(px(&o, 0, 8), c(BLUE));
    assert_eq!(px(&o, 0, 99), c(BLUE));
    // Line 100 restarts at row 0 and the next lines continue from there.
    assert_eq!(px(&o, 0, 100), c(RED));
    assert_eq!(px(&o, 0, 107), c(RED));
    assert_eq!(px(&o, 0, 108), c(BLUE));
}

#[test]
fn bg_disable_bit_hides_the_layer() {
    let (mut vdc, vce) = setup();
    vdc.set_bat(
        BatSize::default(),
        0,
        0,
        BatEntry {
            tile: T1,
            palette: 0,
        },
    );
    let mut vce = vce;
    vce.set(SPRITE_PALETTE_BASE, GREEN);
    let mut f = frame();
    f.lines[0].bg_on = false;
    let o = render(&vdc, &vce, &f);
    // With the layer off (sprites on) the line shows entry 256, the overscan
    // color, not the backdrop.
    assert_eq!(px(&o, 0, 0), c(GREEN));
    assert_eq!(px(&o, 0, 1), c(RED));
    // With both off (not on line 0) it shows the backdrop.
    f.lines[1].bg_on = false;
    f.lines[1].sprites_on = false;
    let o = render(&vdc, &vce, &f);
    assert_eq!(px(&o, 0, 1), c(BLUE));
}

#[test]
fn both_layers_off_on_line_0_blanks_the_frame() {
    let (mut vdc, mut vce) = setup();
    vdc.set_bat(
        BatSize::default(),
        0,
        1,
        BatEntry {
            tile: T1,
            palette: 0,
        },
    );
    vce.set(SPRITE_PALETTE_BASE, GREEN);
    let mut f = frame();
    f.lines[0].bg_on = false;
    f.lines[0].sprites_on = false;
    let o = render(&vdc, &vce, &f);
    // Burst mode: line 8 (map row 1) enables the layer, but the frame stays
    // blank in the overscan color.
    assert_eq!(px(&o, 0, 0), c(GREEN));
    assert_eq!(px(&o, 0, 8), c(GREEN));
    f.lines[0].sprites_on = true;
    let o = render(&vdc, &vce, &f);
    assert_eq!(px(&o, 0, 8), c(RED));
}

fn spr(x: i32, y: i32, pattern: u16, front: bool) -> Sprite {
    Sprite {
        x,
        y,
        pattern,
        front,
        width: 16,
        height: 16,
        ..Sprite::default()
    }
}

#[test]
fn sprite_priority_bit_against_background() {
    let (mut vdc, vce) = setup();
    let bat = BatSize::default();
    for tx in 0..32 {
        vdc.set_bat(
            bat,
            tx,
            0,
            BatEntry {
                tile: T2,
                palette: 0,
            },
        );
    }
    vdc.set_sprite(0, spr(0, 0, P1, false)); // behind
    vdc.set_sprite(1, spr(32, 0, P1, true)); // in front
    let o = render(&vdc, &vce, &frame());
    assert_eq!(px(&o, 0, 0), c(GREEN), "opaque BG covers a back sprite");
    assert_eq!(
        px(&o, 5, 0),
        c(WHITE),
        "back sprite shows through BG color 0"
    );
    assert_eq!(px(&o, 32, 0), c(WHITE), "front sprite covers BG");
}

#[test]
fn lower_sat_index_wins_and_transparent_pixels_fall_through() {
    let (mut vdc, vce) = setup();
    vdc.set_sprite(0, spr(0, 0, P2, true)); // left half green, right half clear
    vdc.set_sprite(1, spr(0, 0, P1, true)); // solid white
    let o = render(&vdc, &vce, &frame());
    assert_eq!(px(&o, 0, 0), c(GREEN));
    assert_eq!(px(&o, 12, 0), c(WHITE));
}

#[test]
fn sprite_flip_x() {
    let (mut vdc, vce) = setup();
    let mut s = spr(0, 0, P2, true);
    s.flip_x = true;
    vdc.set_sprite(0, s);
    let o = render(&vdc, &vce, &frame());
    assert_eq!(px(&o, 0, 0), c(BLUE));
    assert_eq!(px(&o, 15, 0), c(GREEN));
}

#[test]
fn wide_sprite_flip_swaps_cells() {
    let (mut vdc, vce) = setup();
    // 32x16 sprite at P1: cell 0 = P1 (white), cell 1 = P2.
    let mut s = spr(0, 0, P1, true);
    s.width = 32;
    vdc.set_sprite(0, s);
    let o = render(&vdc, &vce, &frame());
    assert_eq!(px(&o, 0, 0), c(WHITE));
    assert_eq!(px(&o, 16, 0), c(GREEN));
    assert_eq!(px(&o, 24, 0), c(BLUE));
    s.flip_x = true;
    vdc.set_sprite(0, s);
    let o = render(&vdc, &vce, &frame());
    assert_eq!(px(&o, 0, 0), c(BLUE));
    assert_eq!(px(&o, 8, 0), c(GREEN));
    assert_eq!(px(&o, 16, 0), c(WHITE));
}

#[test]
fn sixteen_units_per_line_then_drop() {
    let (mut vdc, vce) = setup();
    for i in 0..17 {
        vdc.set_sprite(i, spr(i as i32 * 15, 10, P1, true));
    }
    let o = render(&vdc, &vce, &frame());
    assert_eq!(o.lines[10].units, 17);
    assert_eq!(o.lines[10].dropped, 1);
    assert!(o.overflow());
    assert_eq!(px(&o, 15 * 15, 10), c(WHITE));
    assert_eq!(px(&o, 16 * 15 + 2, 10), c(BLUE), "17th sprite is dropped");
    assert_eq!(o.lines[9].units, 0);
}

#[test]
fn wide_sprites_cost_two_units() {
    let (mut vdc, vce) = setup();
    for i in 0..9 {
        let mut s = spr(i as i32 * 20, 0, P1, true);
        s.width = 32;
        vdc.set_sprite(i, s);
    }
    let o = render(&vdc, &vce, &frame());
    assert_eq!(o.lines[0].units, 18);
    assert_eq!(o.lines[0].dropped, 1);
}

#[test]
fn per_line_palette_writes_persist() {
    let (vdc, vce) = setup();
    let mut f = frame();
    f.lines[50].palette_writes.push((0, RED));
    let o = render(&vdc, &vce, &f);
    assert_eq!(px(&o, 0, 49), c(BLUE));
    assert_eq!(px(&o, 0, 50), c(RED));
    assert_eq!(px(&o, 0, 200), c(RED));
}

#[test]
fn per_line_scroll_x_with_hdma_helper() {
    let (mut vdc, vce) = setup();
    vdc.set_bat(
        BatSize::default(),
        1,
        0,
        BatEntry {
            tile: T1,
            palette: 0,
        },
    );
    let mut f = frame();
    f.hdma(4, 7, |_, l| l.scroll_x = 8);
    let o = render(&vdc, &vce, &f);
    assert_eq!(px(&o, 0, 3), c(BLUE));
    assert_eq!(px(&o, 0, 4), c(RED));
}

#[test]
fn width_follows_the_frame() {
    let (vdc, vce) = setup();
    let f = Frame::new(336, 240, LineRegs::default());
    let o = render(&vdc, &vce, &f);
    assert_eq!((o.width, o.height), (336, 240));
    assert_eq!(o.rgba.len(), 336 * 240 * 4);
}
