//! HuC6270 Video Display Controller: VRAM, the background map (BAT), tiles,
//! and the sprite attribute table (SAT).
//!
//! VRAM is 32K words of 16 bits. The BAT always starts at word 0, one word per
//! map cell: bits 15..12 pick the palette, bits 11..0 the tile. A tile is 16
//! words at `tile * 16`. Sprites are 16x16 patterns of 64 words at
//! `pattern * 64`, and can be stacked into 32 wide and 32 or 64 tall shapes.
//!
//! Tiles, patterns and the BAT share the same VRAM, so tile data placed in the
//! BAT's words gets overwritten by map writes (and vice versa), just like on
//! the console. `BatSize::first_free_tile` gives the lowest safe tile number.

/// VRAM size in 16-bit words.
pub const VRAM_WORDS: usize = 0x8000;

/// Number of sprites in the SAT.
pub const SPRITE_COUNT: usize = 64;

/// Sprite units the VDC can fetch on one line. A 16-wide sprite costs one,
/// a 32-wide sprite costs two.
pub const SPRITE_UNITS_PER_LINE: usize = 16;

/// Hardware offsets: SAT Y 64 is the top line, SAT X 32 is the left column.
pub const SPRITE_Y_OFFSET: i32 = 64;
pub const SPRITE_X_OFFSET: i32 = 32;

/// Virtual background size in tiles, set by the MWR register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatSize {
    pub width: u16,
    pub height: u16,
}

impl Default for BatSize {
    fn default() -> Self {
        BatSize {
            width: 32,
            height: 32,
        }
    }
}

impl BatSize {
    /// Build from tile counts. Width must be 32, 64 or 128; height 32 or 64.
    pub fn new(width: u16, height: u16) -> Option<BatSize> {
        let ok = matches!(width, 32 | 64 | 128) && matches!(height, 32 | 64);
        ok.then_some(BatSize { width, height })
    }

    /// Decode MWR bits 4..6.
    pub fn from_mwr(mwr: u16) -> BatSize {
        let width = match (mwr >> 4) & 3 {
            0 => 32,
            1 => 64,
            _ => 128,
        };
        let height = if mwr & 0x40 != 0 { 64 } else { 32 };
        BatSize { width, height }
    }

    /// Encode as MWR bits 4..6.
    pub fn to_mwr(self) -> u16 {
        let w = match self.width {
            32 => 0,
            64 => 1,
            _ => 2,
        };
        (w << 4) | if self.height == 64 { 0x40 } else { 0 }
    }

    /// Words the BAT occupies at the start of VRAM.
    pub fn words(self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Lowest tile number whose data does not overlap the BAT.
    pub fn first_free_tile(self) -> u16 {
        (self.words() / 16) as u16
    }

    /// Lowest sprite pattern number whose data does not overlap the BAT.
    pub fn first_free_pattern(self) -> u16 {
        (self.words() / 64) as u16
    }
}

/// One BAT cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatEntry {
    pub tile: u16,
    pub palette: u8,
}

impl BatEntry {
    pub fn from_word(w: u16) -> Self {
        BatEntry {
            tile: w & 0x0fff,
            palette: (w >> 12) as u8,
        }
    }
    pub fn to_word(self) -> u16 {
        (self.tile & 0x0fff) | ((self.palette as u16 & 15) << 12)
    }
}

/// One decoded sprite. Coordinates are screen pixels (the SAT offsets are
/// already removed); `to_words` puts them back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sprite {
    pub x: i32,
    pub y: i32,
    /// 16x16 pattern number, 0..1023 (VRAM word `pattern * 64`).
    pub pattern: u16,
    pub palette: u8,
    /// In front of opaque background pixels when true.
    pub front: bool,
    /// 16 or 32.
    pub width: u8,
    /// 16, 32 or 64.
    pub height: u8,
    pub flip_x: bool,
    pub flip_y: bool,
}

