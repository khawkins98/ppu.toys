//! A rough cost meter for a frame, so toys stay honest about what a real
//! TurboGrafx-16 could do.
//!
//! The renderer itself is exact about the sprite line limit. CPU time is an
//! estimate: the core does not run HuC6280 code, so each kind of work a game
//! would do (a raster interrupt, a VDC register write, a palette write, a
//! VRAM transfer, a PSG write) is charged a typical cycle cost. The numbers
//! are in the right range for hand-written assembly, not exact.

use crate::render::{Frame, Output};

/// HuC6280 cycles in one 60 Hz frame at 7.16 MHz.
pub const CYCLES_PER_FRAME: u32 = 7_159_090 / 60;
/// VRAM words a game can usually push during vertical blank with block
/// transfers (about 38 blank lines of CPU time at 6 cycles per byte).
pub const VBLANK_VRAM_WORDS: u32 = 1400;

/// Typical cycle costs.
pub mod cost {
    /// Vertical blank handler, SATB DMA kick, joypad read.
    pub const FRAME_BASE: u32 = 1500;
    /// Raster interrupt entry, status read, next compare, return.
    pub const RASTER_IRQ: u32 = 90;
    /// One VDC register write (select + low + high).
    pub const VDC_REG: u32 = 18;
    /// One VCE palette write (address + data).
    pub const VCE_WRITE: u32 = 20;
    /// Building one sprite's four SAT words.
    pub const SPRITE: u32 = 40;
    /// One VRAM word by block transfer.
    pub const VRAM_WORD: u32 = 12;
    /// One PSG register write.
    pub const PSG_WRITE: u32 = 12;
}

/// Work counted while `frame()` ran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub vram_words: u32,
    pub palette_writes: u32,
    pub psg_writes: u32,
    pub sprites: u32,
    /// Cycles a toy charged with `cpu(n)` for work it does in software
    /// (polygon filling, decompression, game logic).
    pub software: u32,
}

/// The meter for one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    /// Estimated CPU cycles used, against [`CYCLES_PER_FRAME`].
    pub cycles: u32,
    /// Lines where a raster hook ran.
    pub raster_lines: u32,
    /// VDC register writes the hooks imply (scroll, BYR reload, enables).
    pub vdc_writes: u32,
    pub palette_writes: u32,
    /// VRAM words written during `frame()` (not `init()`).
    pub vram_words: u32,
    /// Sprites placed this frame, of 64.
    pub sprites: u32,
    /// Cycles charged with `cpu(n)`, included in `cycles`.
    pub software: u32,
    /// Most sprite units any line wanted, of 16.
    pub peak_line_units: u8,
    /// Lines where sprites were dropped.
    pub dropped_lines: u32,
}

impl Budget {
    pub fn measure(frame: &Frame, touched: &[bool], counts: Counts, out: &Output) -> Budget {
        let raster_lines = touched.iter().filter(|&&t| t).count() as u32;
        let mut vdc_writes = 0;
        let mut prev = None;
        for (y, l) in frame.lines.iter().enumerate() {
            let changed = match prev {
                None => 0,
                Some((x, bg, spr)) => {
                    (l.scroll_x != x) as u32 + (bg != l.bg_on || spr != l.sprites_on) as u32
                }
            };
            vdc_writes += changed + l.scroll_y_write.is_some() as u32 * (y > 0) as u32;
            prev = Some((l.scroll_x, l.bg_on, l.sprites_on));
        }
        let cycles = cost::FRAME_BASE
            + raster_lines * cost::RASTER_IRQ
            + vdc_writes * cost::VDC_REG
            + counts.palette_writes * cost::VCE_WRITE
            + counts.sprites * cost::SPRITE
            + counts.vram_words * cost::VRAM_WORD
            + counts.psg_writes * cost::PSG_WRITE
            + counts.software;
        Budget {
            cycles,
            raster_lines,
            vdc_writes,
            palette_writes: counts.palette_writes,
            vram_words: counts.vram_words,
            sprites: counts.sprites,
            software: counts.software,
            peak_line_units: out.lines.iter().map(|l| l.units).max().unwrap_or(0),
            dropped_lines: out.lines.iter().filter(|l| l.dropped > 0).count() as u32,
        }
    }

    /// Share of the frame's CPU time, 0.0..
    pub fn cpu_share(&self) -> f64 {
        self.cycles as f64 / CYCLES_PER_FRAME as f64
    }

    /// Plain-language problems, empty when the frame fits.
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        if self.cycles > CYCLES_PER_FRAME {
            p.push(format!(
                "estimated CPU {:.0}% of a frame ({} raster lines, {} palette writes, {} software cycles)",
                self.cpu_share() * 100.0,
                self.raster_lines,
                self.palette_writes,
                self.software
            ));
        }
        if self.vram_words > VBLANK_VRAM_WORDS {
            p.push(format!(
                "{} VRAM words written in one frame; about {VBLANK_VRAM_WORDS} fit in vertical blank",
                self.vram_words
            ));
        }
        if self.dropped_lines > 0 {
            p.push(format!(
                "sprites dropped on {} lines (a line holds 16 units)",
                self.dropped_lines
            ));
        }
        p
    }
}
