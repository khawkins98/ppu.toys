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

/// BAT word of the title text ("COMET LANCER" at map column 10, row 4).
const TITLE_CELL: usize = 4 * 64 + 10;

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
    let title = e.vdc().vram[TITLE_CELL];
    assert_ne!(
        title & 0xfff,
        e.vdc().vram[0] & 0xfff,
        "title shown in attract mode"
    );

    step(&mut e, RUN);
    step(&mut e, 0);
    assert_ne!(e.vdc().vram[TITLE_CELL], title, "RUN clears the title");

    // Weave, climb, dive, fire and roll for 15 seconds.
    for k in 0..900u32 {
        let mut pad = I;
        pad |= if k / 90 % 2 == 0 { LEFT } else { RIGHT };
        pad |= if k / 150 % 2 == 0 { UP } else { DOWN };
        if k % 200 == 100 {
            pad |= II;
        }
        step(&mut e, pad);
    }
}
