//! ROM export: the player blob, the ROM layout and the stream.
use std::path::Path;
use std::process::Command;

use tg16_core::rom::{self, RomOptions, BANK_SIZE, CONFIG_OFFSET, MAGIC, PLAYER};
use tg16_core::{Engine, RomError};

fn load(src: &str) -> Engine {
    let mut e = Engine::new();
    e.load(&[("main.lua", src)]).unwrap();
    e
}

fn export(src: &str, frames: u32, looping: bool) -> Result<rom::Rom, RomError> {
    rom::export(&mut load(src), &RomOptions { frames, looping })
}

fn word(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

/// `player/player.bin` is what `player/build.sh` makes from `player.s`.
/// Skipped when cc65 is not installed.
#[test]
fn committed_player_matches_its_source() {
    let have = |t: &str| Command::new(t).arg("--version").output().is_ok();
    if !have("ca65") || !have("ld65") {
        eprintln!("skipped: needs cc65 (ca65, ld65) to rebuild the player");
        return;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("player");
    let tmp = tempdir();
    let out = tmp.join("player.bin");
    let status = Command::new("sh")
        .arg(dir.join("build.sh"))
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success(), "player/build.sh failed");
    let built = std::fs::read(&out).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(
        built == PLAYER[..],
        "player/player.bin is stale: run crates/tg16-core/player/build.sh"
    );
}

fn tempdir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("tg16-rom-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

const TOY: &str = r#"
function init()
  bat.w, bat.h = 64, 32
  tile(128, string.rep("1", 64))
  map(0, 0, 128, 0)
  pal(0, 1, rgb3(7, 0, 0))
  pattern(64, string.rep("2", 256))
  spal(0, 2, rgb3(0, 7, 0))
  wave(1, "square")
end
function frame(t, f)
  spr(0, f, 20, 64)
  psg(1, { on = f % 2 == 0, note = "A4" })
  hdma(50, 52, function(y) bg.scroll.x = y end)
  hdma(100, 100, function() BYR = 7 end)
  hdma(120, 120, function() color(0, rgb3(0, 0, 7)) end)
end
"#;

#[test]
fn rom_has_player_config_and_vectors() {
    let r = export(TOY, 10, false).unwrap();
    let b = &r.bytes;
    assert!(b.len().is_power_of_two() && b.len() >= 2 * BANK_SIZE);
    assert_eq!(r.stats.rom_bytes, b.len());
    // Bank 0 is the player apart from the config block.
    assert_eq!(b[..CONFIG_OFFSET], PLAYER[..CONFIG_OFFSET]);
    assert_eq!(b[0x1FF6..BANK_SIZE], PLAYER[0x1FF6..]);
    let cfg = &b[CONFIG_OFFSET..];
    assert_eq!(&cfg[..8], MAGIC);
    assert_eq!(cfg[9], 0, "5.37 MHz dot clock for 256 wide");
    assert_eq!(word(cfg, 10), 0x10, "MWR for a 64x32 map");
    assert_eq!(word(cfg, 18), 223, "VDW for 224 lines");
    assert_eq!(word(cfg, 22), r.stats.satb);
    assert_eq!((cfg[24], word(cfg, 25)), (1, 0x4000), "boot record");
    // Reset vector into bank 0's $E000 window.
    assert!(word(b, 0x1FFE) >= 0xE000);
}

/// Walk the stream as the player does and return, per record, the commands
/// and raster events.
/// A record's command bytes and its raster events as `(rcr, flags)`.
type Record = (Vec<u8>, Vec<(u16, u8)>);

fn walk(b: &[u8], records: usize) -> Vec<Record> {
    let off = |bank: u8, addr: u16| bank as usize * BANK_SIZE + (addr as usize - 0x4000);
    let cfg = &b[CONFIG_OFFSET..];
    let mut p = off(cfg[24], word(cfg, 25));
    let mut out = Vec::new();
    for _ in 0..records {
        let next = off(b[p], word(b, p + 1));
        let mut q = p + 3;
        let mut cmds = Vec::new();
        loop {
            let c = b[q];
            q += 1;
            match c {
                0 => break,
                1 | 2 => {
                    let n = if b[q + 2] == 0 {
                        256
                    } else {
                        b[q + 2] as usize
                    };
                    q += 3 + 2 * n;
                }
                3 => q += 3,
                4 => {
                    assert!(b[q] >= 1 && b[q] <= 126);
                    q += 1 + 2 * b[q] as usize;
                }
                _ => panic!("bad command {c} at {q:#x}"),
            }
            cmds.push(c);
        }
        let mut rcr = word(b, q);
        q += 2;
        let mut events = Vec::new();
        while rcr != 0 {
            let flags = b[q];
            q += 1;
            if flags & 1 != 0 {
                q += 2;
            }
            if flags & 2 != 0 {
                q += 1 + 4 * b[q] as usize;
            }
            if flags & 4 != 0 {
                q += 2;
            }
            if flags & 8 != 0 {
                q += 2;
            }
            events.push((rcr, flags));
            rcr = word(b, q);
            q += 2;
        }
        out.push((cmds, events));
        p = next;
    }
    out
}

#[test]
fn stream_records_raster_events_on_the_right_lines() {
    let r = export(TOY, 3, false).unwrap();
    let recs = walk(&r.bytes, 3);
    // Boot: VRAM and palette only, no raster events.
    assert!(recs[0].0.iter().all(|&c| c == 1 || c == 2));
    assert!(recs[0].1.is_empty());
    let (cmds, events) = &recs[1];
    assert!(cmds.contains(&3) && cmds.contains(&4), "{cmds:?}");
    // BXR on lines 50..52 and back to 0 on 53: slots 50..53 (RCR = line +
    // 64). The BYR reload for line 100 goes in slot 99, a line early; the
    // backdrop write for line 120 in slot 120.
    let slots: Vec<(u16, u8)> = events.iter().map(|&(r, f)| (r - 64, f)).collect();
    assert_eq!(
        slots,
        vec![(50, 1), (51, 1), (52, 1), (53, 1), (99, 4), (120, 2)]
    );
}

#[test]
fn looping_jumps_back_to_record_one() {
    let r = export(TOY, 4, true).unwrap();
    let b = &r.bytes;
    let off = |bank: u8, addr: u16| bank as usize * BANK_SIZE + (addr as usize - 0x4000);
    // Follow next pointers: boot, 0, 1, 2, 3, loop, then back to 1.
    let cfg = &b[CONFIG_OFFSET..];
    let mut chain = vec![off(cfg[24], word(cfg, 25))];
    for _ in 0..7 {
        let p = *chain.last().unwrap();
        chain.push(off(b[p], word(b, p + 1)));
    }
    assert_eq!(chain[6], chain[2], "after the loop record comes record 1");
    let r = export(TOY, 4, false).unwrap();
    let b = &r.bytes;
    let mut p = off(cfg[24], word(cfg, 25));
    for _ in 0..5 {
        p = off(b[p], word(b, p + 1));
    }
    assert_eq!(
        off(b[p], word(b, p + 1)),
        p,
        "the hold record repeats itself"
    );
}

#[test]
fn reports_frames_that_overrun_vertical_blank() {
    // 2048 fresh VRAM words a frame is more than vertical blank can take.
    let src = r#"
function frame(t, f)
  for i = 0, 2047 do vram(0x4000 + i, (f * 7 + i) % 65536) end
end
"#;
    let r = export(src, 5, false).unwrap();
    assert!(r.stats.peak_vram_words >= 2048);
    assert_eq!(
        r.stats.over_vblank.len(),
        4,
        "every frame after the boot one"
    );
    assert!(r.warnings()[0].contains("vertical blank"));
}

#[test]
fn rejects_recordings_bigger_than_a_hucard() {
    let src = r#"
function frame(t, f)
  for i = 0, 4095 do vram(0x2000 + i, (f * 13 + i * 7) % 65536) end
end
"#;
    match export(src, 200, false) {
        Err(RomError::TooLarge { bytes }) => assert!(bytes > 1 << 20),
        other => panic!("expected TooLarge, got {:?}", other.map(|r| r.stats)),
    }
}

#[test]
fn rejects_display_changes() {
    let src = "function frame(t, f) if f == 2 then display.height = 240 end end";
    assert!(matches!(
        export(src, 4, false),
        Err(RomError::DisplayChanged { frame: 2, .. })
    ));
    assert!(matches!(export("", 0, false), Err(RomError::NoFrames)));
}

#[test]
fn satb_avoids_vram_the_toy_shows() {
    // Tiles near the top of VRAM: the SATB copy must move below them.
    let src = r#"
function init()
  tile(2047, string.rep("1", 64))
  map(0, 0, 2047, 0)
end
"#;
    let r = export(src, 2, false).unwrap();
    assert_eq!(r.stats.satb, 0x7E00);
}
