//! HuC6260 Video Color Encoder: the palette RAM and the dot clock.
//!
//! The VCE holds 512 colors of 9 bits each (3 bits each of green, red and
//! blue, packed GRB). Entries 0..255 are the 16 background palettes, entries
//! 256..511 are the 16 sprite palettes. Entry 0 doubles as the backdrop:
//! wherever no background or sprite pixel is opaque, the screen shows it.

/// Number of palette entries in the VCE.
pub const VCE_ENTRIES: usize = 512;

/// First sprite palette entry.
pub const SPRITE_PALETTE_BASE: usize = 256;

/// Pack 3-bit channels into the VCE's 9-bit GRB format.
pub const fn grb(r: u8, g: u8, b: u8) -> u16 {
    (((g & 7) as u16) << 6) | (((r & 7) as u16) << 3) | ((b & 7) as u16)
}

/// Pack 8-bit channels into 9-bit GRB, rounding to the nearest level.
pub fn rgb8(r: u8, g: u8, b: u8) -> u16 {
    let q = |c: u8| ((c as u32 * 7 + 127) / 255) as u8;
    grb(q(r), q(g), q(b))
}

/// Expand a 3-bit channel to 8 bits so 0 maps to 0 and 7 maps to 255.
#[inline]
pub const fn expand3(v: u16) -> u8 {
    let v = (v & 7) as u8;
    (v << 5) | (v << 2) | (v >> 1)
}

/// Convert a 9-bit GRB color to 8-bit RGB.
#[inline]
pub const fn to_rgb8(c: u16) -> [u8; 3] {
    [expand3(c >> 3), expand3(c >> 6), expand3(c)]
}

/// The dot clock the VCE drives the VDC with. It sets how many pixels fit
/// across the screen; the VDC's HDW register picks the exact count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DotClock {
    /// 5.37 MHz: about 256 pixels wide. Most games use this.
    #[default]
    Low,
    /// 7.16 MHz: about 336 pixels wide.
    Mid,
    /// 10.74 MHz: about 512 pixels wide.
    High,
}

impl DotClock {
    /// The usual visible width for this clock.
    pub const fn typical_width(self) -> usize {
        match self {
            DotClock::Low => 256,
            DotClock::Mid => 336,
            DotClock::High => 512,
        }
    }

    /// The widest the VDC can show at this clock.
    pub const fn max_width(self) -> usize {
        match self {
            DotClock::Low => 288,
            DotClock::Mid => 352,
            DotClock::High => 512,
        }
    }

    /// Pick the slowest clock that can show `width` pixels.
    pub fn for_width(width: usize) -> Option<DotClock> {
        [DotClock::Low, DotClock::Mid, DotClock::High]
            .into_iter()
            .find(|c| width <= c.max_width())
    }
}

/// The VCE's palette RAM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vce {
    pub colors: [u16; VCE_ENTRIES],
}

impl Default for Vce {
    fn default() -> Self {
        Vce {
            colors: [0; VCE_ENTRIES],
        }
    }
}

impl Vce {
    /// Write one entry, masked to 9 bits like the hardware.
    pub fn set(&mut self, index: usize, color: u16) {
        self.colors[index & (VCE_ENTRIES - 1)] = color & 0x1ff;
    }

    /// Read a background palette entry.
    pub fn bg(&self, palette: u8, index: u8) -> u16 {
        self.colors[((palette as usize & 15) << 4) | (index as usize & 15)]
    }

    /// Read a sprite palette entry.
    pub fn sprite(&self, palette: u8, index: u8) -> u16 {
        self.colors[SPRITE_PALETTE_BASE + ((palette as usize & 15) << 4) + (index as usize & 15)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grb_packs_green_red_blue() {
        assert_eq!(grb(7, 0, 0), 0o070);
        assert_eq!(grb(0, 7, 0), 0o700);
        assert_eq!(grb(0, 0, 7), 0o007);
    }

    #[test]
    fn expand_hits_both_ends() {
        assert_eq!(expand3(0), 0);
        assert_eq!(expand3(7), 255);
        assert_eq!(to_rgb8(grb(7, 3, 1)), [255, 109, 36]);
    }

    #[test]
    fn rgb8_rounds_to_nearest_level() {
        assert_eq!(rgb8(255, 0, 0), grb(7, 0, 0));
        assert_eq!(rgb8(128, 128, 128), grb(4, 4, 4));
    }

    #[test]
    fn set_masks_to_nine_bits() {
        let mut v = Vce::default();
        v.set(3, 0xffff);
        assert_eq!(v.colors[3], 0x1ff);
    }

    #[test]
    fn dot_clock_for_width() {
        assert_eq!(DotClock::for_width(256), Some(DotClock::Low));
        assert_eq!(DotClock::for_width(320), Some(DotClock::Mid));
        assert_eq!(DotClock::for_width(512), Some(DotClock::High));
        assert_eq!(DotClock::for_width(600), None);
    }
}
