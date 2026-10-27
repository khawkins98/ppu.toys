-- rom_timing.lua: a calibration toy for the ROM exporter's raster timing.
-- Every map pixel differs from its neighbours (tile, row and palette all
-- vary), so a background that is a line or a pixel off shows up in the
-- emulator comparison. Raster hooks cover each kind of write: BXR runs, raw
-- BYR reloads (including on consecutive lines and on line 1), palette
-- writes, the background enable bit, the last line, and burst mode (both
-- layers off on line 0 blanks the frame) every 30th frame.

function init()
  bat.w, bat.h = 32, 32
  for t = 0, 31 do
    local px = {}
    for i = 0, 63 do
      local x, y = i % 8, i // 8
      px[i + 1] = (x == 0) and 0 or (1 + (x + 2 * y + t) % 15)
    end
    tile(64 + t, px)
  end
  for p = 0, 15 do
    for i = 1, 15 do
      pal(p, i, rgb3((i + p) % 8, (i * 3 + p) % 8, (i * 5 + p * 3) % 8))
    end
  end
  for ty = 0, 31 do
    for tx = 0, 31 do map(tx, ty, 64 + (tx + ty * 3) % 32, ty % 16) end
  end
  color(0, rgb3(0, 0, 0))
  local px = {}
  for i = 0, 255 do px[i + 1] = (i % 16 + i // 16) % 3 + 1 end
  pattern(32, px)
  spal(0, 1, rgb3(7, 0, 0)); spal(0, 2, rgb3(7, 7, 0)); spal(0, 3, rgb3(0, 7, 7))
end

function frame(t, f)
  bg.scroll.x = f
  bg.scroll.y = 3
  spr(0, 20 + f, 30, 32)
  spr(1, 200, 120, 32)
  hdma(1, 1, function() BYR = 40 end)
  hdma(40, 49, function(y) bg.scroll.x = f + y * 3 end)
  hdma(80, 80, function() BYR = 101 end)
  hdma(100, 100, function() spal(0, 0, rgb3(5, 0, 0)) end)
  hdma(120, 120, function() BYR = 7 end)
  hdma(121, 121, function() BYR = 60 end)
  hdma(150, 150, function() pal(0, 5, rgb3(7, 7, 7)) end)
  hdma(160, 162, function() show.bg = false end)
  hdma(170, 172, function() show.bg, show.sprites = false, false end)
  hdma(190, 191, function() bg.scroll.x = 77 end)
  hdma(200, 200, function() color(0, rgb3(0, 0, 5)) end)
  hdma(205, 206, function() show.bg = false end)
  hdma(222, 222, function() show.bg = false end)
  hdma(223, 223, function() bg.scroll.x = 99 end)
  if f % 30 == 15 then
    hdma(0, 0, function() show.bg, show.sprites = false, false end)
  end
end
