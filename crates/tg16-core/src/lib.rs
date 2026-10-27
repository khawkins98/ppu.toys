//! TurboGrafx-16 / PC Engine video core for ppu.toys.
//!
//! This crate models the HuC6270 VDC (VRAM, background map, tiles, sprites)
//! and the HuC6260 VCE (palette and dot clock), and renders frames from a
//! per-scanline register table. It shares no code with the SNES core in
//! `ppu-core`; see `docs/tg16/PLAN.md` for how the two fit together.

pub mod import;
pub mod render;
pub mod vce;
pub mod vdc;

pub use import::{import_bg, import_sprites, BgImport, Image, ImportError, SpriteImport};
pub use render::{render, Frame, LineRegs, LineStats, Output, MAX_HEIGHT, MAX_WIDTH};
pub use vce::{grb, rgb8, to_rgb8, DotClock, Vce, SPRITE_PALETTE_BASE, VCE_ENTRIES};
pub use vdc::{
    BatEntry, BatSize, Sprite, Vdc, SPRITE_COUNT, SPRITE_UNITS_PER_LINE, SPRITE_X_OFFSET,
    SPRITE_Y_OFFSET, VRAM_WORDS,
};

/// Default output size: 5.37 MHz dot clock, 224 visible lines.
pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 224;

mod lua;
pub use lua::{Engine, LuaError};

pub mod psg;
pub use psg::{Psg, SAMPLE_RATE};

pub mod budget;
pub use budget::{Budget, CYCLES_PER_FRAME};
