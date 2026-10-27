//! Scanline renderer: VRAM + SAT + VCE + a per-line register table in,
//! RGBA pixels and per-line sprite stats out.
//!
//! What can change per line, as on hardware with a raster interrupt:
//! horizontal scroll (BXR), a vertical scroll write (BYR), the background and
//! sprite enable bits (CR), and VCE palette writes. The sprite table is
//! frame-global: on the real machine it is copied into the VDC once per frame.

use crate::vce::{to_rgb8, Vce, SPRITE_PALETTE_BASE};
use crate::vdc::{BatSize, Vdc, SPRITE_COUNT, SPRITE_UNITS_PER_LINE};

/// Tallest frame the VDC can show.
pub const MAX_HEIGHT: usize = 242;
/// Widest frame (10.74 MHz dot clock).
pub const MAX_WIDTH: usize = 512;

/// Register state for one scanline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineRegs {
    /// BXR: horizontal background scroll, 10 bits.
    pub scroll_x: u16,
    /// A BYR write that lands on this line. The VDC keeps an internal row
    /// counter that it loads from BYR at the top of the frame and steps by one
    /// each line; a write reloads it, so this line draws background row
    /// `value` and the lines after continue from there.
    pub scroll_y_write: Option<u16>,
    /// CR bit 7.
    pub bg_on: bool,
    /// CR bit 6.
    pub sprites_on: bool,
    /// VCE writes applied before this line is drawn: `(entry, 9-bit color)`.
    /// They persist for the rest of the frame, like real palette writes.
    pub palette_writes: Vec<(u16, u16)>,
}

impl Default for LineRegs {
    fn default() -> Self {
        LineRegs {
            scroll_x: 0,
            scroll_y_write: None,
            bg_on: true,
            sprites_on: true,
            palette_writes: Vec::new(),
        }
    }
}

/// Everything one frame needs besides memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub bat: BatSize,
    /// BYR at the top of the frame.
    pub scroll_y: u16,
    pub lines: Vec<LineRegs>,
}

impl Frame {
    /// A frame with every line set to `line`.
    pub fn new(width: usize, height: usize, line: LineRegs) -> Frame {
        Frame {
            width: width.clamp(8, MAX_WIDTH),
            height: height.clamp(1, MAX_HEIGHT),
            bat: BatSize::default(),
            scroll_y: 0,
            lines: vec![line; height.clamp(1, MAX_HEIGHT)],
        }
    }

    /// Run `f` on each line of the inclusive range `y0..=y1`, the way a
    /// raster-interrupt handler would touch registers line by line.
    pub fn hdma(&mut self, y0: usize, y1: usize, mut f: impl FnMut(usize, &mut LineRegs)) {
        let last = self.lines.len().saturating_sub(1);
        for y in y0.min(last)..=y1.min(last) {
            f(y, &mut self.lines[y]);
        }
    }
}

/// Per-line sprite load, for the budget meter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineStats {
    /// Sprite units (16 px columns) that wanted this line.
    pub units: u8,
    /// Sprites that were not drawn because the line was full.
    pub dropped: u8,
}

/// Rendered frame.
pub struct Output {
    pub width: usize,
    pub height: usize,
    /// RGBA8, row-major.
    pub rgba: Vec<u8>,
    pub lines: Vec<LineStats>,
}

impl Output {
    /// True if any line hit the sprite limit (the VDC's overflow status bit).
    pub fn overflow(&self) -> bool {
        self.lines.iter().any(|l| l.dropped > 0)
    }
}

