-- patchwork (SNES side of crates/tg16-core/toys/patchwork)
--
-- The same quilt and balloons (10_patchwork.lua is an identical copy). CGRAM
-- is 256 entries, and in Mode 1 the background owns 0-127 and the sprites
-- 128-255: 8 palettes of 15 each, half of the TG16's 16 and 16. So the
-- port does what an SNES artist would:
--   * the quilt's 16 schemes become 8 palettes, and the other 8 schemes are
--     the same palettes with the ramp turned round (light seams-in instead of
--     dark), separate tiles, so neighbouring patches still differ;
--   * the 16 balloons share 8 sprite palettes in twins; the second of each
--     pair is mirrored so its light falls the other way.
-- That caps the picture at 241 colours (120 + 120 + backdrop) against the
-- TG16's 481. The SNES could choose those 241 from 32,768 rather than 512,
-- and could rewrite palettes between rows of the quilt with HDMA, or blend
-- with colour math, but it cannot hold more than 256 entries at once.

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
local function c3(c) -- a TG16 3-bit colour, exactly
  return rgb(floor(c[1] * 255 / 7 + 0.5), floor(c[2] * 255 / 7 + 0.5), floor(c[3] * 255 / 7 + 0.5))
end
local function reversed(c) return c == 1 and 1 or 17 - c end

local CHR, MAP, OBJ = 0x0000, 0x3000, 0x4000

function init()
  sram.meter = nil
  local base = {}
  local n = 1 -- tile 0 stays blank
  for py = 0, 15 do
    for px = 0, 15 do
      local q, d, v = PW.patch(px, py)
      local flip = q % 2
      local key = (d * 15 + v) * 2 + flip
      if not base[key] then
        base[key] = n
        local tiles = PW.patch_tiles(d, v, flip == 1 and reversed or nil)
        for c = 1, 4 do tile4(CHR + (n + c - 1) * 16, tiles[c]) end
        n = n + 4
      end
      local b = base[key]
      local attr = (q // 2) << 10
      local x0, y0 = px * 2, py * 2
      vram[MAP + y0 * 32 + x0] = b | attr
      vram[MAP + y0 * 32 + x0 + 1] = (b + 1) | attr
      vram[MAP + (y0 + 1) * 32 + x0] = (b + 2) | attr
      vram[MAP + (y0 + 1) * 32 + x0 + 1] = (b + 3) | attr
    end
  end
  for p = 0, 7 do
    local s = PW.quilt_colours(2 * p)
    for i = 1, 15 do cgram[p * 16 + i] = c3(s[i]) end
    local b = PW.balloon_colours(2 * p)
    for i = 1, 15 do cgram[128 + p * 16 + i] = c3(b[i]) end
  end
  cgram[0] = 0
  -- The balloon: 32x32 OBJ at tile 0 (tiles cy * 16 + cx).
  local px = PW.balloon()
  for cy = 0, 3 do
    for cx = 0, 3 do
      local cell = {}
      for i = 0, 63 do cell[i + 1] = px[(cy * 8 + i // 8) * 32 + cx * 8 + i % 8 + 1] end
      tile4(OBJ + (cy * 16 + cx) * 16, cell)
    end
  end
end

function frame(t, f)
  mode = 1; brightness = 15
  screen.main.bg1 = true; screen.main.obj = true
  bg[1].char_base = CHR; bg[1].map_base = MAP
  obj.char_base = OBJ; obj.size_sel = 1 -- small 8x8, large 32x32
  local sx, sy = PW.scroll(t)
  bg[1].scroll.x = sx; bg[1].scroll.y = sy - 1
  for k = 0, 15 do
    local x, y = PW.balloon_pos(k, t)
    local o = obj[k]
    o.x, o.y, o.tile, o.pal = x, y, 0, k // 2
    o.flip_x, o.large, o.on, o.prio = k % 2 == 1, true, true, 2
  end
  sram.meter = { rows = { { label = "Palettes", value = "8 BG + 8 OBJ of 15 (TG16: 16 + 16)" } } }
end
