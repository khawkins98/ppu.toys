-- Comet Lancer, part 3: the background. A sky and ground that tilt with
-- the ship, drawn in smooth dithered gradients, and an info bar along the
-- bottom of the screen. All of it is one background layer.
--
-- How the tilted horizon works. The TG16 cannot rotate or scale a layer,
-- but on every line it can pick which map row to show (a BYR reload) and
-- scroll it sideways. A tilted horizon crosses each line at one point, so
-- each line needs a row that is sky on one side of a point and ground on
-- the other, scrolled so that point lands where the horizon crosses that
-- line. The map holds such rows ("ramp rows"): the colour along the row
-- follows the distance from the horizon, stretched for 11 tilt angles
-- (1 to 22 degrees, each way). Lines the horizon does not cross show a
-- flat row of the right colour for their distance. The edge itself is
-- exact on every line; only the gradient's stretch is rounded to the
-- nearest stored angle.
--
-- The gradients are 15-colour palettes (one for the ground, one for the
-- sky) drawn with a 2x2 ordered dither between neighbouring colours. The
-- dither pattern alternates between even and odd map pixel rows, and each
-- line starts on the row with its own parity, so it stays a fine
-- checkerboard on screen.

F = 160          -- focal length in pixels
VIEW_H = 192     -- lines of 3D view; the info bar is below
T_FIRST = 512    -- first tile (above the 128x64 BAT)

-- Map rows.
BAR_ROW = 0      -- 4 rows: the info bar
RAMP_ROW = 4     -- 22 rows: ramp rows, [level] ground-right then ground-left
GROUND_ROW = 26  -- 20 rows: flat ground, haze to near
SKY_ROW = 46     -- 18 rows: flat sky, horizon to top
-- Tilt angles of the ramp rows, in degrees: finer near level, where a
-- small error in the stretch shows most.
RAMP_DEG = { 1, 2, 3, 4.5, 6, 8, 10, 12.5, 15, 18, 22 }
RAMP_LEVELS = #RAMP_DEG
RAMP_SIN = {}
for i, d in ipairs(RAMP_DEG) do RAMP_SIN[i] = sin(d * pi / 180) end
GROUND_FLATS, SKY_FLATS = 20, 18
GROUND_LEN, SKY_LEN = 704, 320   -- how a ramp row's 1024 pixels are shared

local tiles = {}          -- tile pixel string -> tile number
local next_tile = T_FIRST

-- A tile from a 64-character string of hex digits, shared when repeated.
function shared_tile(px)
  local t = tiles[px]
  if t then return t end
  t = next_tile
  next_tile = next_tile + 1
  tile(t, px)
  tiles[px] = t
  return t
end

function tiles_used() return next_tile end

local HEX = { [0] = "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f" }

-- A tile from a 64-entry table of colour numbers.
function tile_from(px)
  local s = ""
  for i = 1, 64 do s = s .. HEX[px[i]] end
  return shared_tile(s)
end

-- Gradient value 1..15 (fractional) -> a dithered tile between the two
-- nearest colours. Values are rounded to quarter steps, so there are at
-- most 57 of these tiles.
local BAYER = { [0] = 0, 2, 3, 1 }   -- 2x2, index (y % 2) * 2 + x % 2
local grad_tiles = {}
function grad_tile(v)
  v = clamp(v, 1, 15)
  local q = floor(v * 4 + 0.5)
  local t = grad_tiles[q]
  if t then return t end
  local k, r = q // 4, q % 4
  local px = {}
  for y = 0, 7 do
    for x = 0, 7 do
      local c = k
      if r > BAYER[(y % 2) * 2 + x % 2] and k < 15 then c = k + 1 end
      px[y * 8 + x + 1] = c
    end
  end
  t = tile_from(px)
  grad_tiles[q] = t
  return t
end

-- Distance from the horizon (pixels, measured square to it) -> colour.
-- Ground runs from haze (1) to dark near grass (15); sky from horizon
-- haze (1) to deep blue overhead (15).
-- Most of the change is in a narrow haze band; past it the colour
-- changes slowly, so lines far from the horizon can share flat rows
-- without a visible step.
function ground_value(d) return 1 + 9 * (1 - math.exp(-d / 13)) + 5 * (1 - math.exp(-d / 110)) end
function sky_value(h) return 1 + 7 * (1 - math.exp(-h / 14)) + 7 * (1 - math.exp(-h / 90)) end

