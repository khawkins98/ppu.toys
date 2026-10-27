//! Fernwick on both engines: the SNES version (compare/snes/fernwick) plays
//! the same scripted walk and fight as the TG16 version, matches its golden
//! frames without sprite overflow, and, because the game logic is one Lua
//! file shared by both, ends every frame in exactly the same game state as
//! the TG16 version given the same buttons.
use ppu_core::{render_frame_stats, LuaEngine};
use std::path::Path;

include!("../../tg16-core/tests/fixtures/fernwick_script.rs");

const SNES_DIR: &str = "../../compare/snes/fernwick";
const TG16_DIR: &str = "../tg16-core/toys/fernwick";
const SHARED: [&str; 3] = ["10_art.lua", "20_world.lua", "30_game.lua"];
const GOLDEN_SCROLL: &str = "tests/fixtures/fernwick_snes_scroll.png";
const GOLDEN_FIGHT: &str = "tests/fixtures/fernwick_snes_fight.png";

fn read(dir: &str, name: &str) -> String {
    std::fs::read_to_string(Path::new(dir).join(name))
        .unwrap_or_else(|e| panic!("{dir}/{name}: {e}"))
}

/// SNES pad bits: B is the sword (bit 5), Start is bit 10.
fn snes_pad(b: u16) -> u16 {
    let mut m = b & 0xf;
    if b & SCRIPT_SWORD != 0 {
        m |= 1 << 5;
    }
    if b & SCRIPT_START != 0 {
        m |= 1 << 10;
    }
    m
}

fn tg16_pad(b: u16) -> u16 {
    let mut m = b & 0xf;
    if b & SCRIPT_SWORD != 0 {
        m |= 32;
    }
    if b & SCRIPT_START != 0 {
        m |= 128;
    }
    m
}

fn snes_engine() -> LuaEngine {
    let manifest: serde_json::Value = serde_json::from_str(&read(SNES_DIR, "ppu.json")).unwrap();
    let names: Vec<String> = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let sources: Vec<(String, String)> = names
        .iter()
        .map(|n| (n.clone(), read(SNES_DIR, n)))
        .collect();
    let files: Vec<(&str, &str)> = sources
        .iter()
        .map(|(n, s)| (n.as_str(), s.as_str()))
        .collect();
    let mut e = LuaEngine::new();
    e.set_sources(&files)
        .unwrap_or_else(|err| panic!("{err:?}"));
    e
}

fn tg16_engine() -> tg16_core::Engine {
    let names = [SHARED[0], SHARED[1], SHARED[2], "main.lua"];
    let sources: Vec<(String, String)> = names
        .iter()
        .map(|n| (n.to_string(), read(TG16_DIR, n)))
        .collect();
    let files: Vec<(&str, &str)> = sources
        .iter()
        .map(|(n, s)| (n.as_str(), s.as_str()))
        .collect();
    let mut e = tg16_core::Engine::new();
    e.load(&files).unwrap_or_else(|err| panic!("{err}"));
    e
}

fn decode_png(path: &str) -> Vec<u8> {
    let decoder = png::Decoder::new(std::fs::File::open(path).unwrap());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    buf
}

fn write_png(path: &str, rgba: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(file, 256, 224);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}

/// Play the script on the SNES; every frame's quest_state(), and the frames
/// at SCRIPT_SCROLL_FRAME and the end.
fn play_snes() -> (Vec<Vec<i64>>, Vec<u8>, Vec<u8>) {
    let mut e = snes_engine();
    let (mut states, mut scroll, mut last) = (Vec::new(), Vec::new(), Vec::new());
    let mut f = 0u32;
    for &(n, b) in SCRIPT {
        for _ in 0..n {
            e.set_pad(snes_pad(b));
            let lines = e
                .frame(f64::from(f) / 60.0, f)
                .unwrap_or_else(|err| panic!("frame {f}: {err:?}"));
            let (pixels, overflow) = render_frame_stats(&lines, e.memory());
            assert!(
                !overflow.range_over && !overflow.time_over,
                "frame {f}: sprite overflow"
            );
            states.push(e.call_ints("quest_state", &[]).unwrap());
            if f == SCRIPT_SCROLL_FRAME {
                scroll = pixels;
            } else {
                last = pixels;
            }
            f += 1;
        }
    }
    (states, scroll, last)
}

#[test]
fn shared_game_files_are_identical() {
    for name in SHARED {
        assert!(
            read(SNES_DIR, name) == read(TG16_DIR, name),
            "{name} differs between {SNES_DIR} and {TG16_DIR}: run python3 {TG16_DIR}/build_art.py"
        );
    }
}

#[test]
fn snes_scripted_walk_and_fight() {
    let (states, scroll, fight) = play_snes();
    assert_eq!(states[SCRIPT_SCROLL_FRAME as usize][0], 2, "mid-scroll");
    assert_eq!(*states.last().unwrap(), SCRIPT_END_STATE);
    for (path, px) in [(GOLDEN_SCROLL, &scroll), (GOLDEN_FIGHT, &fight)] {
        assert!(
            Path::new(path).exists(),
            "golden missing: cargo test -p ppu-cli --test fernwick regen -- --ignored"
        );
        assert!(*px == decode_png(path), "frame differs from {path}");
    }
}

#[test]
#[ignore = "regenerates the committed golden PNGs"]
fn regen() {
    let (_, scroll, fight) = play_snes();
    write_png(GOLDEN_SCROLL, &scroll);
    write_png(GOLDEN_FIGHT, &fight);
}

#[test]
fn both_machines_play_the_same_game() {
    let (snes, _, _) = play_snes();
    let mut e = tg16_engine();
    let mut f = 0u32;
    for &(n, b) in SCRIPT {
        for _ in 0..n {
            e.set_pad(tg16_pad(b));
            e.frame(f64::from(f) / 60.0, f)
                .unwrap_or_else(|err| panic!("frame {f}: {err}"));
            let tg = e.call_ints("quest_state", &[]).unwrap();
            assert_eq!(tg, snes[f as usize], "frame {f}: TG16 vs SNES state");
            f += 1;
        }
    }
}
