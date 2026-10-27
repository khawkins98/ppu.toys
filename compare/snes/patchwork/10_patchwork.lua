-- 10_patchwork.lua: the patchwork scene, the same on both machines.
--
-- This file is identical in crates/tg16-core/toys/patchwork/ and
-- compare/snes/patchwork/. It picks the colours, lays out the quilt and moves
-- the balloons; each main.lua decides how many palettes it can give them.
--
-- The colours: all 512 of the TG16's 9-bit colours, less the 32 darkest,
-- sorted by hue and cut into 32 groups of 15, each group sorted dark to light.
-- Even groups are quilt schemes, odd groups balloon schemes, so both cover
-- the whole colour wheel: 16 + 16 schemes of 15, 480 colours in all.

PW = {}

-- HSV hue of a 0..7 colour (0..6, greys first) and its brightness.
local function hue(c)
  local r, g, b = c[1], c[2], c[3]
  local mx, mn = max(r, g, b), min(r, g, b)
  if mx == mn then return -1 end
  local d = mx - mn
  local h
  if mx == r then h = (g - b) / d
  elseif mx == g then h = 2 + (b - r) / d
  else h = 4 + (r - g) / d end
  if h < 0 then h = h + 6 end
  return h
end
local function lum(c) return c[1] * 30 + c[2] * 59 + c[3] * 11 end

-- No table.sort in this Lua: a plain insertion sort.
local function sort(t, less)
  for i = 2, #t do
    local v = t[i]
    local j = i - 1
    while j >= 1 and less(v, t[j]) do t[j + 1] = t[j]; j = j - 1 end
    t[j + 1] = v
  end
end

-- PW.SCHEME[1..32]: 15 colours each, {r, g, b} in 0..7, dark to light.
PW.SCHEME = {}
do
  local all = {}
  for i = 0, 511 do all[#all + 1] = { i >> 6, (i >> 3) & 7, i & 7, id = i } end
  sort(all, function(a, b)
    local la, lb = lum(a), lum(b)
    if la ~= lb then return la < lb end
    return a.id < b.id
  end)
  local keep = {}
  for i = 33, 512 do keep[#keep + 1] = all[i] end
  for _, c in ipairs(keep) do c.h = hue(c) end
  sort(keep, function(a, b)
    if a.h ~= b.h then return a.h < b.h end
    local la, lb = lum(a), lum(b)
    if la ~= lb then return la < lb end
    return a.id < b.id
  end)
  for g = 0, 31 do
    local s = {}
    for i = 1, 15 do s[i] = keep[g * 15 + i] end
    sort(s, function(a, b)
      local la, lb = lum(a), lum(b)
      if la ~= lb then return la < lb end
      return a.id < b.id
    end)
    PW.SCHEME[g + 1] = s
  end
end
-- Quilt scheme q (0..15) is group 2q; balloon scheme k (0..15) group 2k+1.
function PW.quilt_colours(q) return PW.SCHEME[2 * q + 1] end
function PW.balloon_colours(k) return PW.SCHEME[2 * k + 2] end

-- Quilt patches are 16x16 (2x2 tiles). Patch (px, py) of the 16x16 grid:
-- its scheme q, design d (0..4) and variant v (0..14).
function PW.patch(px, py)
  local q = (px * 5 + py * 3 + (px * py) % 7) % 16
  return q, (px + 2 * py) % 5, (px * 7 + py * 11) % 15
end

-- Colour index (1..15) of pixel (x, y) of a patch; 1 (darkest) is the seam.
function PW.patch_index(d, v, x, y)
  if x == 0 or y == 0 then return 1 end
  local cx, cy = abs(x - 8), abs(y - 8)
  local k
  if d == 0 then k = max(cx, cy)                      -- squares
  elseif d == 1 then k = cx + cy                      -- diamond
  elseif d == 2 then k = (x + y) // 2                 -- stripes
  elseif d == 3 then k = ((x // 4 + y // 4) % 2) * 7 + min(cx, cy) -- checks
  else k = (cx < 2 or cy < 2) and 14 - max(cx, cy) or (cx * cy) // 4 end -- cross
  return 2 + (k + v) % 14
end

-- The four 8x8 tiles (64 indices each) of a patch, after `remap` (index ->
-- index; nil for none).
function PW.patch_tiles(d, v, remap)
  local out = {}
  for cell = 0, 3 do
    local ox, oy = (cell % 2) * 8, (cell // 2) * 8
    local px = {}
    for i = 0, 63 do
      local c = PW.patch_index(d, v, ox + i % 8, oy + i // 8)
      px[i + 1] = remap and remap(c) or c
    end
    out[cell + 1] = px
  end
  return out
end

-- A hot-air balloon, 32x32 (1024 indices): 14 gores dark to light across the
-- envelope, so a dark-to-light scheme shades it like a lit sphere, with the
-- outline, basket and ropes in the darkest colour.
function PW.balloon()
  local function env(x, y)
    if y < 0 or y > 22 then return 0 end
    local dy = (y - 12) / 12
    local half = 14 * sqrt(max(0, 1 - dy * dy))
    if y > 12 then half = half * (1 - (y - 12) / 16) end
    local dx = x - 15.5
    if abs(dx) <= half and half > 0.5 then
      return 2 + min(13, max(0, floor((dx / half + 1) / 2 * 14)))
    end
    return 0
  end
  local px = {}
  for i = 0, 1023 do
    local x, y = i % 32, i // 32
    local c = env(x, y)
    if c > 0 and (env(x - 1, y) == 0 or env(x + 1, y) == 0 or env(x, y - 1) == 0 or env(x, y + 1) == 0) then
      c = 1
    end
    if y >= 23 and y <= 25 and (x == 12 or x == 19) then c = 1 end
    if y >= 26 and y <= 30 and x >= 12 and x <= 19 then c = (y == 26 or x == 12 or x == 19) and 1 or 3 end
    px[i + 1] = c
  end
  return px
end

-- Balloon k (0..15): two rows of eight rising, 144 lines apart, so a line
-- crosses at most eight (16 sprite units on the TG16, 32 slivers on the SNES).
function PW.balloon_pos(k, t)
  local row, col = k // 8, k % 8
  local x = 2 + col * 32 + floor(sin(t * 0.8 + k * 1.7) * 6)
  local y = 256 - floor((t * 18 + row * 144) % 288)
  return x, y
end

function PW.scroll(t) return floor(t * 8), floor(t * 3) end