impl Sprite {
    pub fn from_words(w: [u16; 4]) -> Sprite {
        let height = match (w[3] >> 12) & 3 {
            0 => 16,
            1 => 32,
            _ => 64,
        };
        Sprite {
            y: (w[0] & 0x3ff) as i32 - SPRITE_Y_OFFSET,
            x: (w[1] & 0x3ff) as i32 - SPRITE_X_OFFSET,
            pattern: (w[2] >> 1) & 0x3ff,
            palette: (w[3] & 15) as u8,
            front: w[3] & 0x80 != 0,
            width: if w[3] & 0x100 != 0 { 32 } else { 16 },
            height,
            flip_x: w[3] & 0x800 != 0,
            flip_y: w[3] & 0x8000 != 0,
        }
    }

    pub fn to_words(self) -> [u16; 4] {
        let cgy = match self.height {
            32 => 1,
            64 => 3,
            _ => 0,
        };
        let attr = (self.palette as u16 & 15)
            | ((self.front as u16) << 7)
            | (((self.width == 32) as u16) << 8)
            | ((self.flip_x as u16) << 11)
            | (cgy << 12)
            | ((self.flip_y as u16) << 15);
        [
            ((self.y + SPRITE_Y_OFFSET) as u16) & 0x3ff,
            ((self.x + SPRITE_X_OFFSET) as u16) & 0x3ff,
            (self.pattern & 0x3ff) << 1,
            attr,
        ]
    }

    /// Pattern number of the 16x16 cell at column `cx`, row `cy` of this
    /// sprite, before flipping. Wide and tall sprites ignore the low pattern
    /// bits, as the hardware does.
    pub fn cell_pattern(&self, cx: u16, cy: u16) -> u16 {
        let mut base = self.pattern;
        if self.width == 32 {
            base &= !1;
        }
        match self.height {
            32 => base &= !2,
            64 => base &= !6,
            _ => {}
        }
        (base + cx + cy * 2) & 0x3ff
    }
}

/// VDC memory: VRAM and the sprite table.
#[derive(Clone, Debug)]
pub struct Vdc {
    pub vram: Box<[u16; VRAM_WORDS]>,
    pub sat: [u16; SPRITE_COUNT * 4],
}

impl Default for Vdc {
    fn default() -> Self {
        Vdc {
            vram: Box::new([0; VRAM_WORDS]),
            sat: [0; SPRITE_COUNT * 4],
        }
    }
}

impl Vdc {
    pub fn sprite(&self, i: usize) -> Sprite {
        let o = (i % SPRITE_COUNT) * 4;
        Sprite::from_words([
            self.sat[o],
            self.sat[o + 1],
            self.sat[o + 2],
            self.sat[o + 3],
        ])
    }

    pub fn set_sprite(&mut self, i: usize, s: Sprite) {
        let o = (i % SPRITE_COUNT) * 4;
        self.sat[o..o + 4].copy_from_slice(&s.to_words());
    }

    /// Write a BAT cell at map column `tx`, row `ty`.
    pub fn set_bat(&mut self, size: BatSize, tx: u16, ty: u16, e: BatEntry) {
        let i = (ty % size.height) as usize * size.width as usize + (tx % size.width) as usize;
        self.vram[i] = e.to_word();
    }

    pub fn bat(&self, size: BatSize, tx: u16, ty: u16) -> BatEntry {
        let i = (ty % size.height) as usize * size.width as usize + (tx % size.width) as usize;
        BatEntry::from_word(self.vram[i])
    }

    /// Write an 8x8 tile from 64 color indices (0..15, row-major) in the
    /// VDC's planar format: words 0..7 hold planes 0 and 1 (low and high
    /// byte), words 8..15 hold planes 2 and 3.
    pub fn write_tile(&mut self, tile: u16, pixels: &[u8; 64]) {
        let base = (tile as usize & 0x7ff) * 16;
        for row in 0..8 {
            let mut p = [0u8; 4];
            for col in 0..8 {
                let c = pixels[row * 8 + col];
                for (plane, byte) in p.iter_mut().enumerate() {
                    *byte |= ((c >> plane) & 1) << (7 - col);
                }
            }
            self.vram[base + row] = p[0] as u16 | ((p[1] as u16) << 8);
            self.vram[base + 8 + row] = p[2] as u16 | ((p[3] as u16) << 8);
        }
    }

