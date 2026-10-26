//! The Lua front end driving the TG16 core.
use tg16_core::*;

fn px(o: &Output, x: usize, y: usize) -> [u8; 3] {
    let i = (y * o.width + x) * 4;
    [o.rgba[i], o.rgba[i + 1], o.rgba[i + 2]]
}

fn load(src: &str) -> Engine {
    let mut e = Engine::new();
    e.load(&[("main.lua", src)])
        .unwrap_or_else(|e| panic!("{e}"));
    e
}

const SETUP: &str = r#"
function init()
  tile(64, string.rep("1", 64))
  pattern(32, string.rep("2", 256)) -- tile 64 and pattern 16 would share VRAM
  color(0, rgb3(0, 0, 7))
  pal(0, 1, rgb3(7, 0, 0))
  spal(0, 2, rgb3(7, 7, 7))
  for tx = 0, 31 do map(tx, 0, 64, 0) end
end
"#;

const BLUE: [u8; 3] = [0, 0, 255];
const RED: [u8; 3] = [255, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];

#[test]
fn init_writes_vram_and_palette() {
    let mut e = load(SETUP);
    let o = e.render(0.0, 0).unwrap();
    assert_eq!(px(&o, 0, 0), RED);
    assert_eq!(px(&o, 0, 8), BLUE);
}

#[test]
fn sprites_are_placed_each_frame() {
    let src =
        format!("{SETUP}\nfunction frame(t, f)\n  if f == 0 then spr(0, 10, 20, 32) end\nend");
    let mut e = load(&src);
    let o = e.render(0.0, 0).unwrap();
    assert_eq!(px(&o, 10, 20), WHITE);
    let o = e.render(0.0, 1).unwrap();
    assert_eq!(px(&o, 10, 20), BLUE, "sprite table clears between frames");
}

#[test]
fn scroll_y_in_hooks_is_an_offset() {
    // Rows 0..7 of the map are red. A hook moving lines 100..107 up by 100
    // shows map rows 0..7 there; the lines below go back to normal.
    let src = format!(
        "{SETUP}\nfunction frame()\n  hdma(100, 107, function(y) bg.scroll.y = -100 end)\nend"
    );
    let mut e = load(&src);
    let o = e.render(0.0, 0).unwrap();
    assert_eq!(px(&o, 0, 99), BLUE);
    assert_eq!(px(&o, 0, 100), RED);
    assert_eq!(px(&o, 0, 107), RED);
    assert_eq!(px(&o, 0, 108), BLUE);
    let f = e.frame(0.0, 0).unwrap();
    assert_eq!(f.lines[100].scroll_y_write, Some(0));
    assert_eq!(f.lines[108].scroll_y_write, Some(108));
    assert_eq!(f.lines[109].scroll_y_write, None);
}

#[test]
fn raw_byr_uses_hardware_reload() {
    // Writing BYR = 0 on line 100 restarts the row counter there; with no
    // further writes, the lines below keep counting from row 0.
    let src = format!("{SETUP}\nfunction frame()\n  hdma(100, 100, function(y) BYR = 0 end)\nend");
    let mut e = load(&src);
    let o = e.render(0.0, 0).unwrap();
    assert_eq!(px(&o, 0, 100), RED);
    assert_eq!(px(&o, 0, 107), RED);
    assert_eq!(px(&o, 0, 108), BLUE);
}

#[test]
fn frame_wide_scroll_and_show_bits() {
    let src = format!(
        "{SETUP}\nfunction frame()\n  bg.scroll.y = 8\n  hdma(0, 3, function() show.bg = false end)\nend"
    );
    let mut e = load(&src);
    let f = e.frame(0.0, 0).unwrap();
    assert_eq!(f.scroll_y, 8);
    assert!(!f.lines[3].bg_on);
    assert!(f.lines[4].bg_on);
}

#[test]
fn hook_palette_writes_last_until_frame_end() {
    let src = format!(
        "{SETUP}\nfunction frame()\n  hdma(50, 50, function() color(0, rgb3(7, 0, 0)) end)\nend"
    );
    let mut e = load(&src);
    let o = e.render(0.0, 0).unwrap();
    assert_eq!(px(&o, 0, 49), BLUE);
    assert_eq!(px(&o, 0, 50), RED);
    assert_eq!(px(&o, 0, 200), RED);
    // The base palette is untouched, so the next frame starts blue again.
    let o = e.render(0.0, 1).unwrap();
    assert_eq!(px(&o, 0, 49), BLUE);
}

