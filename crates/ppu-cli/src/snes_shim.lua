-- SNES compatibility layer for toys converted by `ppu tg16 convert`.
--
-- The SNES toy's own Lua runs unchanged on top of this file. It sees the
-- SNES globals (bg[1..4], obj[], cgram, screen, color, hdma, dma, ...) as
-- plain tables. After each frame() this layer reads them and drives the
-- TG16 the way a hand port would:
--
-- * The SNES background layers were squashed into one TG16 background at
--   convert time (__snes.planes says where each layer landed). Every line
--   follows the scroll of the layer that covers most of that line; the other
--   layers on that line move with it.
-- * CGRAM writes become VCE palette writes through __snes.pal (one SNES
--   colour can feed several TG16 entries). A write inside hdma() lasts one
--   line, as on the SNES, so the next line writes the old value back.
-- * OAM entries become spr() calls, using 16x16 patterns built at convert
--   time (__snes.sprites).
-- * Colour math, windows, mosaic, Mode 7 and sound are accepted and ignored.
--   The conversion report lists what that drops.
--
-- Names starting with __snes are internal.

local D = __snes
local T = {
  color = color, spr = spr, hdma = hdma, tile = tile, pattern = pattern,
  vram = vram, scroll = bg.scroll,
}
local floor = math.floor
local LINES = 224

------------------------------------------------------------------------
-- Helpers every SNES toy can use (same behaviour as the SNES engine).

tan = math.tan

local function clamp_u8(v)
  v = floor(v + 0.5)
  if v < 0 then return 0 elseif v > 255 then return 255 end
  return v
end

function rgb(r, g, b)
  return floor(clamp_u8(b or 0) / 8) * 1024 + floor(clamp_u8(g or 0) / 8) * 32 + floor(clamp_u8(r or 0) / 8)
end

function hsl(h, s, l)
  h = ((h % 360) + 360) % 360
  s = min(max(s, 0), 1)
  l = min(max(l, 0), 1)
  local c = (1 - abs(2 * l - 1)) * s
  local hp = h / 60
  local x = c * (1 - abs((hp % 2) - 1))
  local r1, g1, b1
  local k = floor(hp)
  if k == 0 then r1, g1, b1 = c, x, 0
  elseif k == 1 then r1, g1, b1 = x, c, 0
  elseif k == 2 then r1, g1, b1 = 0, c, x
  elseif k == 3 then r1, g1, b1 = 0, x, c
  elseif k == 4 then r1, g1, b1 = x, 0, c
  else r1, g1, b1 = c, 0, x end
  local m = l - c / 2
  return rgb((r1 + m) * 255, (g1 + m) * 255, (b1 + m) * 255)
end

function clamp(value, lo, hi)
  if lo > hi then error("clamp: minimum must not exceed maximum") end
  return max(lo, min(hi, value))
end

function lerp(a, b, t) return a + (b - a) * t end

function lerp_color(a, b, t)
  t = clamp(t, 0, 1)
  local result = 0
  for c = 0, 2 do
    local scale = 32 ^ c
    local av, bv = floor(a / scale) % 32, floor(b / scale) % 32
    result = result + floor(lerp(av, bv, t) + 0.5) * scale
  end
  return floor(result)
end

function ease(t, easing)
  if easing == "smooth" then return t * t * (3 - 2 * t) end
  if easing == "ease-in" then return t * t end
  if easing == "ease-out" then return t * (2 - t) end
  if easing == "step" then return t < 1 and 0 or 1 end
  return t
end

function ramp(kf, y)
  local n = #kf
  if n == 0 then error("ramp: at least one keyframe is required") end
  if y <= kf[1][1] then return kf[1][2] end
  for i = 2, n do
    local a, b = kf[i - 1], kf[i]
    if y <= b[1] then
      local d = b[1] - a[1]
      if d <= 0 then return b[2] end
      return lerp(a[2], b[2], ease((y - a[1]) / d, a[3]))
    end
  end
  return kf[n][2]
end

function ramp_int(kf, y) return floor(ramp(kf, y) + 0.5) end

function ramp_color(kf, y)
  if #kf == 0 then error("ramp_color: at least one keyframe is required") end
  local a, b = kf[1], kf[1]
  for i = 2, #kf do
    if y <= b[1] then break end
    a, b = b, kf[i]
  end
  local t = 0
  if b[1] > a[1] then t = clamp((y - a[1]) / (b[1] - a[1]), 0, 1) end
  t = ease(t, a[3])
  return lerp_color(a[2], b[2], t)
