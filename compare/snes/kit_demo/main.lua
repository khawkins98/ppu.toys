-- kit_demo (SNES port of crates/tg16-core/toys/kit_demo.lua)
--
-- The TG16 toy fakes SNES features on one background layer with its `kit`
-- helpers. This port uses the real thing:
--   kit.parallax      -> BG2 mountains and BG1 hills, each with its own scroll
--   kit.sprite_layer  -> BG3 (2bpp) clouds behind both
--   kit.floor         -> a Mode 7 road: an HDMA hook switches lines 128-199
--                        to Mode 7 and re-aims the matrix on every line
--   kit.flicker_spr   -> a ghost sprite in an OBJ palette (4-7) that colour
--                        math adds onto the sub screen: steady, not flickering
--   kit.mask_lines    -> colour window 1 spanning the whole line, with
--                        CGWSEL's clip-to-black, on the letterbox lines
--   PSG music         -> the same loop on the S-DSP's built-in samples

local function c3(r, g, b) -- TG16 3-bit channels, for matching colours
  return rgb(floor(r * 255 / 7 + 0.5), floor(g * 255 / 7 + 0.5), floor(b * 255 / 7 + 0.5))
end
local function hex(s) -- "0123..." -> colour indices
  local t = {}
  for i = 1, string.len(s) do
    local b = string.byte(s, i)
    t[i] = b >= 97 and b - 87 or (b >= 48 and b - 48 or 0)
  end
  return t
end
local function rep(s, n)
  local out = ""
  for _ = 1, n do out = out .. s end
  return out
end
-- 8x8 tiles from 64 colour indices: 4bpp is 16 words (planes 0/1 then 2/3),
-- 2bpp is 8 words (planes 0/1).
local function tile(addr, px, bpp)
  for y = 0, 7 do
    local p = { 0, 0, 0, 0 }
    for x = 0, 7 do
      local c = px[y * 8 + x + 1]
      for k = 1, bpp do
        if c & (1 << (k - 1)) ~= 0 then p[k] = p[k] | (1 << (7 - x)) end
      end
    end
    vram[addr + y] = p[1] | (p[2] << 8)
    if bpp == 4 then vram[addr + 8 + y] = p[3] | (p[4] << 8) end
  end
end

-- VRAM: Mode 7 owns 0x0000-0x3fff (its fixed interleaved layout).
dma("road")
local CHR = 0x4000   -- BG1/BG2 tiles, 4bpp
local CHR3 = 0x5000  -- BG3 tiles, 2bpp
local MAP1 = 0x5800  -- BG1 hills, 64x32
local OBJ = 0x6000   -- sprite tiles
local MAP2 = 0x7000  -- BG2 mountains, 64x32
local MAP3 = 0x7800  -- BG3 clouds, 32x32
local SKY = c3(2, 4, 7)

-- Map entry: tile | palette << 10. A 64-wide map is two 32x32 screens.
local function put(map, tx, ty, t, pal)
  vram[map + (tx % 32) + (tx >= 32 and 0x400 or 0) + ty * 32] = t | (pal << 10)
end

function init()
  tile(CHR + 16, hex(rep("1", 64)), 4)                                -- mountain
  tile(CHR + 32, hex("00000001" .. "00000011" .. "00000111" .. "00001111" ..
                     "00011111" .. "00111111" .. "01111111" .. "11111111"), 4) -- slope /
  tile(CHR + 48, hex("10000000" .. "11000000" .. "11100000" .. "11110000" ..
                     "11111000" .. "11111100" .. "11111110" .. "11111111"), 4) -- slope \
  tile(CHR + 64, hex(rep("2", 64)), 4)                                -- hill

  for tx = 0, 63 do
    -- Mountains on BG2, map rows 6..11, peaks every 16 tiles.
    local h = 4 - abs((tx % 16) - 8) // 2
    for ty = 6, 11 do
      local top = 11 - h
      if ty > top then put(MAP2, tx, ty, 1, 1)
      elseif ty == top then put(MAP2, tx, ty, (tx % 16 < 8) and 2 or 3, 1) end
    end
    -- Hills on BG1, rows 12..15.
    local hh = 13 + floor(sin(tx * 0.7) * 1.2)
    for ty = hh, 15 do put(MAP1, tx, ty, 4, 1) end
  end

  -- Clouds on BG3: the TG16 toy's 32x16 cloud as 4x2 tiles, one every 64
  -- pixels on map rows 5-6 (screen y 40-55, where the sprite row was).
  for ty = 0, 1 do
    for tx = 0, 3 do
      local px = {}
      for i = 0, 63 do
        local x, y = tx * 8 + i % 8, ty * 8 + i // 8
        px[i + 1] = ((x - 16) ^ 2 / 2 + (y - 8) ^ 2 * 2 < 120) and 1 or 0
      end
      tile(CHR3 + (1 + ty * 4 + tx) * 8, px, 2)
    end
  end
  for c = 0, 31, 8 do
    for ty = 0, 1 do
      for tx = 0, 3 do vram[MAP3 + (5 + ty) * 32 + c + tx] = (1 + ty * 4 + tx) | (2 << 10) end
    end
  end

  -- Ghost: a 16x16 OBJ (tiles 0, 1, 16, 17 in the 16-wide OBJ grid).
  for ty = 0, 1 do
    for tx = 0, 1 do
      local px = {}
      for i = 0, 63 do
        local x, y = tx * 8 + i % 8 + 8, ty * 8 + i // 8
        px[i + 1] = ((x - 16) ^ 2 / 2 + (y - 8) ^ 2 * 2 < 60) and 1 or 0
      end
      tile(OBJ + (ty * 16 + tx) * 16, px, 4)
    end
  end

  -- CGRAM: Mode 7 colours sit at 1..3 (the m7 import), so the Mode 1
  -- layers use other palettes: BG1/BG2 palette 1, BG3 palette 2 (2bpp: 8-11),
  -- the ghost OBJ palette 4 (128 + 64), which colour math may touch.
  cgram[16 + 1] = c3(3, 3, 5)
  cgram[16 + 2] = c3(1, 4, 1)
  cgram[8 + 1] = c3(7, 7, 7)
  cgram[192 + 1] = rgb(72, 88, 104) -- added onto what is behind it
  dsp.mvol = { l = 64, r = 64 } -- about as loud as the TG16 version