#[test]
fn display_size_comes_from_lua() {
    let mut e = load("function init() display.width = 336; display.height = 240 end");
    let o = e.render(0.0, 0).unwrap();
    assert_eq!((o.width, o.height), (336, 240));
}

#[test]
fn errors_name_the_file_and_line() {
    let mut e = Engine::new();
    let err = e.load(&[("main.lua", "x = = 1")]).unwrap_err();
    assert_eq!(err.file.as_deref(), Some("main.lua"));
    assert_eq!(err.line, Some(1));

    let mut e = load("function frame() spr(99, 0, 0, 0) end");
    let err = e.frame(0.0, 0).unwrap_err();
    assert!(err.message.contains("0..63"), "{}", err.message);
    assert_eq!(err.file.as_deref(), Some("main.lua"));
}

#[test]
fn bad_pixel_strings_are_errors() {
    let mut e = Engine::new();
    let err = e.load(&[("main.lua", "tile(64, '123')")]).unwrap_err();
    assert!(err.message.contains("64 pixels"), "{}", err.message);
}

// The sunset toy, rendered at t = 1 s, matches its golden image.
const SUNSET: &str = include_str!("../toys/sunset.lua");
const GOLDEN: &str = "tests/fixtures/golden_sunset.png";

fn sunset() -> Output {
    let mut e = Engine::new();
    e.load(&[("sunset.lua", SUNSET)])
        .unwrap_or_else(|e| panic!("{e}"));
    e.render(1.0, 60).unwrap()
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
fn sunset_toy_matches_golden_png() {
    let o = sunset();
    assert!(!o.overflow());
    assert_eq!(o.rgba, decode_png(GOLDEN), "sunset differs from golden PNG");
}

#[test]
#[ignore = "regenerates the committed golden PNG"]
fn regen_sunset_golden() {
    let o = sunset();
    let file = std::fs::File::create(GOLDEN).unwrap();
    let mut enc = png::Encoder::new(file, o.width as u32, o.height as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&o.rgba)
        .unwrap();
}

#[test]
fn overlapping_vram_writes_warn() {
    // A 64x32 BAT fills words 0..2048, so tile 64 (word 1024) sits inside it.
    let e = load("bat.w = 64\nmap(0, 16, 1, 0)\ntile(64, string.rep('1', 64))");
    let w = e.warnings();
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("tile 64 overwrites the BAT"), "{}", w[0]);

    let e = load(SETUP);
    assert!(e.warnings().is_empty(), "{:?}", e.warnings());
    let e = load(include_str!("../toys/sunset.lua"));
    assert!(e.warnings().is_empty(), "{:?}", e.warnings());
}

#[test]
fn psg_plays_a_note() {
    let mut e = load(
        "function init() wave(1, 'square') end\n\
         function frame(t, f) psg(1, {on = f < 2, note = 'A4', vol = 31}) end",
    );
    e.frame(0.0, 0).unwrap();
    assert_eq!(e.psg().channels[0].period, 254);
    let a = e.audio().to_vec();
    assert!(a.len() == 734 * 2 || a.len() == 735 * 2, "{}", a.len());
    assert!(a.iter().any(|&s| s > 1000) && a.iter().any(|&s| s < -1000));
    e.frame(0.0, 1).unwrap();
    e.frame(0.0, 2).unwrap();
    assert!(e.audio().iter().all(|&s| s == 0), "note off is silent");
    // 60 frames make one second of audio.
    let mut total = 0;
    for f in 0..60 {
        e.frame(0.0, f).unwrap();
        total += e.audio().len() / 2;
    }
    assert_eq!(total, 44_100);
}

#[test]
fn psg_argument_errors() {
    let mut e = load("function frame() psg(7, {on = true}) end");
    assert!(e.frame(0.0, 0).unwrap_err().message.contains("1..6"));
    let mut e = load("function frame() psg(1, {noise = 10}) end");
    assert!(e.frame(0.0, 0).unwrap_err().message.contains("5 and 6"));
    let mut e = load("function frame() wave(1, 'kazoo') end");
    assert!(e
        .frame(0.0, 0)
        .unwrap_err()
        .message
        .contains("unknown preset"));
}

#[test]
fn note_names_and_numbers_agree() {
    let mut e = load(
        "function frame() psg(1, {note = 'C#3'}) psg(2, {note = 49}) psg(3, {note = 'Db3'}) end",
    );
    e.frame(0.0, 0).unwrap();
    let p = e.psg();
    assert_eq!(p.channels[0].period, p.channels[1].period);
    assert_eq!(p.channels[0].period, p.channels[2].period);
}
