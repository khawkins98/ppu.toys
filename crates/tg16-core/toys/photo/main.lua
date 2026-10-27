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

-- Music: a slow, calm arpeggio. Channel 1 plays the melody with a gentle
-- vibrato from the LFO (channel 2 drives it and stays silent); channel 3 is
-- a soft triangle pad.
local ARP = { "E4", "B4", "E5", "G#5", "F#5", "E5", "B4", "G#4",
              "C#4", "G#4", "C#5", "E5", "D#5", "C#5", "G#4", "E4" }
local PAD = { "E3", "C#3", "A2", "B2" }

local setup, draw = init, frame
function init()
  if setup then setup() end
  wave(1, "sine")
  wave(2, "sine")
  wave(3, "triangle")
  psg(1, { vol = 24, left = 13, right = 15 })
  -- LFO rate = 3.58 MHz / (32 * period * freq): about 5 Hz here.
  psg(2, { on = true, period = 88 })
  lfo(255, 1)
  psg(3, { vol = 22, left = 15, right = 12 })
end

function frame(t, f)
  draw(t, f)
  local step = f // 15 -- eighth notes at 120 BPM
  psg(1, { on = f % 15 < 13, note = ARP[step % 16 + 1] })
  psg(3, { on = true, note = PAD[(step // 16) % 4 + 1] })
end
