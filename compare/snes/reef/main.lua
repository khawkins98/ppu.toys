-- reef (SNES side of crates/tg16-core/toys/reef)
--
-- The same reef and the same sixteen species, the SNES way. CGRAM holds 256
-- colours and Mode 1's sprites own the upper 128, so:
--   * the reef gets the 8 background palettes of 15 (the importer reduces the
--     painting to fit, with ordered dither), where the TG16 uses 16;
--   * the fish get 8 sprite palettes for 16 species: build_art.py draws them
--     the way an SNES artist would plan it, two species to a palette (black
--     and white for the eyes, then a 4-shade body and 2-shade fins each)
--     instead of 8-shade bodies and 5-shade fins;
--   * the open water is a smooth HDMA gradient into the backdrop, 15-bit
--     colour on every line for no CPU time (the TG16's 9-bit colour gives
--     five bands).
-- What the SNES has instead: 32,768 colours to choose those entries from,
-- against the TG16's 512, which is why its reef looks smoother even with
-- half the palettes.

local REEF = dma("reef", { char = 0x0000, map = 0x7800, pal = 0 })
local FISH = dma("fish", { char = 0x4000, pal = 0 })

local function lane(k, t) -- the TG16 toy's lanes, unchanged
  local speed = 14 + (k * 7) % 19
  local span = 256 + 64
  local p = (t * speed + k * 53) % (2 * span)
  local right = p >= span
  local x = right and (p - span) - 32 or (span - p) - 32
  local y = 18 + k * 12 + floor(sin(t * 1.3 + k) * 3)
  return floor(x), y, right
end

local TOP, BOTTOM = rgb(83, 160, 236), rgb(14, 64, 158)

function init()
  sram.meter = nil
end

function frame(t, f)
  mode = 1; brightness = 15
  screen.main.bg1 = true; screen.main.obj = true
  bg[1].char_base = REEF.char; bg[1].map_base = REEF.map
  bg[1].scroll.y = -1
  obj.char_base = FISH.char; obj.size_sel = 1 -- small 8x8, large 32x32

  for k = 0, 15 do
    local x, y, right = lane(k, t)
    local cell = FISH.cells[k + 1]
    local o = obj[k]
    o.x, o.y, o.tile, o.pal = x, y, FISH.tile + cell.tile, FISH.pal + cell.pal
    o.flip_x, o.large, o.on, o.prio = right, true, true, 2
  end

  -- One HDMA channel: CGRAM 0 on every water line.
  hdma(0, 223, function(y)
    cgram[0] = lerp_color(TOP, BOTTOM, y / 150)
  end)
  sram.meter = { rows = { { label = "HDMA channels", value = "1 of 8 (water gradient)", frac = 1 / 8 } } }
end
