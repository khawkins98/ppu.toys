-- copper: fourteen stripes whose colours change on every line, to test the
-- TG16's raster colour writes against the SNES.
--
-- The VCE's palette can be written at any time, and the CPU decides how many
-- entries a raster interrupt rewrites, so the TG16 is not held to the SNES's
-- 8 HDMA channels: here every line rewrites 14 entries, one per stripe,
-- taking each from a 256-step colour table (10_copper.lua, shared with the
-- SNES port). The price is CPU time: an interrupt on all 224 lines and 3136
-- palette writes a frame, most of the frame (see the budget meter).
--
-- On hardware the writes go left to right, stripe 0 first, so each lands
-- before the beam reaches its stripe. The colours are the table's 9-bit
-- roundings.
--
-- The SNES port (compare/snes/copper/) does not race the beam at all: the
-- stripes only slide, so Mode 2's offset-per-tile scrolls a pre-drawn
-- gradient up or down each stripe, for no CPU time and no HDMA channels.

local T = 64 -- first tile above a 32x32 BAT
local TAB = {}

function init()
  bat.w, bat.h = 32, 32
  tile(T, string.rep("0", 64))
  local hexd = "0123456789abcdef"
  for i = 0, CU.N - 1 do
    tile(T + 1 + i, string.rep(string.sub(hexd, i + 2, i + 2), 64))
  end
  for ty = 0, 31 do
    for tx = 0, 31 do
      local x = tx * 8
      local i = (x - CU.X0) // 16
      map(tx, ty, (x >= CU.X0 and i < CU.N) and T + 1 + i or T, 0)
    end
  end
  for s = 0, 255 do
    local r, g, b = CU.colour(s)
    TAB[s] = rgb3(floor(r * 7 + 0.5), floor(g * 7 + 0.5), floor(b * 7 + 0.5))
  end
  color(0, rgb3(0, 0, 0))
end

local off = {}

function frame(t, f)
  for i = 0, CU.N - 1 do off[i] = CU.offset(i, t) end
  hdma(0, 223, function(y)
    for i = 0, CU.N - 1 do color(i + 1, TAB[(y + off[i]) % 256]) end
  end)
  -- What cpu() adds to the meter's own charges (90 cycles a raster
  -- interrupt, 20 a palette write): indexing each stripe's table pointer
  -- (lda (zp),y instead of an immediate, about 4 more cycles a write) and
  -- setting up the 14 pointers once a frame.
  cpu(224 * CU.N * 4 + CU.N * 40)
end
