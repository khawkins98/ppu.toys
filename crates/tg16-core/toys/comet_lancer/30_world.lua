-- Comet Lancer, part 3: the background. A HUD band, a sky gradient from
-- backdrop writes, a mountain band, and a perspective checkerboard floor
-- made from one background layer with a raster trick on every line.
--
-- How the floor works: the TG16 can scroll a line sideways and pick which
-- map row it shows (a BYR reload), but it cannot scale. So the map holds
-- the checkerboard pre-scaled: map row k is a row of squares w_k pixels
-- wide, from 1 pixel (near the horizon) to 128 (under the camera). Each
-- floor line works out how far away it is, picks the row whose square
-- width matches, and scrolls it so the squares line up with the camera's
-- x. Moving forward flips the checker phase, which is just a shift by one
-- square. Farther rows use hazier palettes.

F = 160          -- focal length in pixels
SQUARE = 64      -- checker square size in world units
T_BLANK = 256    -- first tile (above the 64x64 BAT)
HUD_ROW = 1      -- map row of the HUD text
TITLE_ROW = 4    -- map rows of the attract-mode title
MOUNT_ROW = 14   -- 3 map rows of mountains
FLOOR_ROW = 17   -- first floor row; row FLOOR_ROW + k is level k

local tiles = {}          -- tile pixel string -> tile number
local next_tile = T_BLANK

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

-- 5x7 font for the HUD, drawn with a drop shadow.
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
  C = { "01110", "10001", "10000", "10000", "10000", "10001", "01110" },
  D = { "11110", "10001", "10001", "10001", "10001", "10001", "11110" },
  E = { "11111", "10000", "10000", "11110", "10000", "10000", "11111" },
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
  Y = { "10001", "10001", "01010", "00100", "00100", "00100", "00100" },
  ["-"] = { "00000", "00000", "00000", "11111", "00000", "00000", "00000" },
  ["!"] = { "00100", "00100", "00100", "00100", "00100", "00000", "00100" },
}
local font = {}

local function build_font()
  font[" "] = shared_tile(string.rep("0", 64))
  for ch, rows in pairs(GLYPHS) do
    local px = {}
    for i = 1, 64 do px[i] = "0" end
    for y = 1, 7 do
      for x = 1, 5 do
        if string.sub(rows[y], x, x) == "1" then
          px[(y - 1) * 8 + x + 1] = "1"
          -- shadow one pixel down and right, where there is no ink
          local s = y * 8 + x + 2
          if y < 8 and x < 7 and px[s] == "0" then px[s] = "5" end
        end
      end
    end
    -- the shadow pass can land on ink drawn later; redraw the ink
    for y = 1, 7 do
      for x = 1, 5 do
        if string.sub(rows[y], x, x) == "1" then px[(y - 1) * 8 + x + 1] = "1" end
      end
    end
    local s = ""
    for i = 1, 64 do s = s .. px[i] end
    font[ch] = shared_tile(s)
  end
end

-- Map writes cost VRAM time, so text only rewrites the cells that changed.
local shown = {}
function text(tx, ty, s, pal_)
  for i = 1, #s do
    local ch = string.sub(s, i, i)
    local key = ty * 64 + tx + i - 1
    if shown[key] ~= ch then
      shown[key] = ch
      map(tx + i - 1, ty, font[ch] or font[" "], pal_ or 0)
    end
  end
end

-- Shield bar: 8 tiles, each 0..8 pixels full.
local bar = {}
local function build_bar()
  for n = 0, 8 do
    local s = ""
    for y = 0, 7 do
      for x = 0, 7 do
        local c = "4"
        if y == 0 or y == 7 then c = "3"
        elseif y >= 2 and y <= 5 and x < n then c = (y == 2) and "1" or "2" end
        s = s .. c
      end
    end
    bar[n] = shared_tile(s)
  end
end
local bar_shown = {}
function shield_bar(tx, ty, value, maxv)
  local px = floor(value * 64 / maxv + 0.5)
  for i = 0, 7 do
    local n = clamp(px - i * 8, 0, 8)
    if bar_shown[i] ~= n then
      bar_shown[i] = n
      map(tx + i, ty, bar[n], 0)
    end
  end
end