-- Flat rows cover these values; pick the nearest.
local function flat_index(v, n) return clamp(floor((v - 1) / 14 * (n - 1) + 0.5), 0, n - 1) end

local function build_gradients()
  local GP, SP = 2, 3   -- background palettes for ground and sky
  -- Ramp rows. Row RAMP_ROW + 2a - 2 has ground to the right of column 0
  -- (u 0..GROUND_LEN-1) and sky to its left (the rest, wrapping round).
  -- The next row is the mirror: sky to the right, ground to the left.
  -- Ground gets the longer part: it fills more of the screen.
  for a = 1, RAMP_LEVELS do
    local s = RAMP_SIN[a]
    for tx = 0, 127 do
      local u = tx * 8 + 4
      local g = u < GROUND_LEN
      local dist = g and u * s or (1024 - u) * s
      map(tx, RAMP_ROW + 2 * a - 2, grad_tile(g and ground_value(dist) or sky_value(dist)), g and GP or SP)
      local g2 = u >= SKY_LEN
      local dist2 = g2 and (1024 - u) * s or u * s
      map(tx, RAMP_ROW + 2 * a - 1, grad_tile(g2 and ground_value(dist2) or sky_value(dist2)), g2 and GP or SP)
    end
  end
  for i = 0, GROUND_FLATS - 1 do
    local t = grad_tile(1 + 14 * i / (GROUND_FLATS - 1))
    for tx = 0, 127 do map(tx, GROUND_ROW + i, t, GP) end
  end
  for i = 0, SKY_FLATS - 1 do
    local t = grad_tile(1 + 14 * i / (SKY_FLATS - 1))
    for tx = 0, 127 do map(tx, SKY_ROW + i, t, SP) end
  end
end