    /// Color index (0..15) of pixel `(x, y)` of a tile.
    #[inline]
    pub fn tile_pixel(&self, tile: u16, x: u16, y: u16) -> u8 {
        let base = (tile as usize & 0x7ff) * 16 + (y as usize & 7);
        let a = self.vram[base];
        let b = self.vram[(base + 8) & (VRAM_WORDS - 1)];
        let bit = 7 - (x & 7);
        (((a >> bit) & 1)
            | (((a >> (bit + 8)) & 1) << 1)
            | (((b >> bit) & 1) << 2)
            | (((b >> (bit + 8)) & 1) << 3)) as u8
    }

    /// Write a 16x16 sprite pattern from 256 color indices (row-major).
    /// Each plane is 16 words, one per row, leftmost pixel in bit 15.
    pub fn write_pattern(&mut self, pattern: u16, pixels: &[u8; 256]) {
        let base = (pattern as usize & 0x3ff) * 64;
        for row in 0..16 {
            let mut planes = [0u16; 4];
            for col in 0..16 {
                let c = pixels[row * 16 + col];
                for (plane, word) in planes.iter_mut().enumerate() {
                    *word |= (((c >> plane) & 1) as u16) << (15 - col);
                }
            }
            for (plane, word) in planes.iter().enumerate() {
                self.vram[base + plane * 16 + row] = *word;
            }
        }
    }

    /// Color index (0..15) of pixel `(x, y)` of a 16x16 sprite pattern.
    #[inline]
    pub fn pattern_pixel(&self, pattern: u16, x: u16, y: u16) -> u8 {
        let base = (pattern as usize & 0x3ff) * 64 + (y as usize & 15);
        let bit = 15 - (x & 15);
        let mut c = 0u8;
        for plane in 0..4 {
            c |= (((self.vram[base + plane * 16] >> bit) & 1) as u8) << plane;
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mwr_roundtrip() {
        for (w, h) in [(32, 32), (64, 32), (128, 32), (32, 64), (64, 64), (128, 64)] {
            let s = BatSize::new(w, h).unwrap();
            assert_eq!(BatSize::from_mwr(s.to_mwr()), s);
        }
        assert!(BatSize::new(48, 32).is_none());
    }

    #[test]
    fn sprite_roundtrip() {
        let s = Sprite {
            x: -5,
            y: 200,
            pattern: 0x155,
            palette: 9,
            front: true,
            width: 32,
            height: 64,
            flip_x: true,
            flip_y: false,
        };
        assert_eq!(Sprite::from_words(s.to_words()), s);
    }

    #[test]
    fn sat_offsets_match_hardware() {
        let s = Sprite {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
            ..Sprite::default()
        };
        let w = s.to_words();
        assert_eq!(w[0], 64);
        assert_eq!(w[1], 32);
    }

    #[test]
    fn tall_sprite_masks_low_pattern_bits() {
        let s = Sprite {
            pattern: 7,
            width: 32,
            height: 64,
            ..Sprite::default()
        };
        assert_eq!(s.cell_pattern(0, 0), 0);
        assert_eq!(s.cell_pattern(1, 3), 7);
    }

    #[test]
    fn tile_roundtrip() {
        let mut v = Vdc::default();
        let px: [u8; 64] = std::array::from_fn(|i| (i * 7 % 16) as u8);
        v.write_tile(5, &px);
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(v.tile_pixel(5, x, y), px[(y * 8 + x) as usize]);
            }
        }
    }

    #[test]
    fn tile_layout_is_planar() {
        let mut v = Vdc::default();
        let mut px = [0u8; 64];
        px[0] = 0b1111; // top-left pixel, all planes
        v.write_tile(0, &px);
        assert_eq!(v.vram[0], 0x8080);
        assert_eq!(v.vram[8], 0x8080);
    }

    #[test]
    fn pattern_roundtrip() {
        let mut v = Vdc::default();
        let px: [u8; 256] = std::array::from_fn(|i| (i * 5 % 16) as u8);
        v.write_pattern(3, &px);
        for y in 0..16 {
            for x in 0..16 {
                assert_eq!(v.pattern_pixel(3, x, y), px[(y * 16 + x) as usize]);
            }
        }
        assert_eq!(v.vram[3 * 64] >> 15, (px[0] & 1) as u16);
    }
}
