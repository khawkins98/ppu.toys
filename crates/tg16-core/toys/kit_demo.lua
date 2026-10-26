-- kit_demo.lua: SNES-style effects on one TG16 background layer.
-- Mountains, hills and a road scroll at different speeds (kit.parallax and
-- kit.floor), clouds are a sprite layer behind the hills, a ghost flickers
-- for transparency, and the top and bottom are letterboxed (kit.mask_lines).

T = 128   -- tiles start above the 64x32 BAT
P = 64    -- sprite patterns start above the tiles

function init()
  bat.w, bat.h = 64, 32
  tile(T, string.rep("0", 64))
  tile(T + 1, string.rep("1", 64))                                   -- mountain
  tile(T + 2, "00000001" .. "00000011" .. "00000111" .. "00001111" ..
              "00011111" .. "00111111" .. "01111111" .. "11111111") -- slope /
  tile(T + 3, "10000000" .. "11000000" .. "11100000" .. "11110000" ..
              "11111000" .. "11111100" .. "11111110" .. "11111111") -- slope \
  tile(T + 4, string.rep("2", 64))                                   -- hill
  tile(T + 5, string.rep("3", 32) .. string.rep("4", 32))            -- road stripes
  tile(T + 6, "33333333" .. "33333333" .. "33355333" .. "33355333" ..
              "33355333" .. "33355333" .. "33333333" .. "33333333") -- road marks

  for tx = 0, 63 do
    -- Mountains: rows 6..11, peaks every 16 tiles.
    local h = 4 - abs((tx % 16) - 8) // 2
    for ty = 6, 11 do
      local top = 11 - h
      local t = T
      if ty > top then t = T + 1 elseif ty == top then t = (tx % 16 < 8) and T + 2 or T + 3 end
      map(tx, ty, t, 0)
    end
    -- Hills: rows 12..15.
    local hh = 13 + floor(sin(tx * 0.7) * 1.2)
    for ty = 12, 15 do map(tx, ty, ty >= hh and T + 4 or T, 0) end
    -- Road texture: rows 16..31, used by kit.floor.
    for ty = 16, 31 do map(tx, ty, (ty % 4 == 0 and tx % 8 == 3) and T + 6 or T + 5, 1) end
  end
  color(0, rgb3(2, 4, 7))
  pal(0, 1, rgb3(3, 3, 5))
  pal(0, 2, rgb3(1, 4, 1))
  pal(1, 3, rgb3(2, 2, 2))
  pal(1, 4, rgb3(3, 3, 3))
  pal(1, 5, rgb3(7, 7, 7))

  -- Cloud (32x16: patterns P, P+1) and ghost (16x16: P+2).
  local function blob(cx, cy, r)
    local px = {}
    for i = 0, 255 do
      local x, y = i % 16 + cx, i // 16 + cy
      px[i + 1] = ((x - 16) ^ 2 / 2 + (y - 8) ^ 2 * 2 < r) and 1 or 0
    end
    return px
  end
  pattern(P, blob(0, 0, 120))
  pattern(P + 1, blob(16, 0, 120))
  pattern(P + 2, blob(8, 0, 60))
  spal(0, 1, rgb3(7, 7, 7))
  spal(1, 1, rgb3(5, 6, 7))
end

function frame(t, f)
  local cam = t * 40
  -- Rows 48..95 are mountains, 96..127 hills; offsets line the map rows up.
  bg.scroll.y = 0
  kit.parallax(cam, { { 24, 95, 0.25 }, { 96, 127, 0.6 } })
  kit.floor({ horizon = 128, bottom = 199, z = t * 300, x = cam, height = 24,
              curve = sin(t) * 2, row0 = 128, rows = 128 })

  kit.sprite_layer({ first = 0, count = 5, pattern = P, w = 32, h = 16,
                     y = 40, x = cam * 0.1, spacing = 64, front = false })
  kit.flicker_spr(f, 10, 120 + floor(sin(t * 2) * 30), 104, P + 2, { pal = 1 })

  kit.mask_lines(0, 23, rgb3(0, 0, 0))
  kit.mask_lines(200, 223, rgb3(0, 0, 0))
end
