//! Fernwick, the adventure game whose logic is shared with an SNES version:
//! the TG16 version plays a scripted walk and fight with the joypad, matches
//! its golden frames, stays within the budget, and its attract mode plays
//! itself into the cave.
use tg16_core::*;

include!("fixtures/fernwick_script.rs");

const FILES: [(&str, &str); 4] = [
    ("10_art.lua", include_str!("../toys/fernwick/10_art.lua")),
    (
        "20_world.lua",
        include_str!("../toys/fernwick/20_world.lua"),
    ),
    ("30_game.lua", include_str!("../toys/fernwick/30_game.lua")),
    ("main.lua", include_str!("../toys/fernwick/main.lua")),
];

const GOLDEN_SCROLL: &str = "tests/fixtures/fernwick_tg16_scroll.png";
const GOLDEN_FIGHT: &str = "tests/fixtures/fernwick_tg16_fight.png";

/// TG16 pad bits for the script's buttons: II is the sword, RUN starts.
fn tg16_pad(b: u16) -> u16 {
    let mut m = b & 0xf; // up, down, left, right
    if b & SCRIPT_SWORD != 0 {
        m |= 32;
    }
    if b & SCRIPT_START != 0 {
        m |= 128;
    }
    m
}

struct Run {
    e: Engine,
    f: u32,
}

impl Run {
    fn new() -> Run {
        let mut e = Engine::new();
        e.load(&FILES).unwrap_or_else(|err| panic!("{err}"));
        Run { e, f: 0 }
    }
    fn step(&mut self, pad: u16) -> Output {
        self.e.set_pad(pad);
        let o = self
            .e
            .render(f64::from(self.f) / 60.0, self.f)
            .unwrap_or_else(|err| panic!("frame {}: {err}", self.f));
        let b = self.e.budget();
        assert!(
            b.problems().is_empty(),
            "frame {}: {:?}",
            self.f,
            b.problems()
        );
        assert!(!o.overflow(), "frame {}: sprites dropped", self.f);
        self.f += 1;
        o
    }
    fn state(&mut self) -> Vec<i64> {
        self.e.call_ints("quest_state", &[]).unwrap()
    }
}

/// Play the script; returns the frames at SCRIPT_SCROLL_FRAME and the end.
fn play_script() -> (Run, Output, Output) {
    let mut r = Run::new();
    let mut scroll = None;
    let mut last = None;
    for &(n, b) in SCRIPT {
        for _ in 0..n {
            let at = r.f;
            let o = r.step(tg16_pad(b));
            if at == SCRIPT_SCROLL_FRAME {
                let s = r.state();
                assert_eq!(s[0], 2, "mid-scroll at frame {at}: {s:?}");
                scroll = Some(o);
            } else {
                last = Some(o);
            }
        }
    }
    (r, scroll.unwrap(), last.unwrap())
}

fn decode_png(path: &str) -> Vec<u8> {
    let decoder = png::Decoder::new(std::fs::File::open(path).unwrap());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    buf
}

fn write_png(path: &str, o: &Output) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(file, o.width as u32, o.height as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&o.rgba)
        .unwrap();
}

#[test]
fn scripted_walk_and_fight() {
    let (mut r, scroll, fight) = play_script();
    let s = r.state();
    assert_eq!(s, SCRIPT_END_STATE, "state after the script");
    // the HUD shows two full hearts and an empty one (map row 26, columns 2-4)
    let vram = &r.e.vdc().vram;
    let heart = |c: usize| vram[26 * 64 + c] & 0xfff;
    assert_eq!(heart(2), heart(3));
    assert_ne!(heart(3), heart(4));
    for (path, o) in [(GOLDEN_SCROLL, &scroll), (GOLDEN_FIGHT, &fight)] {
        assert!(
            std::path::Path::new(path).exists(),
            "golden missing: cargo test -p tg16-core --test fernwick regen -- --ignored"
        );
        assert!(o.rgba == decode_png(path), "frame differs from {path}");
    }
}

#[test]
#[ignore = "regenerates the committed golden PNGs"]
fn regen() {
    let (_, scroll, fight) = play_script();
    write_png(GOLDEN_SCROLL, &scroll);
    write_png(GOLDEN_FIGHT, &fight);
}

#[test]
fn attract_mode_plays_into_the_cave() {
    let mut r = Run::new();
    let mut rooms = std::collections::BTreeSet::new();
    let mut soft = 0;
    let mut key = false;
    for _ in 0..1950 {
        r.step(0);
        soft = soft.max(r.e.budget().software);
        let s = r.state();
        rooms.insert(s[1]);
        key |= s[6] == 1;
    }
    assert!(
        rooms.is_superset(&[1, 2, 4, 5, 7].into()),
        "visited {rooms:?}"
    );
    assert!(key, "the autopilot takes the key");
    assert!(soft > 1500, "game logic is charged with cpu(): {soft}");
    assert_eq!(r.state()[0], 0, "back to the title");
}
