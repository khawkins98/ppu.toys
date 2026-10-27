-- patchwork: hot-air balloons over a quilt, to show the TG16's palettes.
--
-- The VCE holds 16 background palettes and 16 sprite palettes of 15 colours,
-- plus the backdrop: 481 colours at once (482 counting the overscan colour).
-- Here the quilt uses 16 colour schemes, one per background palette, and
-- each of the 16 balloons has a sprite palette of its own: 480 different
-- colours, nearly all of the 512 the TG16 can make. The SNES port in
-- compare/snes/patchwork/ has 8 and 8 (CGRAM is 256 entries, and Mode 1's
-- sprites own the upper 128), so it repeats its schemes.
--
-- 10_patchwork.lua (shared with the SNES port) picks the colours and lays out
-- the scene; this file only loads it the TG16 way.

local T = 64 -- first tile above a 32x32 BAT
local P = 96 -- balloon patterns (word 6144, above the tiles)

function init()
  bat.w, bat.h = 32, 32
  -- One tile set per (design, variant), shared by every scheme: the
  -- palette number in the BAT picks the colours.
  local base = {}
  local n = T
  for py = 0, 15 do
    for px = 0, 15 do
      local q, d, v = PW.patch(px, py)
      local key = d * 15 + v
      if not base[key] then
        base[key] = n
        local tiles = PW.patch_tiles(d, v)
        for c = 1, 4 do tile(n + c - 1, tiles[c]) end
        n = n + 4
      end
      local b = base[key]
      map(px * 2, py * 2, b, q); map(px * 2 + 1, py * 2, b + 1, q)
      map(px * 2, py * 2 + 1, b + 2, q); map(px * 2 + 1, py * 2 + 1, b + 3, q)
    end
  end
  for q = 0, 15 do
    local s = PW.quilt_colours(q)
    for i = 1, 15 do pal(q, i, rgb3(s[i][1], s[i][2], s[i][3])) end
  end
  -- The balloon as four 16x16 patterns (a 32x32 sprite reads P..P+3).
  local px = PW.balloon()
  for cell = 0, 3 do
    local ox, oy = (cell % 2) * 16, (cell // 2) * 16
    local p = {}
    for i = 0, 255 do p[i + 1] = px[(oy + i // 16) * 32 + ox + i % 16 + 1] end
    pattern(P + cell, p)
  end
  for k = 0, 15 do
    local s = PW.balloon_colours(k)
    for i = 1, 15 do spal(k, i, rgb3(s[i][1], s[i][2], s[i][3])) end
  end
  color(0, rgb3(0, 0, 0))
end

function frame(t, f)
  bg.scroll.x, bg.scroll.y = PW.scroll(t)
  for k = 0, 15 do
    local x, y = PW.balloon_pos(k, t)
    spr(k, x, y, P, { w = 32, h = 32, pal = k })
  end
end
