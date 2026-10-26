//! Lua front end: runs a toy's `init()` and `frame(t, f)` against the TG16
//! core, collects its `hdma()` hooks, and produces a [`Frame`] to render.
//!
//! The Lua surface is small on purpose: VRAM, palette and sprite writes go
//! straight to native callbacks, and the friendly register names live in
//! `prelude.lua`.

use std::cell::RefCell;
use std::rc::Rc;

use piccolo::{
    Callback, CallbackReturn, Closure, Executor, Function, Lua, PrototypeError, StashedFunction,
    StaticError, Table, Value,
};

use crate::render::{render, Frame, LineRegs, Output, MAX_HEIGHT, MAX_WIDTH};
use crate::vce::{grb, rgb8, Vce};
use crate::vdc::{BatEntry, BatSize, Sprite, Vdc, SPRITE_COUNT, VRAM_WORDS};

const PRELUDE: &str = include_str!("prelude.lua");

/// A Lua compile or runtime error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaError {
    pub message: String,
    pub line: Option<u32>,
    pub file: Option<String>,
}

impl std::fmt::Display for LuaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.file, self.line) {
            (Some(file), Some(line)) => write!(f, "{file}:{line}: {}", self.message),
            (Some(file), None) => write!(f, "{file}: {}", self.message),
            _ => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for LuaError {}

fn static_error(e: StaticError, file: Option<&str>) -> LuaError {
    let line = if let StaticError::Runtime(rt) = &e {
        rt.downcast::<PrototypeError>().and_then(|pe| match pe {
            PrototypeError::Parser(p) => Some(p.line_number.0 as u32 + 1),
            _ => None,
        })
    } else {
        None
    };
    LuaError {
        message: e.to_string(),
        line,
        file: file.map(str::to_string),
    }
}

/// Machine state the native callbacks write into.
struct State {
    vdc: Vdc,
    /// Palette at the top of each frame. Writes from `init()`, top-level
    /// code and `frame()` land here and persist.
    vce: Vce,
    /// Set while a hook runs: palette writes go to this line instead.
    line: Option<usize>,
    line_writes: Vec<Vec<(u16, u16)>>,
    /// What last wrote each VRAM word, to catch tiles, sprite patterns and
    /// the BAT stepping on each other (they share one 64 KB VRAM).
    owner: Vec<Owner>,
    warnings: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owner {
    None,
    Map,
    Tile(u16),
    Pattern(u16),
}

impl Owner {
    fn describe(self) -> String {
        match self {
            Owner::None => "nothing".into(),
            Owner::Map => "the BAT (map)".into(),
            Owner::Tile(n) => format!("tile {n}"),
            Owner::Pattern(n) => format!("sprite pattern {n}"),
        }
    }
}

impl State {
    /// Record that `who` wrote `words`, warning once per pair of owners when
    /// it overwrites something else.
    fn claim(&mut self, words: std::ops::Range<usize>, who: Owner) {
        const MAX_WARNINGS: usize = 20;
        for w in words {
            let w = w & (VRAM_WORDS - 1);
            let prev = self.owner[w];
            self.owner[w] = who;
            let clash = match (prev, who) {
                (Owner::None, _) => false,
                (Owner::Map, Owner::Map) => false,
                (a, b) => a != b,
            };
            if clash && self.warnings.len() < MAX_WARNINGS {
                let msg = format!(
                    "VRAM word {w:#06x}: {} overwrites {}. Tiles start at word tile*16, \
                     patterns at pattern*64, and the BAT fills words 0..bat.w*bat.h.",
                    who.describe(),
                    prev.describe()
                );
                if !self.warnings.contains(&msg) {
                    self.warnings.push(msg);
                }
                // One warning per write is plenty.
                return;
            }
        }
    }
}

/// A loaded toy.
pub struct Engine {
    lua: Lua,
    state: Rc<RefCell<State>>,
    init_fn: Option<StashedFunction>,
    frame_fn: Option<StashedFunction>,
    frame_file: Option<String>,
}

fn int(v: Value<'_>) -> Option<i64> {
    match v {
        Value::Integer(i) => Some(i),
        Value::Number(f) if f.is_finite() => Some(f.floor() as i64),
        _ => None,
    }
}

fn truthy(v: Value<'_>) -> bool {
    !matches!(v, Value::Nil | Value::Boolean(false))
}

fn err<'gc>(ctx: piccolo::Context<'gc>, msg: &str) -> piccolo::Error<'gc> {
    Value::String(ctx.intern(msg.as_bytes())).into()
}

