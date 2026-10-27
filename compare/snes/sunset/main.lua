-- sunset (SNES port of crates/tg16-core/toys/sunset.lua)
--
-- Same picture, written the SNES way:
--   * hills and ground are two real BG layers (BG1, BG2) that scroll at
--     their own speeds, instead of one layer split by raster interrupts;
--   * the ground still shears line by line, an HDMA table on BG2's scroll;
--   * the sky is an HDMA gradient into CGRAM 0 (the backdrop), smooth where
--     the TG16 version has eight bands;
--   * the middle ball sits behind the hills with OBJ priority 0;
--   * the music is the same tune on the S-DSP's built-in samples.
-- Graphics are generated here (4bpp planar tiles written to VRAM).

local function c3(r, g, b) -- TG16 3-bit channels, for matching colours
  return rgb(floor(r * 255 / 7 + 0.5), floor(g * 255 / 7 + 0.5), floor(b * 255 / 7 + 0.5))
end

-- One 8x8 4bpp tile from 64 colour indices (planes 0/1 in words 0-7, 2/3 in 8-15).
local function tile4(addr, px)
  for y = 0, 7 do
    local p0, p1, p2, p3 = 0, 0, 0, 0
    for x = 0, 7 do
      local c = px[y * 8 + x + 1]
      local bit = 1 << (7 - x)
      if c & 1 ~= 0 then p0 = p0 | bit end
      if c & 2 ~= 0 then p1 = p1 | bit end
      if c & 4 ~= 0 then p2 = p2 | bit end
      if c & 8 ~= 0 then p3 = p3 | bit end
    end
    vram[addr + y] = p0 | (p1 << 8)
    vram[addr + 8 + y] = p2 | (p3 << 8)
  end
end
-- "0123..." -> colour indices (this engine's string library has no rep or
-- tonumber, so these two are spelled out).
local function hex(s)
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

local CHR = 0x2000   -- BG1/BG2 tiles (4bpp)
local MAP1 = 0x0000  -- BG1 map, 64x32 (hills)
local MAP2 = 0x0800  -- BG2 map, 64x32 (ground)
local OBJ = 0x4000   -- sprite tiles

function init()
  -- Tiles 1-3 as in the TG16 toy; tile 0 stays blank (transparent).
  tile4(CHR + 16, hex("00011000" .. "00122100" .. "01211210" .. "12122121" ..
                      "21212212" .. "12121121" .. "21212212" .. "12121212"))
  tile4(CHR + 32, hex(rep("1212121221212121", 4)))
  tile4(CHR + 48, hex(rep("3", 32) .. rep("4", 32)))

  -- Map entries: tile | palette << 10. Hills on BG1, ground on BG2.
  for tx = 0, 63 do
    local top = 16 + floor(sin(tx * 0.45) * 2.5)
    local col = (tx % 32) + (tx >= 32 and 0x400 or 0) -- 64-wide = two 32x32 screens
    for ty = 0, 31 do
      local t = 0
      if ty >= 22 then
        vram[MAP2 + col + ty * 32] = 3 | (1 << 10)
      elseif ty > top then t = 2
      elseif ty == top then t = 1 end
      if t > 0 then vram[MAP1 + col + ty * 32] = t end
    end
  end
  cgram[1] = c3(1, 4, 1)
  cgram[2] = c3(1, 5, 2)
  cgram[16 + 3] = c3(4, 3, 1)
  cgram[16 + 4] = c3(3, 2, 1)

  -- A 32x32 ball. Large OBJ tiles sit in a 16-tile-wide grid: cell (cx, cy)
  -- of the 4x4 is tile cy * 16 + cx.
  for ty = 0, 3 do
    for tx = 0, 3 do
      local px = {}
      for i = 0, 63 do
        local x, y = tx * 8 + i % 8 - 16, ty * 8 + i // 8 - 16
        local d = x * x + y * y
        px[i + 1] = d < 120 and 2 or (d < 230 and 1 or 0)
      end
      tile4(OBJ + (ty * 16 + tx) * 16, px)
    end
  end
  for p = 0, 2 do
    cgram[128 + p * 16 + 1] = c3(7, 2 + p * 2, 1)
    cgram[128 + p * 16 + 2] = c3(7, 6, 3 + p * 2)
  end
  dsp.mvol = { l = 40, r = 40 } -- about as loud as the TG16 version
end

-- Sound: a pulse lead, a bass and a hi-hat from the built-in sample bank.
local LEAD_I = bank("lead", { vol = 70, pan = -0.2 })
local BASS_I = bank("bass", { vol = 110 })
local HAT_I = bank("hat", { vol = 50, pan = 0.25 })
local LEAD = { "E5", "G5", "B5", "G5", "D5", "G5", "A5", "F#5" }
local BASS = { "E2", "E2", "C2", "C2", "G2", "G2", "D2", "D2" }
local bass_note

-- The sky: the TG16 toy's eight band colours, blended per line.
local SKY = {}
for b = 0, 7 do
  SKY[#SKY + 1] = { b * 16 + 8, c3(floor(b / 2), floor(b / 2) + 1, 7 - floor(b / 3)) }
end

function frame(t, f)
  mode = 1; brightness = 15
  screen.main.bg1 = true; screen.main.bg2 = true; screen.main.obj = true
  bg[1].char_base = CHR; bg[1].map_base = MAP1; bg[1].screen_size = 1
  bg[2].char_base = CHR; bg[2].map_base = MAP2; bg[2].screen_size = 1
  obj.char_base = OBJ; obj.size_sel = 1 -- small 8x8, large 32x32

  -- 8 steps per bar at 120 BPM, as on the TG16.
  local step = f // 15
  if f % 15 == 0 then
    sfx(LEAD_I, 0, LEAD[step % 8 + 1])
    sfx(HAT_I, 2)
    local n = BASS[(step // 2) % 8 + 1]
    if n ~= bass_note then sfx(BASS_I, 1, n); bass_note = n end
  elseif f % 15 == 12 then
    koff(0)
  end

  local bob = floor(sin(t * 3) * 6)
  local balls = { { 40, 60 + bob, 0, 2, false }, { 120, 118, 1, 0, false }, { 200, 150, 2, 2, true } }
  for i, b in ipairs(balls) do
    local o = obj[i - 1]
    o.x, o.y, o.tile, o.pal, o.prio = b[1], b[2], 0, b[3], b[4]
    o.flip_y, o.large, o.on = b[5], true, true
  end

  -- Row 0 of each map at the top line (see ppu docs backgrounds).
  bg[1].scroll.y = -1; bg[2].scroll.y = -1
  bg[1].scroll.x = floor(t * 20)
  hdma(0, 175, function(y)
    cgram[0] = ramp_color(SKY, y)
  end)
  hdma(176, 223, function(y)
    bg[2].scroll.x = floor(t * 60) + (y - 176) * 2
  end)
end
