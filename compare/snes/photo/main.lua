-- photo (SNES port of crates/tg16-core/toys/photo)
--
-- The same 512x224 painting (assets/ holds its two halves; build_art.py cuts
-- them from crates/tg16-core/toys/photo/assets/lake.png). Where the TG16 has
-- 16 palettes of 15 colours for its one layer, Mode 1's two 4bpp layers
-- share 8, and a layer can address only 1024 tiles, so:
--   * the left half is BG1 and the right half BG2, each with its own
--     character base and 4 palettes, side by side in 64x32 maps (each map's
--     other half is tile 0, transparent), always scrolled together;
--   * the parallax and the ripple are HDMA writes to both scroll registers,
--     the same raster splits as the TG16 toy;
--   * the reflection is darkened a little with fixed-colour subtraction;
--   * the music is the same arpeggio and pad on the S-DSP, with software
--     vibrato and the echo unit.

local L = dma("lake_left",  { char = 0x0000, map = 0x6000, pal = 0 })  -- BG1 map screen 0
local R = dma("lake_right", { char = 0x3000, map = 0x7400, pal = 64 }) -- BG2 map screen 1

local HILLS, SHORE = 128, 170 -- raster splits, as on the TG16

local ARP_I = bank("flute", { vol = 80, pan = -0.1, echo = true })
local PAD_I = bank("strings", { vol = 44, pan = 0.15, adsr = { a = 9, d = 2, s = 6, r = 0 } })
local ARP = { "E4", "B4", "E5", "G#5", "F#5", "E5", "B4", "G#4",
              "C#4", "G#4", "C#5", "E5", "D#5", "C#5", "G#4", "E4" }
local PAD = { "E3", "C#3", "A2", "B2" }
local arp_pitch = 0x1000
local pad_note

function init()
  dsp.mvol = { l = 20, r = 20 } -- about as loud as the TG16 version
  dsp.echo = { delay = 5, feedback = 60, fir = { 127, 0, 0, 0, 0, 0, 0, 0 } }
  dsp.evol = { l = 40, r = 40 }
end

function frame(t, f)
  mode = 1; brightness = 15
  screen.main.bg1 = true; screen.main.bg2 = true
  bg[1].char_base = L.char; bg[1].map_base = 0x6000; bg[1].screen_size = 1
  bg[2].char_base = R.char; bg[2].map_base = 0x7000; bg[2].screen_size = 1
  cgram[0] = rgb(18, 22, 70)

  local sky = floor(t * 12)
  local near = floor(t * 40)
  local function scroll(x)
    bg[1].scroll.x = x; bg[2].scroll.x = x
  end
  scroll(sky)
  bg[1].scroll.y = -1; bg[2].scroll.y = -1
  hdma(HILLS, SHORE - 1, function(y) scroll(near) end)

  -- The reflection: the hills' speed plus a ripple that grows toward the
  -- viewer, and slightly darker than what it reflects.
  color.addend = "fixed"; color.op = "sub"; color.fixed = rgb(24, 20, 8)
  hdma(SHORE, 223, function(y)
    local d = y - SHORE
    scroll(near + floor(sin(d * 0.35 - t * 4) * (1 + d / 18)))
    color.on.bg1 = true; color.on.bg2 = true
  end)

  -- Music: eighth notes at 120 BPM; the pad changes every two bars.
  local step = f // 15
  if f % 15 == 0 then
    sfx(ARP_I, 0, ARP[step % 16 + 1])
    arp_pitch = voice[0].pitch
    local n = PAD[(step // 16) % 4 + 1]
    if n ~= pad_note then sfx(PAD_I, 1, n); pad_note = n end
  elseif f % 15 == 13 then
    koff(0)
  end
  -- Vibrato, the way SNES sound drivers did it: nudge the pitch every tick
  -- (about 5 Hz, a fifth of a semitone deep).
  voice[0].pitch = floor(arp_pitch * 2 ^ (0.2 * sin(t * 2 * pi * 5) / 12))
end