/// Read `n` color indices from a hex string ("0123...") or a 1-based table.
fn pixels<'gc>(ctx: piccolo::Context<'gc>, v: Value<'gc>, n: usize) -> Result<Vec<u8>, String> {
    match v {
        Value::String(s) => {
            let digits: Vec<u8> = s
                .as_bytes()
                .iter()
                .filter(|b| !b.is_ascii_whitespace())
                .map(|&b| match b {
                    b'0'..=b'9' => Ok(b - b'0'),
                    b'a'..=b'f' => Ok(b - b'a' + 10),
                    b'A'..=b'F' => Ok(b - b'A' + 10),
                    b'.' => Ok(0),
                    _ => Err(format!("bad pixel character '{}'", b as char)),
                })
                .collect::<Result<_, _>>()?;
            if digits.len() != n {
                return Err(format!("expected {n} pixels, got {}", digits.len()));
            }
            Ok(digits)
        }
        Value::Table(t) => (1..=n as i64)
            .map(|i| int(t.get(ctx, i)).map(|c| (c & 15) as u8).ok_or(()))
            .collect::<Result<_, _>>()
            .map_err(|_| format!("expected a table of {n} numbers")),
        _ => Err("pixels must be a hex string or a table".into()),
    }
}

fn bat_size(ctx: piccolo::Context<'_>) -> BatSize {
    if let Value::Table(b) = ctx.get_global("bat") {
        let w = int(b.get(ctx, "w")).unwrap_or(32) as u16;
        let h = int(b.get(ctx, "h")).unwrap_or(32) as u16;
        BatSize::new(w, h).unwrap_or_default()
    } else {
        BatSize::default()
    }
}

