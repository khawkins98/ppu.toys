//! HuCard ROM export: record a toy in the engine and replay it on a real
//! PC Engine / TurboGrafx-16.
//!
//! No Lua runs on the console. [`export`] plays the toy for a number of
//! frames and records, per frame, what changed: VRAM words, palette entries,
//! the sprite table, the line-0 registers, the raster events (lines where
//! BXR, BYR, CR or the palette change) and PSG register writes. A small
//! HuC6280 player (`player/player.s`, prebuilt as `player/player.bin`) applies
//! one record per vertical blank and one raster event per raster interrupt.
//!
//! # ROM layout
//!
//! - Bank 0 (`$E000-$FFFF` at reset): the player, with a config block at
//!   [`CONFIG_OFFSET`] (`$FF00`) the exporter patches: VDC timing, MWR, the
//!   VCE dot clock, the SATB address and a pointer to the boot record.
//! - Banks 1 and up: the stream, read through two 8 KB windows (`MPR2/3` at
//!   `$4000` for the vertical-blank part, `MPR4/5` at `$8000` for raster
//!   events). A stream address is a bank and an address in `$4000-$5FFF`;
//!   commands never span more than the two mapped banks.
//! - Padded with `$FF` to a power of two, at most 1 MB.
//!
//! # Stream
//!
//! The boot record (memory for frame 0, loaded with the display off), then
//! one record per frame, then one extra record: frame 0 again when looping
//! (its `next` jumps back to record 1), or a "hold" record that keeps the
//! last picture and silences the PSG (its `next` points at itself).
//!
//! ```text
//! record  := next_bank next_lo next_hi  command* 00  rcr_lo rcr_hi  event*
//! command := 01 addr_lo addr_hi n  word{n}        VRAM words (n = 0 means 256)
//!          | 02 entry_lo entry_hi n  word{n}      palette entries
//!          | 03 reg lo hi                         one VDC register
//!          | 04 n  (reg value){n}                 PSG writes, reg 0..9 = $0800+reg, n <= 126
//! event   := flags [bxr] [n (entry color){n}] [byr] [cr]  next_rcr_lo next_rcr_hi
//!            flags bit 0 BXR, bit 1 palette writes (n <= 48), bit 2 BYR, bit 3 CR
//! ```
//!
//! Words are little-endian. An RCR value of 0 means no (more) raster events.
//! Raster event `s` runs from the interrupt for `RCR = s + 64`.
//!
//! # Hardware timing the exporter compensates for
//!
//! - The sprite table reaches the VDC by SATB DMA at the start of the next
//!   vertical blank, so each record writes the *next* frame's SAT (last, after
//!   the other VRAM writes).
//! - The raster interrupt for `RCR = y + 64` arrives in the horizontal blank
//!   before line `y`. BXR and palette writes made then reach line `y` (palette
//!   writes land a few pixels into it). The VDC has already latched BYR and CR
//!   for line `y`, so those go in the event for line `y - 1`.
//! - After a mid-frame BYR write the VDC steps its row counter before the
//!   next line, so the exporter writes `row - 1` ([`BYR_RELOAD_ADJUST`]). A
//!   BYR written in vertical blank needs no adjustment: line 0 shows row BYR.
//! - Line-0 registers and palette writes are applied in vertical blank.
//! - Palette writes from raster hooks persist on hardware; the model starts
//!   every frame from the palette `frame()` left, so each record restores the
//!   entries the previous frame's hooks changed.
//!
//! All of this was measured with Mednafen's accurate `pce` module (see
//! `crates/ppu-cli/tests/tg16_rom_emu.rs`), not on a console.

use crate::budget::{cost, VBLANK_VRAM_WORDS};
use crate::psg::{Psg, CHANNELS};
use crate::render::{Frame, LineRegs};
use crate::vce::{DotClock, VCE_ENTRIES};
use crate::vdc::{BatSize, Vdc, SPRITE_COUNT, VRAM_WORDS};
use crate::Engine;