/// Render a frame. `vce` is the palette at the top of the frame; line
/// palette writes are applied on a copy.
pub fn render(vdc: &Vdc, vce: &Vce, frame: &Frame) -> Output {
    let (w, h) = (frame.width, frame.height);
    let mut rgba = vec![0u8; w * h * 4];
    let mut stats = vec![LineStats::default(); h];
    let mut pal = vce.clone();

    // Decode the SAT once.
    let sprites: Vec<_> = (0..SPRITE_COUNT).map(|i| vdc.sprite(i)).collect();

    let bat = frame.bat;
    let map_w = bat.width as u32 * 8;
    let map_h = bat.height as u32 * 8;

    // Per-line buffers: color entry (0..511) or none.
    let mut bg_line = vec![0u16; w]; // 0 = transparent, else BG entry
    let mut spr_line: Vec<Option<(u16, bool)>> = vec![None; w];

    // Burst mode: with both layers off when the frame starts, the VDC shows
    // nothing for the whole frame (it gives the CPU the VRAM bus instead),
    // whatever later lines enable. Every line shows the overscan color.
    let burst = frame
        .lines
        .first()
        .is_some_and(|l| !l.bg_on && !l.sprites_on);

    let mut row_counter = frame.scroll_y as u32;
    for y in 0..h {
        let regs = &frame.lines[y];
        for &(i, c) in &regs.palette_writes {
            pal.set(i as usize, c);
        }
        if burst {
            let [r, g, b] = to_rgb8(pal.colors[SPRITE_PALETTE_BASE]);
            for p in rgba[y * w * 4..(y + 1) * w * 4].chunks_exact_mut(4) {
                p.copy_from_slice(&[r, g, b, 255]);
            }
            continue;
        }
        if y > 0 {
            row_counter = row_counter.wrapping_add(1);
        }
        if let Some(v) = regs.scroll_y_write {
            row_counter = v as u32;
        }

        // Background.
        bg_line.iter_mut().for_each(|p| *p = 0);
        if regs.bg_on {
            let py = row_counter % map_h;
            let ty = (py / 8) as u16;
            let fy = (py % 8) as u16;
            let mut x = 0usize;
            let sx = regs.scroll_x as u32 & 0x3ff;
            while x < w {
                let px = (sx + x as u32) % map_w;
                let tx = (px / 8) as u16;
                let e = vdc.bat(bat, tx, ty);
                let base = (e.palette as u16) << 4;
                let mut fx = (px % 8) as u16;
                while fx < 8 && x < w {
                    let c = vdc.tile_pixel(e.tile, fx, fy);
                    if c != 0 {
                        bg_line[x] = base | c as u16;
                    }
                    fx += 1;
                    x += 1;
                }
            }
        }

        // Sprites: lowest SAT index wins; the line holds 16 units.
        spr_line.iter_mut().for_each(|p| *p = None);
        if regs.sprites_on {
            let mut units = 0usize;
            let yi = y as i32;
            for s in &sprites {
                if yi < s.y || yi >= s.y + s.height as i32 {
                    continue;
                }
                let cost = if s.width == 32 { 2 } else { 1 };
                stats[y].units = stats[y].units.saturating_add(cost as u8);
                if units + cost > SPRITE_UNITS_PER_LINE {
                    stats[y].dropped = stats[y].dropped.saturating_add(1);
                    continue;
                }
                units += cost;
                let mut ry = (yi - s.y) as u16;
                if s.flip_y {
                    ry = s.height as u16 - 1 - ry;
                }
                let base = SPRITE_PALETTE_BASE as u16 + ((s.palette as u16) << 4);
                for col in 0..s.width as i32 {
                    let sxp = s.x + col;
                    if sxp < 0 || sxp as usize >= w || spr_line[sxp as usize].is_some() {
                        continue;
                    }
                    let mut rx = col as u16;
                    if s.flip_x {
                        rx = s.width as u16 - 1 - rx;
                    }
                    let pat = s.cell_pattern(rx / 16, ry / 16);
                    let c = vdc.pattern_pixel(pat, rx % 16, ry % 16);
                    if c != 0 {
                        spr_line[sxp as usize] = Some((base | c as u16, s.front));
                    }
                }
            }
        }

        // Compose. With the background off and sprites on, empty pixels
        // show sprite palette 0 color 0 (entry 256, the overscan color)
        // instead of the backdrop. With both off they show the backdrop.
        // Checked against Mednafen.
        let empty = if !regs.bg_on && regs.sprites_on {
            SPRITE_PALETTE_BASE as u16
        } else {
            0
        };
        let out = &mut rgba[y * w * 4..(y + 1) * w * 4];
        for x in 0..w {
            let entry = match (spr_line[x], bg_line[x]) {
                (Some((s, front)), b) if front || b == 0 => s,
                (_, b) if b != 0 => b,
                _ => empty,
            };
            let [r, g, b] = to_rgb8(pal.colors[entry as usize]);
            out[x * 4..x * 4 + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }

    Output {
        width: w,
        height: h,
        rgba,
        lines: stats,
    }
}