fn install<'gc>(ctx: piccolo::Context<'gc>, state: &Rc<RefCell<State>>) {
    let set = |name: &'static str, f: Callback<'gc>| {
        ctx.set_global(name, f).unwrap();
    };
    // piccolo's base library has no _G; the prelude needs it.
    ctx.set_global("_G", ctx.globals()).unwrap();

    // vram(addr, word): raw VRAM write. vram(addr) reads.
    let st = state.clone();
    set(
        "vram",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let addr = int(stack.get(0)).ok_or_else(|| err(ctx, "vram(addr, word)"))? as usize;
            let addr = addr & (VRAM_WORDS - 1);
            match int(stack.get(1)) {
                Some(w) => {
                    st.borrow_mut().vdc.vram[addr] = w as u16;
                    stack.clear();
                }
                None => {
                    let w = st.borrow().vdc.vram[addr];
                    stack.replace(ctx, w as i64);
                }
            }
            Ok(CallbackReturn::Return)
        }),
    );

    // tile(n, pixels): write an 8x8 4bpp tile.
    let st = state.clone();
    set(
        "tile",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let n = int(stack.get(0)).ok_or_else(|| err(ctx, "tile(n, pixels)"))?;
            let px = pixels(ctx, stack.get(1), 64).map_err(|m| err(ctx, &format!("tile: {m}")))?;
            let mut s = st.borrow_mut();
            let base = (n as usize & 0x7ff) * 16;
            s.claim(base..base + 16, Owner::Tile(n as u16 & 0x7ff));
            s.vdc.write_tile(n as u16, &px.try_into().unwrap());
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // pattern(n, pixels): write a 16x16 sprite pattern.
    let st = state.clone();
    set(
        "pattern",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let n = int(stack.get(0)).ok_or_else(|| err(ctx, "pattern(n, pixels)"))?;
            let px =
                pixels(ctx, stack.get(1), 256).map_err(|m| err(ctx, &format!("pattern: {m}")))?;
            let mut s = st.borrow_mut();
            let base = (n as usize & 0x3ff) * 64;
            s.claim(base..base + 64, Owner::Pattern(n as u16 & 0x3ff));
            s.vdc.write_pattern(n as u16, &px.try_into().unwrap());
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // map(tx, ty, tile, pal): write one BAT cell, using bat.w / bat.h.
    let st = state.clone();
    set(
        "map",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let a: Vec<_> = (0..4).map(|i| int(stack.get(i))).collect();
            let [Some(tx), Some(ty), Some(tile)] = [a[0], a[1], a[2]] else {
                return Err(err(ctx, "map(tx, ty, tile, pal)"));
            };
            let e = BatEntry {
                tile: tile as u16,
                palette: a[3].unwrap_or(0) as u8,
            };
            let size = bat_size(ctx);
            let (tx, ty) = (
                tx.rem_euclid(size.width as i64) as usize,
                ty.rem_euclid(size.height as i64) as usize,
            );
            let mut s = st.borrow_mut();
            let w = ty * size.width as usize + tx;
            s.claim(w..w + 1, Owner::Map);
            s.vdc.set_bat(size, tx as u16, ty as u16, e);
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // color(entry, value): VCE write. Inside hdma() it applies from that line
    // to the end of the frame; elsewhere it persists.
    let st = state.clone();
    set(
        "color",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let (Some(i), Some(c)) = (int(stack.get(0)), int(stack.get(1))) else {
                return Err(err(ctx, "color(entry, value)"));
            };
            let mut s = st.borrow_mut();
            match s.line {
                Some(y) => s.line_writes[y].push(((i & 511) as u16, (c & 0x1ff) as u16)),
                None => s.vce.set(i as usize, c as u16),
            }
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // spr(i, x, y, pattern, {pal, w, h, front, flip_x, flip_y})
    let st = state.clone();
    set(
        "spr",
        Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let usage = "spr(i, x, y, pattern, opts)";
            let i = int(stack.get(0)).ok_or_else(|| err(ctx, usage))?;
            if !(0..SPRITE_COUNT as i64).contains(&i) {
                return Err(err(ctx, "spr: index must be 0..63"));
            }
            let x = int(stack.get(1)).ok_or_else(|| err(ctx, usage))?;
            let y = int(stack.get(2)).ok_or_else(|| err(ctx, usage))?;
            let pattern = int(stack.get(3)).ok_or_else(|| err(ctx, usage))?;
            let mut s = Sprite {
                x: x as i32,
                y: y as i32,
                pattern: pattern as u16,
                width: 16,
                height: 16,
                front: true,
                ..Sprite::default()
            };
            if let Value::Table(o) = stack.get(4) {
                s.palette = int(o.get(ctx, "pal")).unwrap_or(0) as u8 & 15;
                s.width = match int(o.get(ctx, "w")).unwrap_or(16) {
                    16 => 16,
                    32 => 32,
                    _ => return Err(err(ctx, "spr: w must be 16 or 32")),
                };
                s.height = match int(o.get(ctx, "h")).unwrap_or(16) {
                    16 => 16,
                    32 => 32,
                    64 => 64,
                    _ => return Err(err(ctx, "spr: h must be 16, 32 or 64")),
                };
                if !matches!(o.get(ctx, "front"), Value::Nil) {
                    s.front = truthy(o.get(ctx, "front"));
                }
                s.flip_x = truthy(o.get(ctx, "flip_x"));
                s.flip_y = truthy(o.get(ctx, "flip_y"));
            }
            st.borrow_mut().vdc.set_sprite(i as usize, s);
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );

    // rgb(r, g, b) with 0..255 channels; grb3(r, g, b) with 0..7 channels.
    set(
        "rgb",
        Callback::from_fn(&ctx, |ctx, _, mut stack| {
            let ch = |v: Value<'_>| int(v).unwrap_or(0).clamp(0, 255) as u8;
            let c = rgb8(ch(stack.get(0)), ch(stack.get(1)), ch(stack.get(2)));
            stack.replace(ctx, c as i64);
            Ok(CallbackReturn::Return)
        }),
    );
    set(
        "rgb3",
        Callback::from_fn(&ctx, |ctx, _, mut stack| {
            let ch = |v: Value<'_>| int(v).unwrap_or(0).clamp(0, 7) as u8;
            let c = grb(ch(stack.get(0)), ch(stack.get(1)), ch(stack.get(2)));
            stack.replace(ctx, c as i64);
            Ok(CallbackReturn::Return)
        }),
    );

    // __line(y): the resolver marks which line its hooks are running on.
    let st = state.clone();
    set(
        "__line",
        Callback::from_fn(&ctx, move |_, _, mut stack| {
            let y = int(stack.get(0)).unwrap_or(-1);
            let mut s = st.borrow_mut();
            s.line = (y >= 0 && (y as usize) < s.line_writes.len()).then_some(y as usize);
            stack.clear();
            Ok(CallbackReturn::Return)
        }),
    );
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// A fresh machine with nothing loaded.
    pub fn new() -> Engine {
        let state = Rc::new(RefCell::new(State {
            vdc: Vdc::default(),
            vce: Vce::default(),
            line: None,
            line_writes: vec![Vec::new(); MAX_HEIGHT],
            owner: vec![Owner::None; VRAM_WORDS],
            warnings: Vec::new(),
        }));
        Engine {
            lua: Lua::core(),
            state,
            init_fn: None,
            frame_fn: None,
            frame_file: None,
        }
    }

    pub fn vdc(&self) -> std::cell::Ref<'_, Vdc> {
        std::cell::Ref::map(self.state.borrow(), |s| &s.vdc)
    }

    pub fn vce(&self) -> std::cell::Ref<'_, Vce> {
        std::cell::Ref::map(self.state.borrow(), |s| &s.vce)
    }

    /// Problems worth fixing that are not errors, such as tile data that
    /// overlaps the BAT. Collected since `load()`.
    pub fn warnings(&self) -> Vec<String> {
        self.state.borrow().warnings.clone()
    }

    fn run_chunk(&mut self, name: &str, src: &str) -> Result<(), LuaError> {
        let ex = self
            .lua
            .try_enter(|ctx| {
                let closure = Closure::load(ctx, Some(name), src.as_bytes())?;
                Ok(ctx.stash(Executor::start(ctx, closure.into(), ())))
            })
            .map_err(|e| static_error(e, Some(name)))?;
        self.lua
            .execute::<()>(&ex)
            .map_err(|e| static_error(e, Some(name)))
    }

    fn call(
        &mut self,
        f: &StashedFunction,
        args: (f64, i64),
        file: Option<&str>,
    ) -> Result<(), LuaError> {
        let ex = self.lua.enter(|ctx| {
            let func = ctx.fetch(f);
            ctx.stash(Executor::start(ctx, func, args))
        });
        self.lua
            .execute::<()>(&ex)
            .map_err(|e| static_error(e, file))
    }

    /// Load a toy from `(file name, source)` chunks, run in order into one
    /// global environment, then run `init()` once if it exists. Resets the
    /// machine first.
    pub fn load(&mut self, files: &[(&str, &str)]) -> Result<(), LuaError> {
        *self = Engine::new();
        let state = self.state.clone();
        self.lua.enter(|ctx| install(ctx, &state));
        self.run_chunk("prelude", PRELUDE)?;
        for (name, src) in files {
            self.run_chunk(name, src)?;
        }
        let find = |lua: &mut Lua, name: &'static str| {
            lua.enter(|ctx| match ctx.get_global(name) {
                Value::Function(f) => Some(ctx.stash(f)),
                _ => None,
            })
        };
        self.init_fn = find(&mut self.lua, "init");
        self.frame_fn = find(&mut self.lua, "frame");
        self.frame_file = self.lua.enter(|ctx| match ctx.get_global("frame") {
            Value::Function(Function::Closure(c)) => {
                Some(c.prototype().chunk_name.to_str_lossy().into_owned())
            }
            _ => None,
        });
        if let Some(init) = self.init_fn.clone() {
            self.call(&init, (0.0, 0), files.last().map(|f| f.0))?;
        }
        Ok(())
    }

    /// Run `frame(t, f)` and the scanline hooks, and return the register
    /// table for the renderer. Sprites are cleared before `frame()` runs, so
    /// it places every sprite it wants on screen.
    pub fn frame(&mut self, t: f64, f: u32) -> Result<Frame, LuaError> {
        {
            let mut s = self.state.borrow_mut();
            s.vdc.sat = [0; SPRITE_COUNT * 4];
            s.line_writes.iter_mut().for_each(Vec::clear);
            s.line = None;
        }
        self.lua.enter(|ctx| {
            ctx.set_global("__hooks", Table::new(&ctx)).unwrap();
        });
        if let Some(func) = self.frame_fn.clone() {
            let file = self.frame_file.clone();
            self.call(&func, (t, f as i64), file.as_deref())?;
        }

        let (width, height, bat) = self.lua.enter(|ctx| {
            let (mut w, mut h) = (256i64, 224i64);
            if let Value::Table(d) = ctx.get_global("display") {
                w = int(d.get(ctx, "width")).unwrap_or(256);
                h = int(d.get(ctx, "height")).unwrap_or(224);
            }
            (
                (w.clamp(8, MAX_WIDTH as i64) as usize) / 8 * 8,
                h.clamp(1, MAX_HEIGHT as i64) as usize,
                bat_size(ctx),
            )
        });

        let ex = self.lua.enter(|ctx| {
            let Value::Function(resolve) = ctx.get_global("__resolve") else {
                unreachable!("prelude defines __resolve")
            };
            ctx.stash(Executor::start(ctx, resolve, height as i64))
        });
        let file = self.frame_file.clone();
        self.lua
            .execute::<()>(&ex)
            .map_err(|e| static_error(e, file.as_deref()))?;

        let mut frame = Frame::new(width, height, LineRegs::default());
        frame.bat = bat;
        self.lua.enter(|ctx| {
            frame.scroll_y = int(ctx.get_global("__frame_y")).unwrap_or(0) as u16;
            if let Value::Table(t) = ctx.get_global("__lines") {
                for (y, line) in frame.lines.iter_mut().enumerate() {
                    let at = |k: usize| int(t.get(ctx, (y * 4 + k + 1) as i64)).unwrap_or(0);
                    line.scroll_x = at(0) as u16;
                    let reload = at(1);
                    line.scroll_y_write = (reload >= 0).then_some(reload as u16);
                    line.bg_on = at(2) != 0;
                    line.sprites_on = at(3) != 0;
                }
            }
        });
        let s = self.state.borrow();
        for (line, writes) in frame.lines.iter_mut().zip(&s.line_writes) {
            line.palette_writes.clone_from(writes);
        }
        Ok(frame)
    }

    /// `frame()` then render.
    pub fn render(&mut self, t: f64, f: u32) -> Result<Output, LuaError> {
        let frame = self.frame(t, f)?;
        let s = self.state.borrow();
        Ok(render(&s.vdc, &s.vce, &frame))
    }
}