/// The assembled player: ROM bank 0.
pub const PLAYER: &[u8; BANK_SIZE] = include_bytes!("../player/player.bin");
/// Where the config block sits in bank 0 (`$FF00`).
pub const CONFIG_OFFSET: usize = 0x1F00;
/// First bytes of the config block.
pub const MAGIC: &[u8; 8] = b"PPUTG16R";
/// Config block format version the player understands.
pub const CONFIG_VERSION: u8 = 1;
pub const BANK_SIZE: usize = 8192;
/// Largest plain HuCard (no Street Fighter II mapper).
pub const MAX_ROM_BYTES: usize = 1 << 20;
/// Raster compare value for display line 0.
pub const RCR_BASE: i32 = 64;
/// Added to `s + RCR_BASE` for raster event `s`. Checked against Mednafen;
/// see `crates/ppu-cli/tests/tg16_rom_emu.rs`.
pub const RCR_ADJUST: i32 = 0;
/// Added to a background row to get the BYR value that shows it after a
/// mid-frame reload: the VDC loads its row counter from BYR and steps it
/// before the next line, so writing `row - 1` during line `y - 1` shows `row`
/// on line `y`. (A BYR written in vertical blank needs no adjustment: line 0
/// shows row BYR.) Checked against Mednafen.
pub const BYR_RELOAD_ADJUST: i32 = -1;
/// Most palette writes one raster event may carry: each costs about 40
/// cycles and a line is 455.
pub const MAX_LINE_PALETTE_WRITES: usize = 48;
/// CPU cycles in one scanline at 7.16 MHz.
pub const CYCLES_PER_LINE: u32 = 455;
/// Scanlines per frame (the VCE's 263-line mode).
pub const LINES_PER_FRAME: u32 = 263;

const SAT_WORDS: usize = SPRITE_COUNT * 4;
/// PSG writes per stream command (2 bytes each, read with an 8-bit index).
const MAX_PSG_WRITES: usize = 126;

/// What to record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomOptions {
    /// Frames to record (60 per second).
    pub frames: u32,
    /// Jump back to frame 1 after the last frame instead of holding it.
    pub looping: bool,
}

/// Why a toy could not be exported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RomError {
    NoFrames,
    Lua {
        frame: u32,
        message: String,
    },
    /// Display size or BAT size changed after frame 0.
    DisplayChanged {
        frame: u32,
        what: String,
    },
    UnsupportedDisplay(String),
    /// No 256-word gap in VRAM for the sprite table copy.
    NoSatbSpace,
    TooManyPaletteWrites {
        frame: u32,
        line: usize,
        writes: usize,
    },
    TooLarge {
        bytes: usize,
    },
}

impl std::fmt::Display for RomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RomError::NoFrames => write!(f, "record at least one frame"),
            RomError::Lua { frame, message } => write!(f, "frame {frame}: {message}"),
            RomError::DisplayChanged { frame, what } => write!(
                f,
                "{what} changed at frame {frame}; a ROM keeps the display and map size of frame 0"
            ),
            RomError::UnsupportedDisplay(m) => write!(f, "{m}"),
            RomError::NoSatbSpace => write!(
                f,
                "no free 256-word block in VRAM for the sprite table copy (SATB); \
                 the toy uses every part of VRAM that the map, tiles or sprites show"
            ),
            RomError::TooManyPaletteWrites {
                frame,
                line,
                writes,
            } => write!(
                f,
                "frame {frame}, line {line}: {writes} palette writes in one raster hook; \
                 at most {MAX_LINE_PALETTE_WRITES} fit in a scanline"
            ),
            RomError::TooLarge { bytes } => write!(
                f,
                "the recording needs {bytes} bytes; a HuCard holds {MAX_ROM_BYTES}. \
                 Record fewer seconds, or upload less VRAM per frame"
            ),
        }
    }
}

impl std::error::Error for RomError {}

/// Sizes and the frames that will not keep up on hardware.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RomStats {
    pub frames: u32,
    pub stream_bytes: usize,
    pub rom_bytes: usize,
    /// VRAM word address of the sprite table copy.
    pub satb: u16,
    /// Estimated vertical-blank cycles of the busiest record, and its frame.
    pub peak_vblank_cycles: u32,
    pub peak_vblank_frame: u32,
    /// Cycles available in vertical blank for this display height.
    pub vblank_cycles: u32,
    /// Frames whose record will likely not finish within vertical blank:
    /// the picture tears and that frame's first raster events can be missed.
    pub over_vblank: Vec<u32>,
    /// Most VRAM words (including the SAT copy) one record writes.
    pub peak_vram_words: u32,
    /// Most raster events in one frame.
    pub peak_raster_events: u32,
    /// `(frame, line)` of raster events too slow to finish within their line.
    pub slow_raster_events: Vec<(u32, usize)>,
}

/// A built ROM.
#[derive(Clone, Debug)]
pub struct Rom {
    pub bytes: Vec<u8>,
    pub stats: RomStats,
}