end

function vr(addr, words)
  for i = 1, #words do vram[addr + i - 1] = words[i] end
end

------------------------------------------------------------------------
-- Things a TG16 has no equivalent for: tables that accept anything.

local function sink()
  return setmetatable({}, {
    __index = function(t, k) local v = sink(); rawset(t, k, v); return v end,
  })
end
local function noop() end

score, kon, koff, timer, m7pixel, coldata, raster = noop, noop, noop, noop, noop, noop, noop
note = function(n) return n end
instrument = function(t) return t end
sfx, song, bank = noop, noop, noop
BANK, dsp, aram, sram = sink(), sink(), {}, {}
voice = {}
for i = 0, 7 do voice[i] = sink() end
pad = { up = false, down = false, left = false, right = false, a = false, b = false,
        x = false, y = false, l = false, r = false, start = false, select = false }

local function view(m, cx, cy, sy, visible)
  return { a = m[1], b = m[2], c = m[3], d = m[4], cx = cx, cy = cy,
           scroll_x = cx - 128, scroll_y = sy, visible = visible }
end
local function wrap1024(v) return floor(v + 0.5) % 1024 end
function mode7_transform(angle, zoom, x, z)
  local r = angle * math.pi / 180
  local c, s = cos(r), sin(r)
  local cx, cy = wrap1024(x), wrap1024(z)
  return view({ c / zoom, -s / zoom, s / zoom, c / zoom }, cx, cy, cy - 113, true)
end
function mode7_floor(y, horizon, height, angle, x, z)
  local r = angle * math.pi / 180
  local c, s = cos(r), sin(r)
  local scale = min(height / max(y - horizon, 1), 127)
  local depth = 128 * scale
  return view({ c * scale, 0, s * scale, 0 }, wrap1024(x - s * depth), wrap1024(z + c * depth), 0, y > horizon)
end

------------------------------------------------------------------------
-- SNES register tables, power-on values.

mode, bg3_priority, brightness, mosaic = 1, false, 15, 0
direct_color, force_blank = false, false
TM, TS = 0, 0
for _, k in ipairs({ "WH0", "WH1", "WH2", "WH3", "W12SEL", "W34SEL", "WOBJSEL", "WBGLOG",
                     "WOBJLOG", "TMW", "TSW", "CGWSEL", "CGADSUB", "COLDATA" }) do
  _G[k] = 0
end

bg = {}
for n = 1, 4 do
  bg[n] = { scroll = { x = 0, y = 0 }, visible = true, tile_size = 8, map_base = 0,
            screen_size = 0, char_base = 0, mosaic = false, map = {} }
end
m7 = { a = 1, b = 0, c = 0, d = 1, cx = 0, cy = 0, wrap = 0, flip_x = false,
       flip_y = false, extbg = false, map = {} }
obj = { char_base = 0, name_select = 0, size_sel = 0, first = 0,
        priority_rotate = false, oam_addr = 0 }
for i = 0, 127 do
  obj[i] = { x = 0, y = 0, tile = 0, pal = 0, prio = 0, flip_x = false, flip_y = false,
             on = false, large = false }
end
local function layers()
  return { bg1 = false, bg2 = false, bg3 = false, bg4 = false, obj = false }
end
screen = { main = layers(), sub = layers() }
color = { on = { bg1 = false, bg2 = false, bg3 = false, bg4 = false, obj = false, backdrop = false },
          op = "add", half = false, fixed = 0, addend = "fixed", region = "everywhere",
          clip = "never" }
win = { w1 = { lo = 0, hi = 0 }, w2 = { lo = 0, hi = 0 } }
for _, k in ipairs({ "bg1", "bg2", "bg3", "bg4", "obj", "color" }) do
  win[k] = { w1 = false, w2 = false, w1_invert = false, w2_invert = false, invert = false,
             combine = "OR", main = false, sub = false }
end

-- VRAM: accepted and readable, but the background and sprite graphics were
-- fixed at convert time, so later writes do not change the picture.
local VR = {}
vram = setmetatable({}, {
  __index = function(_, a) return VR[a] end,
  __newindex = function(_, a, v) VR[a] = v end,
})

