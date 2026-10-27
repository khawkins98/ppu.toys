-- copper (SNES side of crates/tg16-core/toys/copper)
--
-- The TG16 toy rewrites 14 palette entries on every line. The SNES cannot:
-- CGRAM is writable only during blanking, and HDMA has 8 channels, each
-- good for about one colour a line, so per-line palette writes would reach
-- 8 of the 14 stripes at most. But these stripes only slide, so an SNES
-- programmer would not write palettes at all:
--   * the colour cycle (10_copper.lua, the same file as the TG16's) is drawn
--     once into a column of 32 tiles, a 256-line gradient, in 8 palettes of
--     15 (the cycle is cut into 8 runs of 32 steps, 15 colours each, so a
--     colour lasts about two lines);
--   * Mode 2's offset-per-tile gives every 8-pixel column of BG1 its own
--     vertical scroll, read from a table in BG3's map; each frame sets 28
--     words of it (two columns per stripe) to the stripes' offsets.
-- That costs no HDMA channels and almost no CPU, and the colours come from
-- 32,768 instead of 512, so the SNES side is smoother. What it could not do
-- is change the colours in ways that are not a slide (stretching, or new
-- colours computed per line): that needs palette writes, where the TG16 can
-- do more per line than HDMA's 8, if it can spare the CPU.

local CHR, MAP, OPT = 0x1000, 0x0000, 0x0400

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

local function slot(s) return floor((s % 32) * 15 / 32) end

function init()
  sram.meter = nil
  -- The gradient: tile 1 + r holds steps 8r .. 8r + 7, one per pixel row,
  -- in palette r // 4.
  for r = 0, 31 do
    local px = {}
    for j = 0, 7 do
      local k = slot(8 * r + j) + 1
      for x = 0, 7 do px[j * 8 + x + 1] = k end
    end
    tile4(CHR + (1 + r) * 16, px)
  end
  for p = 0, 7 do
    for k = 0, 14 do
      local r, g, b = CU.colour(32 * p + floor((k + 0.5) * 32 / 15))
      cgram[p * 16 + k + 1] = rgb(r * 255, g * 255, b * 255)
    end
  end
  cgram[0] = 0
  for r = 0, 31 do
    for c = 0, 31 do
      local x = c * 8
      local stripe = x >= CU.X0 and (x - CU.X0) // 16 < CU.N
      vram[MAP + r * 32 + c] = stripe and ((1 + r) | ((r // 4) << 10)) or 0
    end
  end
  for c = 0, 63 do vram[OPT + c] = 0 end
end

function frame(t, f)
  mode = 2; brightness = 15
  screen.main.bg1 = true
  bg[1].char_base = CHR; bg[1].map_base = MAP
  bg[1].scroll.x = 0; bg[1].scroll.y = -1
  bg[3].map_base = OPT; bg[3].scroll.x = 0; bg[3].scroll.y = 0
  -- The offset table: screen column c reads table column c - 1; row 1 is
  -- the vertical offset, bit 13 applies it to BG1.
  for i = 0, CU.N - 1 do
    local v = ((CU.offset(i, t) - 1) & 0x3ff) | 0x2000
    local c = (CU.X0 + i * 16) // 8
    vram[OPT + 32 + c - 1] = v
    vram[OPT + 32 + c] = v
  end
  -- 65816 estimate: NMI and frame set-up (1500), 14 offsets (about 30
  -- cycles each), and 28 table words by DMA in vertical blank (about 100).
  local cycles = 1500 + CU.N * 30 + 100
  sram.meter = { rows = {
    { label = "CPU (65816 estimate)", value = floor(cycles / 49500 * 100 + 0.5) .. "% (" .. cycles .. " of 49500)", frac = cycles / 49500 },
    { label = "HDMA channels", value = "0 of 8 (offset-per-tile instead)", frac = 0 },
  } }
end