impl Rom {
    /// Plain-language warnings about frames that will not keep up.
    pub fn warnings(&self) -> Vec<String> {
        let s = &self.stats;
        let mut w = Vec::new();
        if !s.over_vblank.is_empty() {
            w.push(format!(
                "{} frames need more than vertical blank (first at frame {}; busiest ~{} of {} cycles \
                 at frame {}): expect tearing there",
                s.over_vblank.len(),
                s.over_vblank[0],
                s.peak_vblank_cycles,
                s.vblank_cycles,
                s.peak_vblank_frame
            ));
        }
        if !s.slow_raster_events.is_empty() {
            let (f, y) = s.slow_raster_events[0];
            w.push(format!(
                "{} raster events take longer than a scanline (first at frame {f}, line {y}); \
                 the next line's event will run late",
                s.slow_raster_events.len()
            ));
        }
        w
    }
}

/// VDC and VCE timing for a display size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub vce: u8,
    pub hsr: u16,
    pub hdr: u16,
    pub vpr: u16,
    pub vdw: u16,
    pub vcr: u16,
}

impl Timing {
    /// Timing registers that show `width` x `height` pixels, centred.
    /// 256 x 224 uses the values most games use; other sizes are derived
    /// from the same line and frame totals.
    pub fn for_display(width: usize, height: usize) -> Result<Timing, RomError> {
        let clock = DotClock::for_width(width).ok_or_else(|| {
            RomError::UnsupportedDisplay(format!("display.width {width} is wider than 512"))
        })?;
        if !width.is_multiple_of(8) || !(8..=512).contains(&width) {
            return Err(RomError::UnsupportedDisplay(format!(
                "display.width {width} must be a multiple of 8"
            )));
        }
        if !(16..=242).contains(&height) {
            return Err(RomError::UnsupportedDisplay(format!(
                "display.height {height} must be 16..242"
            )));
        }
        let tiles = (width / 8) as i32;
        // Line length in 8-dot units and the sync width for each dot clock.
        let (vce, total, hsw) = match clock {
            DotClock::Low => (0u8, 43, 2),
            DotClock::Mid => (1, 57, 3),
            DotClock::High => (2, 85, 5),
        };
        let rest = (total - tiles - (hsw + 1)).max(2);
        let (hds, hde) = if clock == DotClock::Low && tiles == 32 {
            (2, 4)
        } else {
            let hds = (rest / 2 - 1).max(0);
            (hds, (rest - hds - 2).max(0))
        };
        let h = height as i32;
        let vsw = 2;
        let vds = (15 + (240 - h) / 2).clamp(2, 255);
        let vcr = (LINES_PER_FRAME as i32 - (vsw + 1) - (vds + 2) - h).max(3);
        Ok(Timing {
            vce,
            hsr: ((hds as u16) << 8) | hsw as u16,
            hdr: ((hde as u16) << 8) | (tiles - 1) as u16,
            vpr: ((vds as u16) << 8) | vsw as u16,
            vdw: (h - 1) as u16,
            vcr: vcr as u16,
        })
    }
}

/// One recorded frame.
struct Snap {
    frame: Frame,
    sat: [u16; SAT_WORDS],
    vce: [u16; VCE_ENTRIES],
    psg: Psg,
    /// VRAM words that changed since the previous frame.
    vram_diff: Vec<(u16, u16)>,
}

