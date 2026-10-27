//! Comet Lancer: the 3D flying toy plays, takes the joypad and stays on budget.
use tg16_core::*;

const FILES: [(&str, &str); 4] = [
    (
        "10_raster.lua",
        include_str!("../toys/comet_lancer/10_raster.lua"),
    ),
    (
        "20_models.lua",
        include_str!("../toys/comet_lancer/20_models.lua"),
    ),
    (
        "30_world.lua",
        include_str!("../toys/comet_lancer/30_world.lua"),
    ),
    ("main.lua", include_str!("../toys/comet_lancer/main.lua")),
];

const UP: u16 = 1;
const DOWN: u16 = 2;
const LEFT: u16 = 4;
const RIGHT: u16 = 8;
const I: u16 = 16;
const II: u16 = 32;
const RUN: u16 = 128;

const SELECT: u16 = 64;

/// BAT word of the first letter of the info bar's message line (map
/// column 6, row 3 of a 128-wide map), and of a cell that is always blank.
const MESSAGE_CELL: usize = 3 * 128 + 6;
const BLANK_CELL: usize = 3 * 128 + 100;

#[test]
fn attract_mode_flies_itself_and_run_starts_a_game() {
    let mut e = Engine::new();
    e.load(&FILES).unwrap_or_else(|err| panic!("{err}"));
    let mut f = 0u32;
    let mut step = |e: &mut Engine, pad: u16| {
        e.set_pad(pad);
        let o = e
            .render(f64::from(f) / 60.0, f)
            .unwrap_or_else(|err| panic!("frame {f}: {err}"));
        let b = e.budget();
        assert!(b.problems().is_empty(), "frame {f}: {:?}", b.problems());
        assert!(!o.overflow(), "frame {f}: the toy drops far sprites itself");
        f += 1;
        b
    };
    let mut soft = 0;
    for _ in 0..300 {
        soft = soft.max(step(&mut e, 0).software);
    }
    assert!(
        soft > 10_000,
        "the polygon ship charges its fill time: {soft}"
    );
    let blank = e.vdc().vram[BLANK_CELL] & 0xfff;
    assert_ne!(
        e.vdc().vram[MESSAGE_CELL] & 0xfff,
        blank,
        "the title is typed out in attract mode"
    );

    step(&mut e, RUN);
    assert_eq!(
        e.vdc().vram[MESSAGE_CELL] & 0xfff,
        blank,
        "RUN starts a game and a new message"
    );

    // Weave, climb, dive, fire, roll and drop a bomb for 15 seconds.
    for k in 0..900u32 {
        let mut pad = I;
        pad |= if k / 90 % 2 == 0 { LEFT } else { RIGHT };
        pad |= if k / 150 % 2 == 0 { UP } else { DOWN };
        if k % 200 == 100 {
            pad |= II;
        }
        if k == 400 {
            pad |= SELECT;
        }
        step(&mut e, pad);
    }
}
