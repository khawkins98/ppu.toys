-- reef: sixteen species of fish over a painted reef. The other side of the
-- colour story (see patchwork for the TG16's win).
--
-- The VCE holds 16 background and 16 sprite palettes of 15 colours, twice
-- the SNES's 8 and 8, so every fish species here has a palette of its own
-- (8-shade body, 5-shade fins, black and white eye) and the reef can use all
-- 16 background palettes. But each entry is one of only 512 colours (3 bits
-- a channel), so a smooth painting collapses to few distinct colours: this
-- reef has 77 after rounding, and its open water needs the importer's dither
-- to hide the steps. The SNES port in compare/snes/reef/ has half the
-- palettes, from 32,768 colours, and a free HDMA gradient for the water.
--
-- Art: compare/snes/reef/build_art.py paints assets/reef.png (water painted
-- in, so the ordered dither smooths it, the way PC Engine artists did) and
-- assets/fish.png.

local fish

function init()
  bat.w, bat.h = 32, 32
  image_bg("reef", { dither = "ordered" })
  fish = image_sprites("fish", { w = 32, h = 16 })
end

-- Each species swims in its own lane, back and forth across the screen.
local function lane(k, t)
  local speed = 14 + (k * 7) % 19
  local span = 256 + 64
  local p = (t * speed + k * 53) % (2 * span)
  local right = p >= span
  local x = right and (p - span) - 32 or (span - p) - 32
  local y = 18 + k * 12 + floor(sin(t * 1.3 + k) * 3)
  return floor(x), y, right
end

function frame(t, f)
  for k = 0, 15 do
    local x, y, right = lane(k, t)
    spr(k, x, y, fish.pattern + k * fish.step, { w = 32, h = 16, pal = fish.pals[k + 1], flip_x = right })
  end
end