/// Record `opts.frames` frames of a loaded toy and build a ROM.
///
/// `engine` must have the toy loaded (its `init()` has run); frames are played
/// from 0 with `t = f / 60`.
pub fn export(engine: &mut Engine, opts: &RomOptions) -> Result<Rom, RomError> {
    if opts.frames == 0 {
        return Err(RomError::NoFrames);
    }
    // Pass 1: play and record.
    let mut snaps = Vec::with_capacity(opts.frames as usize);
    let mut vram0: Option<Box<[u16; VRAM_WORDS]>> = None;
    let mut cur: Box<[u16; VRAM_WORDS]> = Box::new([0; VRAM_WORDS]);
    let mut used = vec![false; VRAM_WORDS];
    for f in 0..opts.frames {
        let frame = engine
            .frame(f64::from(f) / 60.0, f)
            .map_err(|e| RomError::Lua {
                frame: f,
                message: e.to_string(),
            })?;
        let vdc = engine.vdc();
        let mut vram_diff = Vec::new();
        if vram0.is_none() {
            vram0 = Some(vdc.vram.clone());
            cur.copy_from_slice(&vdc.vram[..]);
        } else {
            for (i, (c, &n)) in cur.iter_mut().zip(vdc.vram.iter()).enumerate() {
                if *c != n {
                    *c = n;
                    vram_diff.push((i as u16, n));
                }
            }
        }
        mark_used(&vdc, &frame, &mut used);
        if let Some(first) = snaps.first() {
            let first: &Snap = first;
            let what = if first.frame.width != frame.width {
                Some("display.width")
            } else if first.frame.height != frame.height {
                Some("display.height")
            } else if first.frame.bat != frame.bat {
                Some("the map size (bat.w, bat.h)")
            } else {
                None
            };
            if let Some(what) = what {
                return Err(RomError::DisplayChanged {
                    frame: f,
                    what: what.into(),
                });
            }
        }
        snaps.push(Snap {
            sat: vdc.sat,
            vce: engine.vce().colors,
            psg: engine.psg().clone(),
            vram_diff,
            frame,
        });
    }
    let vram0 = vram0.expect("at least one frame");
    let first = &snaps[0].frame;
    let timing = Timing::for_display(first.width, first.height)?;
    let satb = find_satb(&used).ok_or(RomError::NoSatbSpace)?;

    // Pass 2: encode.
    let n = snaps.len();
    let mut hw = Hw {
        vram: Box::new([0; VRAM_WORDS]),
        pal: [0; VCE_ENTRIES],
        sat: [0; SAT_WORDS],
        psg: PsgHw::default(),
    };
    let vblank_cycles = (LINES_PER_FRAME - first.height as u32) * CYCLES_PER_LINE;
    let budget_cycles = (VBLANK_VRAM_WORDS * cost::VRAM_WORD).min(vblank_cycles);
    let mut stats = RomStats {
        frames: opts.frames,
        satb,
        vblank_cycles: budget_cycles,
        ..Default::default()
    };

    // Boot: frame 0's VRAM, palette and sprite table, display off.
    let mut bodies: Vec<Vec<u8>> = Vec::with_capacity(n + 2);
    {
        let mut b = Vec::new();
        let mut c = Cost::default();
        vram_commands(&mut hw.vram[..], &vram0[..], satb, &mut b, &mut c);
        let mut pal = snaps[0].vce;
        for &(i, v) in &snaps[0].frame.lines[0].palette_writes {
            pal[i as usize & 511] = v;
        }
        palette_commands(&mut hw.pal, &pal, &mut b, &mut c);
        sat_commands(&mut hw.sat, &snaps[0].sat, satb, &mut b, &mut c);
        b.extend_from_slice(&[0, 0, 0]); // end, no raster events
        bodies.push(b);
    }

    // Records 0..n-1, then the loop or hold record.
    let mut vram = vram0.clone();
    for r in 0..=n {
        let (snap, next_sat_idx, psg) = if r < n {
            for &(i, v) in &snaps[r].vram_diff {
                vram[i as usize] = v;
            }
            let next = if r + 1 < n {
                r + 1
            } else if opts.looping {
                0
            } else {
                n - 1
            };
            (&snaps[r], next, snaps[r].psg.clone())
        } else if opts.looping {
            vram.copy_from_slice(&vram0[..]);
            (&snaps[0], 1.min(n - 1), snaps[0].psg.clone())
        } else {
            let mut quiet = snaps[n - 1].psg.clone();
            quiet.channels.iter_mut().for_each(|c| c.on = false);
            (&snaps[n - 1], n - 1, quiet)
        };
        let frame_no = if r < n { r as u32 } else { 0 };
        let mut b = Vec::new();
        let mut c = Cost::default();
        encode_record(
            &mut hw,
            &vram[..],
            snap,
            &snaps[next_sat_idx].sat,
            &psg,
            satb,
            frame_no,
            &mut b,
            &mut c,
            &mut stats,
        )?;
        if r < n {
            if c.cycles > stats.peak_vblank_cycles {
                stats.peak_vblank_cycles = c.cycles;
                stats.peak_vblank_frame = frame_no;
            }
            if c.cycles > budget_cycles {
                stats.over_vblank.push(frame_no);
            }
            stats.peak_vram_words = stats.peak_vram_words.max(c.vram_words);
        }
        bodies.push(b);
    }

    // Lay out: boot, records 0..=n. Body k (0 = boot) starts at offsets[k].
    let mut offsets = Vec::with_capacity(bodies.len());
    let mut o = 0usize;
    for b in &bodies {
        offsets.push(o);
        o += 3 + b.len();
    }
    let stream_len = o;
    let rom_len = (BANK_SIZE + stream_len).next_power_of_two();
    if rom_len > MAX_ROM_BYTES {
        return Err(RomError::TooLarge {
            bytes: BANK_SIZE + stream_len,
        });
    }
    let target = |k: usize| -> usize {
        // Boot (0) runs record 0 (body 1) next; record r is body r + 1.
        let last = bodies.len() - 1;
        if k < last {
            k + 1
        } else if opts.looping {
            2.min(last) // record 1
        } else {
            last
        }
    };
    let mut rom = vec![0xFFu8; rom_len];
    rom[..BANK_SIZE].copy_from_slice(PLAYER);
    let mut pos = BANK_SIZE;
    for (k, b) in bodies.iter().enumerate() {
        let (bank, addr) = stream_address(offsets[target(k)]);
        rom[pos] = bank;
        rom[pos + 1..pos + 3].copy_from_slice(&addr.to_le_bytes());
        rom[pos + 3..pos + 3 + b.len()].copy_from_slice(b);
        pos += 3 + b.len();
    }
    let cfg = &mut rom[CONFIG_OFFSET..CONFIG_OFFSET + 27];
    cfg[..8].copy_from_slice(MAGIC);
    cfg[8] = CONFIG_VERSION;
    cfg[9] = timing.vce;
    let words = [
        frame_mwr(snaps[0].frame.bat),
        timing.hsr,
        timing.hdr,
        timing.vpr,
        timing.vdw,
        timing.vcr,
        satb,
    ];
    for (i, w) in words.iter().enumerate() {
        cfg[10 + i * 2..12 + i * 2].copy_from_slice(&w.to_le_bytes());
    }
    let (bank, addr) = stream_address(0);
    cfg[24] = bank;
    cfg[25..27].copy_from_slice(&addr.to_le_bytes());

    stats.stream_bytes = stream_len;
    stats.rom_bytes = rom_len;
    Ok(Rom { bytes: rom, stats })
}