end

-- Music: lead, bass and drums from the built-in bank.
local LEAD_I = bank("lead", { vol = 64, pan = -0.15 })
local BASS_I = bank("strings", { vol = 70, adsr = { a = 15, d = 3, s = 5, r = 20 } })
local KICK_I = bank("kick", { vol = 110 })
local HAT_I = bank("hat", { vol = 50, pan = 0.2 })
local LEAD = { "A4", "C5", "E5", "A5", "G5", "E5", "D5", "E5",
               "F4", "A4", "C5", "F5", "E5", "C5", "B4", "G4" }
local BASS = { "A2", "A2", "F2", "F2", "C3", "C3", "G2", "G2" }

local HORIZON, BOTTOM, HEIGHT = 128, 199, 24

function frame(t, f)
  local cam = t * 40
  mode = 1; brightness = 15
  screen.main.bg1 = true; screen.main.bg2 = true; screen.main.bg3 = true
  screen.main.obj = true
  screen.sub.bg1 = true; screen.sub.bg2 = true; screen.sub.bg3 = true
  bg[1].char_base = CHR; bg[1].map_base = MAP1; bg[1].screen_size = 1
  bg[2].char_base = CHR; bg[2].map_base = MAP2; bg[2].screen_size = 1
  bg[3].char_base = CHR3; bg[3].map_base = MAP3; bg[3].screen_size = 0
  obj.char_base = OBJ; obj.size_sel = 0 -- 8x8 / 16x16
  cgram[0] = SKY

  -- Parallax: each layer is its own scroll register.
  bg[1].scroll.x = floor(cam * 0.6); bg[1].scroll.y = -1
  bg[2].scroll.x = floor(cam * 0.25); bg[2].scroll.y = -1
  bg[3].scroll.x = floor(cam * 0.1); bg[3].scroll.y = -1

  -- Ghost: steady, added onto whatever the sub screen has behind it. The
  -- sub screen's backdrop is the fixed colour, so make that the sky too.
  local g = obj[0]
  g.x, g.y, g.tile, g.pal, g.prio = 120 + floor(sin(t * 2) * 30), 104, 0, 4, 3
  g.large, g.on = true, true
  color.on.obj = true
  color.addend = "sub"
  color.op = "add"
  color.half = false
  color.fixed = SKY

  -- Letterbox: colour window 1 is empty (lo > hi) except on the masked
  -- lines, where it spans the line and CGWSEL clips the picture to black.
  CGWSEL = 0x80             -- bits 6-7 = 2: clip to black inside the colour window
  win.color.w1 = true
  win.w1.lo, win.w1.hi = 1, 0
  local function mask()
    win.w1.lo, win.w1.hi = 0, 255
  end
  hdma(0, 23, mask)
  hdma(200, 223, mask)

  -- The road: Mode 7 below the horizon. Each line picks a texture row by
  -- depth (as kit.floor does with BYR) and a horizontal scale by depth (which
  -- the TG16 cannot do), then shifts sideways by curve * depth.
  local z, curve = t * 300, sin(t) * 2
  hdma(HORIZON, BOTTOM, function(y)
    local dy = y - HORIZON + 1
    local depth = HEIGHT * 32 / dy
    local u = floor(cam + curve * depth) % 1024
    mode = 7
    m7.a, m7.b, m7.c, m7.d = 72 / dy, 0, 0, 1
    m7.cx, m7.cy = u, 0
    m7.wrap = 0
    bg[1].scroll.x = u - 128           -- screen x 128 samples texel column u
    bg[1].scroll.y = floor(z + depth - y - 1) % 1024 -- line y samples row z + depth
  end)

  -- Music, step for step with the TG16 version (16ths at about 112 BPM).
  local step = f // 8
  if f % 8 == 0 then
    sfx(LEAD_I, 0, LEAD[step % 16 + 1])
    if step % 4 == 0 then sfx(KICK_I, 2) else sfx(HAT_I, 2) end
  elseif f % 8 == 6 then
    koff(0)
  end
  if f % 16 == 0 then sfx(BASS_I, 1, BASS[(step // 4) % 8 + 1])
  elseif f % 16 == 14 then koff(1) end
end