-- CGRAM. Frame-wide writes persist; writes inside hdma() last one line.
local CG = {}
for i = 0, 255 do CG[i] = D.cgram[i + 1] or 0 end
local in_line = false
local LINE_CG = nil
cgram = setmetatable({}, {
  __index = function(_, i)
    if in_line and LINE_CG[i] ~= nil then return LINE_CG[i] end
    return CG[i]
  end,
  __newindex = function(_, i, v)
    i = floor(i) % 256
    v = floor(v) % 32768
    if in_line then LINE_CG[i] = v else CG[i] = v end
  end,
})

-- Scanline hooks, collected each frame and run after frame() returns.
local SH = {}
function hdma(y0, y1, fn)
  if type(fn) ~= "function" then error("hdma(y0, y1, fn): fn must be a function", 2) end
  SH[#SH + 1] = { y0, y1, fn }
end
scanline = hdma

-- dma(): replays what the SNES engine returned for the same call when the
-- toy was converted. Its graphics are already in TG16 VRAM.
local dma_n = 0
local function copy(v)
  if type(v) ~= "table" then return v end
  local out = {}
  for k, x in pairs(v) do out[k] = copy(x) end
  return out
end
function dma(name, opts)
  dma_n = dma_n + 1
  local rec = D.dma[dma_n]
  if rec == nil or rec.name ~= name then
    error("dma(\"" .. tostring(name) .. "\"): this toy now calls dma() differently than when it was converted; convert it again", 2)
  end
  return copy(rec.ret)
end

------------------------------------------------------------------------
-- Colour conversion: BGR555 -> 9-bit GRB, through SNES brightness.

local conv_cache = {}
local function conv(c, b)
  local key = c * 16 + b
  local v = conv_cache[key]
  if v then return v end
  local function ch(v5)
    local c8 = v5 * 8 + floor(v5 / 4)
    c8 = floor(c8 * b / 15)
    return floor((c8 * 7 + 127) / 255)
  end
  local r, g, bl = ch(c % 32), ch(floor(c / 32) % 32), ch(floor(c / 1024) % 32)
  v = g * 64 + r * 8 + bl
  conv_cache[key] = v
  return v
end

-- SNES CGRAM index -> list of TG16 VCE entries.
local PAL = D.pal

------------------------------------------------------------------------
-- Per-line state.

local BPP = {
  [0] = { 2, 2, 2, 2 }, [1] = { 4, 4, 2, 0 }, [2] = { 4, 4, 0, 0 }, [3] = { 8, 4, 0, 0 },
  [4] = { 8, 2, 0, 0 }, [5] = { 0, 0, 0, 0 }, [6] = { 0, 0, 0, 0 }, [7] = { 0, 0, 0, 0 },
}
local BIT = { 1, 2, 4, 8, 16 }
local function bit_on(v, i) return floor((floor(v) % 256) / BIT[i]) % 2 == 1 end

local function plane_key(n, l, m)
  local bpp = (BPP[m] or BPP[1])[n]
  if bpp == 0 then return nil end
  local map_base = floor(floor(l.map_base) / 1024) % 32 * 1024
  local char_base = floor(floor(l.char_base) / 4096) % 8 * 4096
  local ts = (l.tile_size or 8) >= 16 and 16 or 8
  return n .. ":" .. map_base .. ":" .. char_base .. ":" .. floor(l.screen_size) % 4 .. ":" .. ts .. ":" .. bpp
end

local function rnd(v) return floor(v + 0.5) end

-- Which layer drives this line, and the TG16 scroll that shows it.
-- Returns bx, row (absolute BAT pixel row) or nil when no layer is shown.
local W, H = D.w, D.h
local prev_driver = nil
local function line_scroll(y, m, blank)
  if blank then return nil end
  local best, best_cov, best_rank, bx, row = nil, -1, 99, 0, 0
  local main = screen.main
  for n = 1, 4 do
    local l = bg[n]
    if (main["bg" .. n] or bit_on(TM, n)) and l.visible ~= false then
      local key = plane_key(n, l, m)
      local p = key and D.planes[key]
      if p then
        local sx, sy = rnd(l.scroll.x), rnd(l.scroll.y)
        local r = (y + 1 + sy) % p.h
        local cov = p.cov[r + 1] or 0
        -- Keep the previous line's driver while it still covers a fair share,
        -- so the scroll does not flip back and forth (each flip costs a
        -- raster interrupt).
        if p == prev_driver and cov > 0 then cov = cov * 1.5 end
        if cov > best_cov or (cov == best_cov and p.rank < best_rank) then
          best, best_cov, best_rank = p, cov, p.rank
          bx = (sx - p.dx) % W
          row = (y + 1 + sy - p.dy) % H
        end
      end
    end
  end
  prev_driver = best
  if best == nil then return nil end
  return bx, row
end

-- Snapshot/restore of the fields hdma() hooks may change, so every line
-- starts from the frame-wide values (SNES semantics).
-- Only fields the lowering reads (plus colour-math basics) are put back;
-- other fields a hook writes stay written for the lines after it.
local GLOBALS = { "mode", "brightness", "force_blank", "TM" }
local BGKEYS = { "visible", "map_base", "char_base", "screen_size", "tile_size" }
local ST, SK, SV, SN = {}, {}, {}, 0
local function keep(t, k) SN = SN + 1; ST[SN] = t; SK[SN] = k end
local function snapshot()
  SN = 0
  for i = 1, #GLOBALS do keep(_G, GLOBALS[i]) end
  for n = 1, 4 do
    local l = bg[n]
    keep(l.scroll, "x"); keep(l.scroll, "y")
    for i = 1, #BGKEYS do keep(l, BGKEYS[i]) end
  end
  for _, k in ipairs({ "bg1", "bg2", "bg3", "bg4", "obj" }) do keep(screen.main, k) end
  for _, k in ipairs({ "op", "half", "fixed", "addend" }) do keep(color, k) end
  for i = 1, SN do SV[i] = ST[i][SK[i]] end
end
local function restore()
  for i = 1, SN do
    local t, k, v = ST[i], SK[i], SV[i]
    if t[k] ~= v then t[k] = v end
  end
end

-- One line's lowered state from the current tables.
local function lower_line(y)
  local blank = force_blank == true
  local bx, row = line_scroll(y, floor(mode) % 8, blank)
  return {
    bx = bx, row = row,
    bg = bx ~= nil,
    spr = (not blank) and (screen.main.obj or bit_on(TM, 5)) and true or false,
  }
end

------------------------------------------------------------------------
-- Sprites: OAM -> SAT, every frame.

local SPR = D.sprites
local SIZES = {
  [0] = { { 8, 8 }, { 16, 16 } }, { { 8, 8 }, { 32, 32 } }, { { 8, 8 }, { 64, 64 } },
  { { 16, 16 }, { 32, 32 } }, { { 16, 16 }, { 64, 64 } }, { { 32, 32 }, { 64, 64 } },
  { { 16, 32 }, { 32, 64 } }, { { 16, 32 }, { 32, 32 } },
}
__snes_stats = { sprites_dropped = 0, sprites_missing = 0 }

local function lower_sprites()
  local cb = floor(floor(obj.char_base) / 8192) % 4 * 8192
  local ns = floor(obj.name_select or 0) % 4
  local ss = floor(obj.size_sel or 0) % 8
  local slot = 0
  for i = 0, 127 do
    local o = obj[i]
    if o.on then
      local large = o.large and 1 or 0
      local size = SIZES[ss][large + 1]
      local w, h = size[1], size[2]
      local x = floor(o.x) % 512
      if x >= 256 then x = x - 512 end
      local y = floor(o.y) % 256
      if y < 224 and x < 256 and x + w > 0 then
        local parts = SPR[cb .. ":" .. ns .. ":" .. ss .. ":" .. large .. ":" .. floor(o.tile) % 512]
        if parts == nil then
          __snes_stats.sprites_missing = __snes_stats.sprites_missing + 1
        else
          local fx, fy = o.flip_x and true or false, o.flip_y and true or false
          for k = 1, #parts do
            local p = parts[k]
            if slot > 63 then
              __snes_stats.sprites_dropped = __snes_stats.sprites_dropped + 1
            else
              local px = fx and (x + w - p[4] - p[2]) or (x + p[4])
              local py = fy and (y + h - p[5] - p[3]) or (y + p[5])
              T.spr(slot, px, py, p[1], { w = p[2], h = p[3], pal = floor(o.pal) % 8,
                                           front = o.prio >= 1, flip_x = fx, flip_y = fy })
              slot = slot + 1
            end
          end
        end
      end
    end
  end
end

------------------------------------------------------------------------
-- Frame driver.

local VCE = {}  -- what the frame-wide VCE holds (persists across frames)

local function vce_values(cg, b, out)
  for i, entries in pairs(PAL) do
    local v = conv(cg[i] or 0, b)
    for k = 1, #entries do out[entries[k]] = v end
  end
end

function __snes_init(user_init)
  bat.w, bat.h = W / 8, H / 8
  for i = 1, #D.tiles do T.tile(D.first_tile + i - 1, D.tiles[i]) end
  for i = 1, #D.patterns do T.pattern(D.first_pattern + i - 1, D.patterns[i]) end
  local words = D.bat
  for i = 1, #words do T.vram(i - 1, words[i]) end
  if user_init then user_init() end
end

function __snes_frame(user_frame, t, f)
  SH = {}
  if user_frame then user_frame(t, f) end

  local frame_b = force_blank == true and 0 or clamp(floor(brightness), 0, 15)
  snapshot()
  local nh = #SH
  prev_driver = nil
  local frame_line = nil
  local LS = {}
  for y = 0, LINES - 1 do
    local any = false
    for i = 1, nh do
      local hk = SH[i]
      if y >= hk[1] and y <= hk[2] then any = true; break end
    end
    if any then
      in_line, LINE_CG = true, {}
      for i = 1, nh do
        local hk = SH[i]
        if y >= hk[1] and y <= hk[2] then hk[3](y) end
      end
      in_line = false
      local s = lower_line(y)
      s.cg = LINE_CG
      LS[y] = s
      restore()
    else
      local s = lower_line(y)
      LS[y] = s
    end
  end
  LINE_CG = nil

  lower_sprites()

  -- Frame-wide palette: the frame's CGRAM plus line 0's own writes.
  local want = {}
  local cg0 = CG
  if LS[0].cg and next(LS[0].cg) then
    cg0 = setmetatable({}, { __index = CG })
    for i, v in pairs(LS[0].cg) do cg0[i] = v end
  end
  vce_values(cg0, frame_b, want)
  for e, v in pairs(want) do
    if VCE[e] ~= v then T.color(e, v); VCE[e] = v end
  end

  -- Frame-wide scroll and enables come from line 0.
  local s0 = LS[0]
  local fbx, frow = s0.bx or 0, s0.row or 0
  T.scroll.x, T.scroll.y = fbx, frow
  show.bg, show.sprites = s0.bg, s0.spr

  -- Lines that differ from what the TG16 would show anyway get a raster hook.
  local cur = {}          -- palette entries overridden on the previous line
  local counter = frow
  local todo = {}
  for y = 1, LINES - 1 do
    local s = LS[y]
    counter = (counter + 1) % H
    local writes = nil
    -- this line's palette overrides
    local over = {}
    if s.cg then
      for i, v in pairs(s.cg) do
        local entries = PAL[i]
        if entries then
          local c = conv(v, frame_b)
          for k = 1, #entries do over[entries[k]] = c end
        end
      end
    end
    for e, c in pairs(over) do
      if (cur[e] or want[e]) ~= c then writes = writes or {}; writes[#writes + 1] = { e, c } end
    end
    for e, c in pairs(cur) do
      if over[e] == nil and c ~= want[e] then
        writes = writes or {}; writes[#writes + 1] = { e, want[e] or 0 }
      end
    end
    cur = over
    local need = writes ~= nil or s.bg ~= s0.bg or s.spr ~= s0.spr
    local bx, row = s.bx, s.row
    if bx ~= nil then
      if bx ~= fbx or row ~= counter then need = true end
      counter = row
    end
    if need then todo[y] = { bx = bx or fbx, row = row, bg = s.bg, spr = s.spr, writes = writes } end
  end

  local y = 1
  while y < LINES do
    if todo[y] then
      local y0 = y
      while y + 1 < LINES and todo[y + 1] do y = y + 1 end
      T.hdma(y0, y, function(ly)
        local s = todo[ly]
        T.scroll.x = s.bx
        if s.row then BYR = s.row end
        show.bg, show.sprites = s.bg, s.spr
        local w = s.writes
        if w then for k = 1, #w do T.color(w[k][1], w[k][2]) end end
      end)
    end
    y = y + 1
  end
end