/// MWR for a map size (with the default VRAM access width).
fn frame_mwr(bat: BatSize) -> u16 {
    bat.to_mwr()
}

/// Bank and `$4000`-window address of stream offset `o`.
fn stream_address(o: usize) -> (u8, u16) {
    ((1 + o / BANK_SIZE) as u8, 0x4000 + (o % BANK_SIZE) as u16)
}

/// Mark VRAM words the picture depends on: the BAT, tiles the BAT names and
/// the patterns of on-screen sprites.
fn mark_used(vdc: &Vdc, frame: &Frame, used: &mut [bool]) {
    let bat = frame.bat;
    for i in 0..bat.words() {
        used[i] = true;
        let tile = (vdc.vram[i] & 0x7ff) as usize;
        used[tile * 16..tile * 16 + 16].fill(true);
    }
    for i in 0..SPRITE_COUNT {
        let s = vdc.sprite(i);
        let visible = s.y + (s.height as i32) > 0
            && s.y < frame.height as i32
            && s.x + (s.width as i32) > 0
            && s.x < frame.width as i32;
        if !visible {
            continue;
        }
        for cy in 0..s.height as u16 / 16 {
            for cx in 0..s.width as u16 / 16 {
                let p = s.cell_pattern(cx, cy) as usize;
                used[p * 64..p * 64 + 64].fill(true);
            }
        }
    }
}

/// Highest 256-word block (256-aligned) nothing on screen uses.
fn find_satb(used: &[bool]) -> Option<u16> {
    (0..VRAM_WORDS / SAT_WORDS)
        .rev()
        .map(|b| b * SAT_WORDS)
        .find(|&a| !used[a..a + SAT_WORDS].iter().any(|&u| u))
        .map(|a| a as u16)
}

/// What the console holds, as far as the stream has told it.
struct Hw {
    vram: Box<[u16; VRAM_WORDS]>,
    /// Palette after the last frame's raster writes.
    pal: [u16; VCE_ENTRIES],
    /// The SAT copy in VRAM at the SATB address.
    sat: [u16; SAT_WORDS],
    psg: PsgHw,
}

#[derive(Default)]
struct Cost {
    cycles: u32,
    vram_words: u32,
}

/// Group sorted indices into runs, joining gaps of up to `gap` unchanged
/// entries (cheaper than a new command) and splitting at 256.
fn runs(idx: &[usize], gap: usize) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &i in idx {
        match out.last_mut() {
            Some((s, l)) if i < *s + *l + gap + 1 && i - *s < 256 => *l = i - *s + 1,
            _ => out.push((i, 1)),
        }
    }
    out
}

fn push_words(b: &mut Vec<u8>, words: &[u16]) {
    for w in words {
        b.extend_from_slice(&w.to_le_bytes());
    }
}

/// VRAM commands that turn `hw` into `want`, leaving the SATB block alone.
fn vram_commands(hw: &mut [u16], want: &[u16], satb: u16, b: &mut Vec<u8>, c: &mut Cost) {
    let satb = satb as usize;
    for range in [0..satb, satb + SAT_WORDS..VRAM_WORDS] {
        let idx: Vec<usize> = range.clone().filter(|&i| hw[i] != want[i]).collect();
        for (s, l) in runs(&idx, 2) {
            vram_run(b, c, s as u16, &want[s..s + l]);
            hw[s..s + l].copy_from_slice(&want[s..s + l]);
        }
    }
}