-- Floor levels: square widths in pixels, nearest first is the last.
FLOOR_W = {}
local function build_floor()
  local seen = {}
  FLOOR_W[0] = 0    -- level 0: solid haze, for lines near the horizon
  for k = 0, 45 do
    local w = floor(1.5 * (128 / 1.5) ^ (k / 45) + 0.5)
    if not seen[w] then
      seen[w] = true
      FLOOR_W[#FLOOR_W + 1] = w
    end
  end
  local levels = #FLOOR_W
  for k = 0, levels do
    local w = FLOOR_W[k]
    -- haze: 1 at the horizon, 0 for squares 40+ pixels wide
    local haze = (w == 0) and 1 or clamp(1 - (w - 1) / 40, 0, 1) ^ 2
    local p = 2 + floor((1 - haze) * 13 + 0.5)
    for tx = 0, 63 do
      local s = ""
      for x = 0, 7 do
        local c = 1
        if w > 0 and (tx * 8 + x) // w % 2 == 1 then c = 2 end
        s = s .. c
      end
      map(tx, FLOOR_ROW + k, shared_tile(string.rep(s, 8)), p)
    end
  end
  -- 14 palettes from full haze (2) to none (15)
  local ga, gb = { 70, 170, 80 }, { 40, 118, 58 }
  local hz = { 172, 206, 214 }
  for p = 2, 15 do
    local h = 1 - (p - 2) / 13
    local function mix(c) return rgb(floor(lerp(c[1], hz[1], h)), floor(lerp(c[2], hz[2], h)),
                                     floor(lerp(c[3], hz[3], h))) end
    pal(p, 1, mix(ga))
    pal(p, 2, mix(gb))
  end
end

-- Pick the floor level for a square width (a lookup table, as a 6280
-- game would use, filled on first use).
local LEVEL_OF = {}
function floor_level(w)
  if w < 2 then return 0 end
  local q = min(floor(w * 2), 400)
  local k = LEVEL_OF[q]
  if k then return k end
  local best, bd = 1, 1e9
  for i = 1, #FLOOR_W do
    local d = abs(FLOOR_W[i] - q / 2)
    if d < bd then best, bd = i, d end
  end
  LEVEL_OF[q] = best
  return best
end

-- Mountains: a 512-pixel-wide ridge in 24 rows, fading into haze at the
-- bottom so it meets the floor.
local function build_mountains()
  local hgt = {}
  for x = 0, 511 do
    local a = x / 512 * 2 * pi
    hgt[x] = 7 + 6 * sin(a * 3 + 1) + 4 * sin(a * 7 + 2) + 2 * sin(a * 17)
  end
  for ty = 0, 2 do
    for tx = 0, 63 do
      local s = ""
      for y = 0, 7 do
        local py = ty * 8 + y          -- 0 at the top, 23 at the bottom
        for x = 0, 7 do
          local h = hgt[tx * 8 + x]
          local c = 0
          if py >= 24 - h then
            c = (py <= 24 - h + 1) and 2 or 1
            if py >= 20 then c = 3 end
          end
          s = s .. c
        end
      end
      map(tx, MOUNT_ROW + ty, shared_tile(s), 1)
    end
  end
  pal(1, 1, rgb(80, 100, 150))
  pal(1, 2, rgb(200, 215, 235))
  pal(1, 3, rgb(172, 206, 214))
end

function build_world()
  bat.w, bat.h = 64, 64
  build_font()
  build_bar()
  for ty = 0, 63 do
    for tx = 0, 63 do map(tx, ty, font[" "], 0) end
  end
  build_floor()
  build_mountains()
  -- HUD palette
  pal(0, 1, rgb(255, 255, 255))
  pal(0, 2, rgb(90, 230, 120))
  pal(0, 3, rgb(150, 160, 180))
  pal(0, 4, rgb(20, 30, 50))
  pal(0, 5, rgb(10, 15, 40))
end

-- Sky colors from the top of the screen to the horizon.
local SKY = {
  { 18, 30, 80 }, { 28, 48, 112 }, { 44, 72, 140 }, { 66, 100, 165 },
  { 92, 130, 185 }, { 120, 160, 200 }, { 150, 186, 210 }, { 172, 206, 214 },
}

-- Register this frame's raster hooks. cam = {x, y, z}; hor = horizon line;
-- hud_lines = how many top lines keep scroll x = 0 for text.
function world_frame(cam, hor, hud_lines)
  local mscroll = floor(cam.x * 0.3) % 512
  bg.scroll.x = mscroll
  bg.scroll.y = 0
  color(0, rgb(SKY[1][1], SKY[1][2], SKY[1][3]))
  hdma(0, hud_lines - 1, function() bg.scroll.x = 0 end)

  -- Sky: a backdrop write every few lines down to the mountains.
  local m0 = hor - 24
  local step = max(4, (m0 - 8) // 7)
  local i = 2
  for y = 8 + step, m0, step do
    local c = SKY[min(i, #SKY)]
    hdma(y, y, function() color(0, rgb(c[1], c[2], c[3])) end)
    i = i + 1
  end
  -- Mountains start with one BYR reload.
  hdma(m0, m0, function() BYR = MOUNT_ROW * 8 end)

  -- Floor: every line below the horizon.
  local camy, camx, camz = cam.y, cam.x, cam.z
  local shown_row = (MOUNT_ROW * 8) + 23   -- the row the line above showed
  hdma(hor, 223, function(y)
    local dy = y - hor + 0.5
    local w = SQUARE * dy / camy
    local k = floor_level(w)
    local lw = FLOOR_W[k]
    local row = FLOOR_ROW + k
    local want = shown_row + 1
    if want // 8 ~= row then want = row * 8 end
    BYR = want
    shown_row = want
    if lw > 0 then
      local dz = camy * F / dy
      local zpar = floor((camz + dz) / SQUARE) % 2
      bg.scroll.x = floor(camx * lw / SQUARE - 128 + zpar * lw) % (2 * lw)
    else
      bg.scroll.x = 0
    end
  end)
end
