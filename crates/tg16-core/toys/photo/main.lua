-- photo: a painted 512x224 panorama with thousands of colors, squeezed into
-- 16 palettes of 15 by the importer (ordered dither), then scrolled with
-- raster parallax: the sky drifts, the hills pass faster and the lake below
-- wobbles line by line.

function init()
  bat.w, bat.h = 64, 32 -- 512x256 pixels, so the 512-wide image wraps
  lake = image_bg("lake", { dither = "ordered" })
  -- lake.reduced, lake.colors, lake.error, lake.merged_tiles say what the
  -- reduction cost; `ppu tg16 check` prints it as a warning too.
  color(0, rgb(18, 22, 70)) -- backdrop: the top of the sky
end

local HILLS, SHORE = 128, 170 -- raster splits (the mist at 128 is flat)

function frame(t, f)
  local sky = floor(t * 12)
  local near = floor(t * 40)
  bg.scroll.x = sky
  hdma(HILLS, SHORE - 1, function(y)
    bg.scroll.x = near
  end)
  -- The reflection: same speed as the hills, plus a ripple that grows
  -- toward the viewer.
  hdma(SHORE, 223, function(y)
    local d = y - SHORE
    bg.scroll.x = near + floor(sin(d * 0.35 - t * 4) * (1 + d / 18))
  end)
end