fn vram_run(b: &mut Vec<u8>, c: &mut Cost, addr: u16, words: &[u16]) {
    b.push(1);
    b.extend_from_slice(&addr.to_le_bytes());
    b.push(words.len() as u8); // 256 wraps to 0
    push_words(b, words);
    c.cycles += 70 + 12 * words.len() as u32;
    c.vram_words += words.len() as u32;
}

fn palette_commands(
    hw: &mut [u16; VCE_ENTRIES],
    want: &[u16; VCE_ENTRIES],
    b: &mut Vec<u8>,
    c: &mut Cost,
) {
    let idx: Vec<usize> = (0..VCE_ENTRIES).filter(|&i| hw[i] != want[i]).collect();
    for (s, l) in runs(&idx, 1) {
        b.push(2);
        b.extend_from_slice(&(s as u16).to_le_bytes());
        b.push(l as u8);
        push_words(b, &want[s..s + l]);
        hw[s..s + l].copy_from_slice(&want[s..s + l]);
        c.cycles += 60 + 12 * l as u32;
    }
}

fn sat_commands(
    hw: &mut [u16; SAT_WORDS],
    want: &[u16; SAT_WORDS],
    satb: u16,
    b: &mut Vec<u8>,
    c: &mut Cost,
) {
    let idx: Vec<usize> = (0..SAT_WORDS).filter(|&i| hw[i] != want[i]).collect();
    for (s, l) in runs(&idx, 2) {
        vram_run(b, c, satb + s as u16, &want[s..s + l]);
        hw[s..s + l].copy_from_slice(&want[s..s + l]);
    }
}

fn cr_word(l: &LineRegs) -> u16 {
    // Background and sprite enables plus the raster and vertical-blank
    // interrupt enables the player runs on. VRAM address step 1.
    ((l.bg_on as u16) << 7) | ((l.sprites_on as u16) << 6) | 0x0C
}

/// BYR written in vertical blank: line 0 shows row BYR.
fn byr_top(row: u16) -> u16 {
    row & 0x1ff
}

/// BYR written during line `y - 1` so that line `y` shows `row`.
fn byr_reload(row: u16) -> u16 {
    ((row as i32 + BYR_RELOAD_ADJUST) & 0x1ff) as u16
}

fn rcr_word(slot: usize) -> u16 {
    (slot as i32 + RCR_BASE + RCR_ADJUST) as u16
}