-- Palettes: 15-step ramps, set again when a bomb flashes the screen.
local function ramp_colors(stops, n)
  local out = {}
  for i = 1, n do
    local k = (i - 1) / (n - 1) * (#stops - 1)
    local j = min(floor(k), #stops - 2)
    local t = k - j
    local a, b = stops[j + 1], stops[j + 2]
    out[i] = { lerp(a[1], b[1], t), lerp(a[2], b[2], t), lerp(a[3], b[3], t) }
  end
  return out
end
GROUND_COLORS = ramp_colors({ { 176, 208, 210 }, { 122, 178, 136 }, { 72, 150, 88 },
                              { 50, 124, 68 }, { 38, 104, 58 } }, 15)
SKY_COLORS = ramp_colors({ { 196, 222, 228 }, { 130, 184, 220 }, { 72, 130, 200 },
                           { 40, 84, 168 }, { 22, 44, 112 } }, 15)

function world_palettes(flash)
  for i = 1, 15 do
    local g, s = GROUND_COLORS[i], SKY_COLORS[i]
    local f = flash or 0
    pal(2, i, rgb(floor(lerp(g[1], 255, f)), floor(lerp(g[2], 255, f)), floor(lerp(g[3], 240, f))))
    pal(3, i, rgb(floor(lerp(s[1], 255, f)), floor(lerp(s[2], 255, f)), floor(lerp(s[3], 240, f))))
  end
end

-- 5x7 font for the info bar, drawn with a drop shadow.
local GLYPHS = {
  ["0"] = { "01110", "10001", "10011", "10101", "11001", "10001", "01110" },
  ["1"] = { "00100", "01100", "00100", "00100", "00100", "00100", "01110" },
  ["2"] = { "01110", "10001", "00001", "00110", "01000", "10000", "11111" },
  ["3"] = { "11110", "00001", "00001", "01110", "00001", "00001", "11110" },
  ["4"] = { "00010", "00110", "01010", "10010", "11111", "00010", "00010" },
  ["5"] = { "11111", "10000", "11110", "00001", "00001", "10001", "01110" },
  ["6"] = { "00110", "01000", "10000", "11110", "10001", "10001", "01110" },
  ["7"] = { "11111", "00001", "00010", "00100", "01000", "01000", "01000" },
  ["8"] = { "01110", "10001", "10001", "01110", "10001", "10001", "01110" },
  ["9"] = { "01110", "10001", "10001", "01111", "00001", "00010", "01100" },
  A = { "01110", "10001", "10001", "11111", "10001", "10001", "10001" },
  B = { "11110", "10001", "10001", "11110", "10001", "10001", "11110" },
  C = { "01110", "10001", "10000", "10000", "10000", "10001", "01110" },
  D = { "11110", "10001", "10001", "10001", "10001", "10001", "11110" },
  E = { "11111", "10000", "10000", "11110", "10000", "10000", "11111" },
  F = { "11111", "10000", "10000", "11110", "10000", "10000", "10000" },
  G = { "01110", "10001", "10000", "10111", "10001", "10001", "01111" },
  H = { "10001", "10001", "10001", "11111", "10001", "10001", "10001" },
  I = { "01110", "00100", "00100", "00100", "00100", "00100", "01110" },
  K = { "10001", "10010", "10100", "11000", "10100", "10010", "10001" },
  L = { "10000", "10000", "10000", "10000", "10000", "10000", "11111" },
  M = { "10001", "11011", "10101", "10101", "10001", "10001", "10001" },
  N = { "10001", "11001", "10101", "10011", "10001", "10001", "10001" },
  O = { "01110", "10001", "10001", "10001", "10001", "10001", "01110" },
  P = { "11110", "10001", "10001", "11110", "10000", "10000", "10000" },
  R = { "11110", "10001", "10001", "11110", "10100", "10010", "10001" },
  S = { "01111", "10000", "10000", "01110", "00001", "00001", "11110" },
  T = { "11111", "00100", "00100", "00100", "00100", "00100", "00100" },
  U = { "10001", "10001", "10001", "10001", "10001", "10001", "01110" },
  V = { "10001", "10001", "10001", "10001", "10001", "01010", "00100" },
  W = { "10001", "10001", "10001", "10101", "10101", "11011", "10001" },
  X = { "10001", "10001", "01010", "00100", "01010", "10001", "10001" },
  Y = { "10001", "10001", "01010", "00100", "00100", "00100", "00100" },
  ["-"] = { "00000", "00000", "00000", "11111", "00000", "00000", "00000" },
  ["!"] = { "00100", "00100", "00100", "00100", "00100", "00000", "00100" },
  ["."] = { "00000", "00000", "00000", "00000", "00000", "01100", "01100" },
  [","] = { "00000", "00000", "00000", "00000", "01100", "00100", "01000" },
  ["'"] = { "00100", "00100", "01000", "00000", "00000", "00000", "00000" },
  ["?"] = { "01110", "10001", "00001", "00110", "00100", "00000", "00100" },
  [":"] = { "00000", "01100", "01100", "00000", "01100", "01100", "00000" },
  J = { "00111", "00010", "00010", "00010", "00010", "10010", "01100" },
  Q = { "01110", "10001", "10001", "10001", "10101", "10010", "01101" },
  Z = { "11111", "00001", "00010", "00100", "01000", "10000", "11111" },
}
local font = {}

local function build_font()
  local blank = {}
  for i = 1, 64 do blank[i] = 0 end
  font[" "] = tile_from(blank)
  for ch, rows in pairs(GLYPHS) do
    local px = {}
    for i = 1, 64 do px[i] = 0 end
    for pass = 1, 2 do
      for y = 1, 7 do
        for x = 1, 5 do
          if string.sub(rows[y], x, x) == "1" then
            if pass == 1 then px[y * 8 + x + 2] = 5 else px[(y - 1) * 8 + x + 1] = 1 end
          end
        end
      end
    end
    font[ch] = tile_from(px)
  end
end

-- Map writes cost VRAM time, so text only rewrites the cells that changed.
local shown = {}
function text(tx, ty, s, pal_)
  for i = 1, #s do
    local ch = string.sub(s, i, i)
    local key = ty * 128 + tx + i - 1
    if shown[key] ~= ch then
      shown[key] = ch
      map(tx + i - 1, ty, font[ch] or font[" "], pal_ or 0)
    end
  end
end

-- Meter tiles: 8 tiles of 0..8 pixels each. Colour 2 is the fill, so the
-- same tiles make the shield bar (palette 0) and the roll meter (palette 4).
local bar = {}
local function build_bar()
  for n = 0, 8 do
    local px = {}
    for y = 0, 7 do
      for x = 0, 7 do
        local c = 4
        if y == 1 or y == 6 then c = 3
        elseif y < 1 or y > 6 then c = 0
        elseif x < n then c = (y == 2) and 1 or 2 end
        px[y * 8 + x + 1] = c
      end
    end
    bar[n] = tile_from(px)
  end
end
local meter_shown = {}
function meter(tx, ty, value, maxv, pal_)
  local px = floor(value * 64 / maxv + 0.5)
  for i = 0, 7 do
    local n = clamp(px - i * 8, 0, 8)
    local key = ty * 128 + tx + i
    if meter_shown[key] ~= n then
      meter_shown[key] = n
      map(tx + i, ty, bar[n], pal_)
    end
  end
end

-- Bomb icons: a red and white disc, or an empty socket.
local bomb_tiles = {}
local function build_bombs()
  for full = 0, 1 do
    local px = {}
    for y = 0, 7 do
      for x = 0, 7 do
        local dx, dy = x - 3.5, y - 3.5
        local r = dx * dx + dy * dy
        local c = 0
        if r < 13 then
          if full == 1 then
            c = (r < 3) and 1 or ((dx + dy < 0) and 7 or 6)
          else
            c = (r > 8) and 3 or 4
          end
        end
        px[y * 8 + x + 1] = c
      end
    end
    bomb_tiles[full] = tile_from(px)
  end
end
local bombs_shown = {}
function bomb_icons(tx, ty, n)
  for i = 0, 2 do
    local v = (i < n) and 1 or 0
    if bombs_shown[i] ~= v then
      bombs_shown[i] = v
      map(tx + i, ty, bomb_tiles[v], 0)
    end
  end
end

-- The co-pilot portrait: ROVY, a little robot, 32x32 with a frame, in
-- background palette 1. Two mouth frames; only the bottom row differs.
local portrait = {}
local function build_portrait()
  for frame_ = 0, 1 do
    local b = {}
    for i = 1, 32 * 32 do b[i] = 0 end
    local function set(x, y, c) if x >= 0 and x < 32 and y >= 0 and y < 32 then b[y * 32 + x + 1] = c end end
    for y = 0, 31 do
      for x = 0, 31 do
        local c = (y < 16) and 3 or 13
        if (y >= 12 and y < 20) and (x + y) % 2 == 0 then c = 13 end
        if x == 0 or y == 0 or x == 31 or y == 31 then c = 1
        elseif x == 1 or y == 1 or x == 30 or y == 30 then c = 2 end
        set(x, y, c)
      end
    end
    -- antenna
    for y = 4, 8 do set(15, y, 6); set(16, y, 5) end
    set(15, 3, 10); set(16, 3, 10); set(15, 2, 10); set(16, 2, 14)
    -- head, shaded left to right
    for y = 9, 29 do
      for x = 6, 25 do
        local corner = (y == 9 or y == 29) and (x == 6 or x == 25)
        if not corner then
          local c = (x < 11) and 6 or ((x < 21) and 5 or 4)
          if y > 26 then c = 4 end
          set(x, y, c)
        end
      end
    end
    -- visor and eyes
    for y = 12, 17 do for x = 8, 23 do set(x, y, 7) end end
    for y = 14, 15 do
      for x = 10, 13 do set(x, y, 8) end
      for x = 18, 21 do set(x, y, 8) end
    end
    set(10, 14, 9); set(18, 14, 9)
    -- mouth grille
    for y = 21, 25 do
      for x = 11, 20 do
        local c = 11
        if frame_ == 0 then
          c = (y % 2 == 0) and 12 or 11
        elseif y == 21 or y == 25 then
          c = 12
        end
        set(x, y, c)
      end
    end
    portrait[frame_] = {}
    for ty = 0, 3 do
      for tx = 0, 3 do
        local px = {}
        for y = 0, 7 do
          for x = 0, 7 do px[y * 8 + x + 1] = b[(ty * 8 + y) * 32 + tx * 8 + x + 1] end
        end
        portrait[frame_][ty * 4 + tx] = tile_from(px)
      end
    end
  end
  local cols = { rgb(220, 225, 235), rgb(110, 120, 140), rgb(40, 80, 170), rgb(70, 76, 92),
                 rgb(140, 148, 165), rgb(200, 206, 220), rgb(20, 24, 40), rgb(80, 230, 255),
                 rgb(230, 255, 255), rgb(240, 60, 50), rgb(30, 34, 48), rgb(170, 176, 190),
                 rgb(28, 56, 130), rgb(255, 200, 120) }
  for i, c in ipairs(cols) do pal(1, i, c) end
end

local mouth_shown = -1
function show_portrait(tx, ty, mouth_open)
  local m = mouth_open and 1 or 0
  if mouth_shown == -1 then
    for i = 0, 15 do map(tx + i % 4, ty + i // 4, portrait[0][i], 1) end
    mouth_shown = 0
  end
  if m ~= mouth_shown then
    mouth_shown = m
    for i = 0, 15 do
      if portrait[0][i] ~= portrait[1][i] then map(tx + i % 4, ty + i // 4, portrait[m][i], 1) end
    end
  end
end

-- The bar's top edge: a two-pixel rule.
local function build_bar_frame()
  local px = {}
  for i = 1, 64 do px[i] = 0 end
  for x = 0, 7 do px[x + 1] = 3; px[8 + x + 1] = 5 end
  local edge = tile_from(px)
  for tx = 0, 127 do map(tx, BAR_ROW, edge, 0) end
end

function build_world()
  bat.w, bat.h = 128, 64
  build_font()
  build_bar()
  build_bombs()
  for ty = 0, 3 do
    for tx = 0, 127 do map(tx, BAR_ROW + ty, font[" "], 0) end
  end
  build_bar_frame()
  build_portrait()
  build_gradients()
  world_palettes(0)
  -- Info bar palette 0; palette 4 is the same with a purple fill.
  local bar_cols = { rgb(255, 255, 255), rgb(90, 230, 120), rgb(150, 160, 185), rgb(12, 18, 40),
                     rgb(6, 8, 24), rgb(230, 40, 60), rgb(255, 170, 190) }
  for i, c in ipairs(bar_cols) do pal(0, i, c); pal(4, i, c) end
  pal(4, 2, rgb(160, 110, 255))
  color(0, rgb(24, 32, 64))   -- backdrop: the bar's panel colour
end

-- Register this frame's raster hooks. hor: the horizon's line at the
-- screen centre; tilt: its angle (radians, positive turns it clockwise).
function world_frame(hor, tilt)
  bg.scroll.x, bg.scroll.y = 0, 0
  local s, c = sin(tilt), cos(tilt)
  -- the stored tilt nearest this one (0: flying level enough for flat rows)
  local level, sa = 0, 0
  local deg = abs(tilt) * 180 / pi
  if deg >= 0.5 then
    local bd = 1e9
    for i = 1, RAMP_LEVELS do
      local e = abs(RAMP_DEG[i] - deg)
      if e < bd then level, bd = i, e end
    end
    sa = RAMP_SIN[level]
  end
  local ground_right = s < 0
  local ramp = RAMP_ROW + 2 * level - (ground_right and 2 or 1)
  local shown_row = -100
  hdma(0, VIEW_H - 1, function(y)
    -- d(x) = A - x*s is the distance below the horizon at column x;
    -- dc is the distance at the screen centre.
    local A = (y - hor) * c + 128 * s
    local dc = A - 128 * s
    local row, scroll = nil, 0
    if level > 0 then
      local x0 = A / s
      -- Columns of the row the line can show with the edge placed exactly
      -- (the row holds GROUND_LEN of ground and SKY_LEN of sky).
      local fits
      if ground_right then fits = x0 <= SKY_LEN and x0 > 255 - GROUND_LEN
      else fits = x0 > 255 - SKY_LEN and x0 <= GROUND_LEN end
      if x0 >= 0 and x0 < 256 then
        -- The horizon crosses this line: put the row's edge on it.
        row, scroll = ramp, floor(-x0 + 0.5) % 1024
      else
        -- Off the screen. t is how far along the row, from its edge, the
        -- centre's colour is stored: match that (the stored stretch is
        -- only near this tilt, so matching the centre beats placing the
        -- off-screen edge). Near the edge, place the edge exactly; too far
        -- for the row, a flat row is close enough.
        local t = abs(dc) / sa
        local len = (dc > 0) and GROUND_LEN or SKY_LEN
        if t < 128 then
          if fits then row, scroll = ramp, floor(-x0 + 0.5) % 1024 end
        elseif t + 128 <= len then
          -- ground and sky run away from the edge in opposite directions,
          -- and the two row types mirror
          local u = ((dc > 0) == ground_right) and t or 1024 - t
          row, scroll = ramp, floor(u - 128) % 1024
        end
      end
    end
    if not row then
      if dc > 0 then row = GROUND_ROW + flat_index(ground_value(dc), GROUND_FLATS)
      else row = SKY_ROW + flat_index(sky_value(-dc), SKY_FLATS) end
    end
    local want = shown_row + 1
    if want // 8 ~= row then want = row * 8 + y % 2 end
    BYR = want
    shown_row = want
    bg.scroll.x = scroll
  end)
  -- The info bar: back to map row 0, no sprites over it.
  hdma(VIEW_H, VIEW_H, function() BYR = BAR_ROW * 8 end)
  hdma(VIEW_H, display.height - 1, function() show.sprites = false end)
end
