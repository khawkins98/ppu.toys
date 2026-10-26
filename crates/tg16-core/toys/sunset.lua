-- sunset.lua: a TurboGrafx-16 toy. One background layer, three sprites,
-- and raster tricks: a sky gradient from backdrop writes, and a ground that
-- scrolls faster than the hills (parallax from one layer).

T = 128 -- first tile above a 64x32 BAT (2048 words / 16 words per tile)

function init()
  bat.w, bat.h = 64, 32

  tile(T + 0, string.rep("0", 64))
  tile(T + 1, "00011000" .. "00122100" .. "01211210" .. "12122121" ..
              "21212212" .. "12121121" .. "21212212" .. "12121212")
  tile(T + 2, "12121212" .. "21212121" .. "12121212" .. "21212121" ..
              "12121212" .. "21212121" .. "12121212" .. "21212121")
  tile(T + 3, string.rep("3", 32) .. string.rep("4", 32))

  for tx = 0, 63 do
    local top = 16 + floor(sin(tx * 0.45) * 2.5)
    for ty = 0, 31 do
      local t = T
      if ty >= 22 then t = T + 3
      elseif ty > top then t = T + 2
      elseif ty == top then t = T + 1 end
      map(tx, ty, t, ty >= 22 and 1 or 0)
    end
  end
  pal(0, 1, rgb3(1, 4, 1))
  pal(0, 2, rgb3(1, 5, 2))
  pal(1, 3, rgb3(4, 3, 1))
  pal(1, 4, rgb3(3, 2, 1))

  -- A 32x32 ball from four 16x16 patterns. Pattern 64 starts at word 4096,
  -- clear of the BAT and the tiles (64 words per pattern).
  for cell = 0, 3 do
    local px = {}
    local cx, cy = (cell % 2) * 16, floor(cell / 2) * 16
    for i = 0, 255 do
      local x, y = cx + i % 16 - 16, cy + floor(i / 16) - 16
      local d = x * x + y * y
      px[i + 1] = d < 120 and 2 or (d < 230 and 1 or 0)
    end
    pattern(64 + cell, px)
  end
  for p = 0, 2 do
    spal(p, 1, rgb3(7, 2 + p * 2, 1))
    spal(p, 2, rgb3(7, 6, 3 + p * 2))
  end
end

function frame(t, f)
  local bob = floor(sin(t * 3) * 6)
  spr(0, 40, 60 + bob, 64, { w = 32, h = 32, pal = 0 })
  spr(1, 120, 118, 64, { w = 32, h = 32, pal = 1, front = false })
  spr(2, 200, 150, 64, { w = 32, h = 32, pal = 2, flip_y = true })

  bg.scroll.x = floor(t * 20)
  hdma(0, 127, function(y)
    if y % 16 == 0 then
      local b = floor(y / 16)
      color(0, rgb3(floor(b / 2), floor(b / 2) + 1, 7 - floor(b / 3)))
    end
  end)
  hdma(176, 223, function(y)
    bg.scroll.x = floor(t * 60) + (y - 176) * 2
  end)
end