#[allow(clippy::too_many_arguments)]
fn encode_record(
    hw: &mut Hw,
    vram: &[u16],
    snap: &Snap,
    next_sat: &[u16; SAT_WORDS],
    psg: &Psg,
    satb: u16,
    frame_no: u32,
    b: &mut Vec<u8>,
    c: &mut Cost,
    stats: &mut RomStats,
) -> Result<(), RomError> {
    let frame = &snap.frame;
    c.cycles += cost::FRAME_BASE;
    vram_commands(&mut hw.vram[..], vram, satb, b, c);

    // Palette at the top of the frame, with line 0's hook writes folded in.
    let mut pal = snap.vce;
    let l0 = &frame.lines[0];
    for &(i, v) in &l0.palette_writes {
        pal[i as usize & 511] = v & 0x1ff;
    }
    palette_commands(&mut hw.pal, &pal, b, c);

    // Line 0 registers. BYR is always written: the previous frame's
    // reloads changed it.
    let row0 = l0.scroll_y_write.unwrap_or(frame.scroll_y);
    for (reg, v) in [
        (7u8, l0.scroll_x & 0x3ff),
        (8, byr_top(row0)),
        (5, cr_word(l0)),
    ] {
        b.push(3);
        b.push(reg);
        b.extend_from_slice(&v.to_le_bytes());
        c.cycles += 40;
    }

    let writes = psg_writes(&mut hw.psg, psg);
    // The player indexes a command with Y, so one command stays under 256 bytes.
    for chunk in writes.chunks(MAX_PSG_WRITES) {
        b.push(4);
        b.push(chunk.len() as u8);
        for &(r, v) in chunk {
            b.push(r);
            b.push(v);
        }
        c.cycles += 30 + cost::PSG_WRITE * chunk.len() as u32;
    }

    // The sprite table for the next frame, last (SATB DMA reads it at the
    // start of the next vertical blank).
    sat_commands(&mut hw.sat, next_sat, satb, b, c);
    b.push(0);

    // Raster events. The interrupt for slot `s` (RCR = s + 64) arrives just
    // before line `s`: BXR and palette writes made then reach line `s`, BYR
    // and CR writes reach line `s + 1`.
    let h = frame.lines.len();
    let mut events: Vec<(usize, Vec<u8>)> = Vec::new();
    for s in 0..h {
        let (l, prev) = (&frame.lines[s], &frame.lines[s.saturating_sub(1)]);
        let next = frame.lines.get(s + 1);
        let mut flags = 0u8;
        let mut e = Vec::new();
        let mut cyc = 110;
        if s > 0 && l.scroll_x != prev.scroll_x {
            flags |= 1;
            e.extend_from_slice(&(l.scroll_x & 0x3ff).to_le_bytes());
            cyc += 30;
        }
        if s > 0 && !l.palette_writes.is_empty() {
            let n = l.palette_writes.len();
            if n > MAX_LINE_PALETTE_WRITES {
                return Err(RomError::TooManyPaletteWrites {
                    frame: frame_no,
                    line: s,
                    writes: n,
                });
            }
            flags |= 2;
            e.push(n as u8);
            for &(i, v) in &l.palette_writes {
                e.extend_from_slice(&(i & 511).to_le_bytes());
                e.extend_from_slice(&(v & 0x1ff).to_le_bytes());
            }
            cyc += 40 * n as u32;
        }
        if let Some(nl) = next {
            if let Some(v) = nl.scroll_y_write {
                flags |= 4;
                e.extend_from_slice(&byr_reload(v).to_le_bytes());
                cyc += 30;
            }
            if cr_word(nl) != cr_word(l) {
                flags |= 8;
                e.extend_from_slice(&cr_word(nl).to_le_bytes());
                cyc += 30;
            }
        }
        if flags == 0 {
            continue;
        }
        if cyc > CYCLES_PER_LINE {
            stats.slow_raster_events.push((frame_no, s));
        }
        let mut ev = vec![flags];
        ev.extend_from_slice(&e);
        events.push((s, ev));
    }
    stats.peak_raster_events = stats.peak_raster_events.max(events.len() as u32);
    let first = events.first().map_or(0, |(y, _)| rcr_word(*y));
    b.extend_from_slice(&first.to_le_bytes());
    for (k, (_, ev)) in events.iter().enumerate() {
        b.extend_from_slice(ev);
        let next = events.get(k + 1).map_or(0, |(y, _)| rcr_word(*y));
        b.extend_from_slice(&next.to_le_bytes());
    }

    // The palette the console ends the frame with.
    hw.pal = pal;
    for l in &frame.lines[1..] {
        for &(i, v) in &l.palette_writes {
            hw.pal[i as usize & 511] = v & 0x1ff;
        }
    }
    Ok(())
}

/// One PSG channel's registers as last written.
#[derive(Clone, PartialEq, Eq)]
struct ChanHw {
    freq: u16,
    ctrl: u8,
    balance: u8,
    wave: [u8; 32],
    noise: u8,
    dda_level: u8,
}

#[derive(Default)]
struct PsgHw {
    ch: [Option<ChanHw>; CHANNELS],
    master: Option<u8>,
    lfo_freq: Option<u8>,
    lfo_ctrl: Option<u8>,
}

