// The scripted play-through of Fernwick that the TG16 test
// (crates/tg16-core/tests/fernwick.rs) and the SNES/parity test
// (crates/ppu-cli/tests/fernwick.rs) both include!: `(frames, buttons)`,
// buttons as SCRIPT_* flags that each test maps to its machine's pad bits.

pub const SCRIPT_UP: u16 = 1;
pub const SCRIPT_DOWN: u16 = 2;
pub const SCRIPT_LEFT: u16 = 4;
pub const SCRIPT_RIGHT: u16 = 8;
pub const SCRIPT_SWORD: u16 = 16;
pub const SCRIPT_START: u16 = 32;

pub const SCRIPT: &[(u32, u16)] = &[
    (20, 0),
    (1, SCRIPT_START),
    (10, 0),
    (230, SCRIPT_RIGHT), // east, through a scroll, into the crossroads (room 5)
    (10, 0),
    (32, SCRIPT_DOWN), // down to the blob's row
    (20, SCRIPT_RIGHT),
    (1, SCRIPT_SWORD), // swing: the blob dies
    (19, 0),
    (1, SCRIPT_SWORD), // a beetle charges in, hurts the hero, takes a hit
    (19, 0),
    (1, SCRIPT_SWORD),
    (19, 0),
];

/// Frame (counted from 0) in the middle of the scroll to room 5.
pub const SCRIPT_SCROLL_FRAME: u32 = 160;

/// Expected `quest_state()` after the script: mode (1 = play), room, hero
/// x, y, half hearts, gems, key, foes left, kills, door open, camera x, y,
/// attract mode.
pub const SCRIPT_END_STATE: [i64; 13] = [1, 5, 149, 160, 4, 0, 0, 2, 1, 0, 0, 0, 0];