/// PSG register writes (`$0800 + reg`, value) that bring `hw` to `psg`.
fn psg_writes(hw: &mut PsgHw, psg: &Psg) -> Vec<(u8, u8)> {
    let mut w = Vec::new();
    for (ch, c) in psg.channels.iter().enumerate() {
        let noisy = ch >= 4 && c.noise;
        let on = c.on && (c.dda || noisy || c.period != 0);
        let want = ChanHw {
            freq: c.period & 0xfff,
            ctrl: ((on as u8) << 7) | ((c.dda as u8) << 6) | (c.volume & 31),
            balance: ((c.left & 15) << 4) | (c.right & 15),
            wave: c.wave.map(|s| s & 31),
            noise: if noisy { 0x80 | (c.noise_freq & 31) } else { 0 },
            dda_level: c.dda_level & 31,
        };
        let old = hw.ch[ch].as_ref();
        if old == Some(&want) {
            continue;
        }
        w.push((0, ch as u8));
        let mut ctrl_dirty = old.is_none_or(|o| o.ctrl != want.ctrl);
        if old.is_none_or(|o| o.wave != want.wave) {
            // Channel off with DDA set resets the wave index; then 32 samples.
            w.push((4, 0x40));
            w.push((4, 0x00));
            w.extend(want.wave.iter().map(|&s| (6, s)));
            ctrl_dirty = true;
        }
        if old.is_none_or(|o| o.freq != want.freq) {
            w.push((2, want.freq as u8));
            w.push((3, (want.freq >> 8) as u8));
        }
        if old.is_none_or(|o| o.balance != want.balance) {
            w.push((5, want.balance));
        }
        if ch >= 4 && old.is_none_or(|o| o.noise != want.noise) {
            w.push((7, want.noise));
        }
        if ctrl_dirty {
            w.push((4, want.ctrl));
        }
        if c.dda && (ctrl_dirty || old.is_none_or(|o| o.dda_level != want.dda_level)) {
            w.push((6, want.dda_level));
        }
        hw.ch[ch] = Some(want);
    }
    let master = ((psg.master_left & 15) << 4) | (psg.master_right & 15);
    if hw.master != Some(master) {
        w.push((1, master));
        hw.master = Some(master);
    }
    if hw.lfo_freq != Some(psg.lfo.freq) {
        w.push((8, psg.lfo.freq));
        hw.lfo_freq = Some(psg.lfo.freq);
    }
    let lfo_ctrl = psg.lfo.mode & 3;
    if hw.lfo_ctrl != Some(lfo_ctrl) {
        w.push((9, lfo_ctrl));
        hw.lfo_ctrl = Some(lfo_ctrl);
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_has_config_and_vectors() {
        assert_eq!(&PLAYER[CONFIG_OFFSET..CONFIG_OFFSET + 8], MAGIC);
        assert_eq!(PLAYER[CONFIG_OFFSET + 8], CONFIG_VERSION);
        // Reset vector points into bank 0's window.
        let reset = u16::from_le_bytes([PLAYER[0x1FFE], PLAYER[0x1FFF]]);
        assert!(reset >= 0xE000);
    }

    #[test]
    fn standard_timing_for_256x224() {
        let t = Timing::for_display(256, 224).unwrap();
        assert_eq!(t.vce, 0);
        assert_eq!(t.hsr, 0x0202);
        assert_eq!(t.hdr, 0x041F);
        assert_eq!(t.vdw, 223);
        assert_eq!(t.vpr, 0x1702);
        let t = Timing::for_display(256, 240).unwrap();
        assert_eq!(t.vpr, 0x0F02);
        assert_eq!(t.vcr, 3);
        assert_eq!(Timing::for_display(336, 224).unwrap().vce, 1);
        assert!(Timing::for_display(600, 224).is_err());
    }

    #[test]
    fn runs_join_small_gaps_and_split_at_256() {
        assert_eq!(runs(&[1, 2, 5, 9], 2), vec![(1, 5), (9, 1)]);
        let long: Vec<usize> = (0..300).collect();
        assert_eq!(runs(&long, 0), vec![(0, 256), (256, 44)]);
    }

    #[test]
    fn satb_avoids_used_words() {
        let mut used = vec![false; VRAM_WORDS];
        assert_eq!(find_satb(&used), Some(0x7F00));
        used[0x7F80] = true;
        assert_eq!(find_satb(&used), Some(0x7E00));
        used.fill(true);
        assert_eq!(find_satb(&used), None);
    }

    #[test]
    fn vram_runs_skip_the_satb_block() {
        let mut hw = vec![0u16; VRAM_WORDS];
        let mut want = vec![0u16; VRAM_WORDS];
        for w in &mut want[0x7EF0..0x7F10] {
            *w = 1;
        }
        let mut b = Vec::new();
        vram_commands(&mut hw, &want, 0x7F00, &mut b, &mut Cost::default());
        assert!(hw[0x7EF0..0x7F00].iter().all(|&w| w == 1));
        assert!(hw[0x7F00..0x7F10].iter().all(|&w| w == 0));
    }

    #[test]
    fn psg_writes_only_what_changed() {
        let mut hw = PsgHw::default();
        let mut p = Psg::default();
        let first = psg_writes(&mut hw, &p);
        assert!(first.len() > 6 * 32);
        assert!(psg_writes(&mut hw, &p).is_empty());
        p.channels[2].period = 300;
        p.channels[2].on = true;
        let w = psg_writes(&mut hw, &p);
        assert_eq!(w, vec![(0, 2), (2, 44), (3, 1), (4, 0x80 | 31)]);
    }

    #[test]
    fn stream_addresses_use_the_4000_window() {
        assert_eq!(stream_address(0), (1, 0x4000));
        assert_eq!(stream_address(8191), (1, 0x5FFF));
        assert_eq!(stream_address(8192), (2, 0x4000));
    }
}
