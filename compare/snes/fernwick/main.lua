-- Fernwick (SNES): the same game as crates/tg16-core/toys/fernwick, drawn
-- the SNES way. 10_art.lua, 20_world.lua and 30_game.lua are the TG16
-- toy's files, copied unchanged; only this file differs.
--
--   * Mode 1, three layers: BG1 is the ground, BG2 the tree canopies (tile
--     priority 1, so they cover the hero's priority-2 sprite with no
--     sprite tricks), BG3 the HUD and messages (BG3 priority on top), which
--     simply does not scroll. HDMA hides BG1/BG2/OBJ on the HUD lines.
--   * Colour math: shadows (OBJ palette 4) and the wisps (palette 5) are
--     half-added onto the sub screen: real translucency, every frame. A hit
--     adds a fixed red to everything for a few frames.
--   * The dark cave: colour window 1 is a circle around the hero, re-aimed
--     on every line by HDMA; fixed-colour subtraction darkens outside it.
--   * Cave iris: the same circular window, clipping to black outside.
--   * Title: the crest is a Mode 7 plane that spins and zooms in, over an
--     HDMA backdrop gradient; an HDMA mode split puts BG3 text below it.
--     The logo is five 32x32 sprites. Mode 7 data shares VRAM with the
--     game's maps, so it is uploaded when the title starts (as a game would
--     during forced blank).
--   * Music and effects on the S-DSP's built-in samples.

START_LABEL = "START"

local floor, abs, min, max, sqrt = math.floor, math.abs, math.min, math.max, math.sqrt
local sin, cos = math.sin, math.cos

local MAP1 = 0x0000   -- BG1 map, 64x64 (time-shared with Mode 7 on the title)
local MAP2 = 0x1000   -- BG2 map, 64x64
local CHR = 0x4000    -- BG1/BG2 tiles, 4bpp
local CHR3 = 0x5000   -- BG3 tiles, 2bpp
local MAP3 = 0x5800   -- BG3 map, 32x32
local OBJ = 0x6000    -- sprite tiles

local function hexv(s, i)
  local b = string.byte(s, i)
  return b >= 97 and b - 87 or b - 48
end

-- An 8x8 tile from 64 hex digits (or a pixel getter), as 4bpp or 2bpp words.
local function tile_words(addr, get, bpp)
  for y = 0, 7 do
    local p = { 0, 0, 0, 0 }
    for x = 0, 7 do
      local c = get(x, y)
      for k = 1, bpp do
        if c & (1 << (k - 1)) ~= 0 then p[k] = p[k] | (1 << (7 - x)) end
      end
    end
    vram[addr + y] = p[1] | (p[2] << 8)
    if bpp == 4 then vram[addr + 8 + y] = p[3] | (p[4] << 8) end
  end
end
local function hex_tile(addr, s, bpp)
  tile_words(addr, function(x, y) return hexv(s, y * 8 + x + 1) end, bpp)
end

local function c24(c) return rgb((c >> 16) & 255, (c >> 8) & 255, c & 255) end

local SPRITE_TILE = {}   -- sprite name -> OBJ tile number
local SPRITE_PAL = {}
local LOGO_TILE = 128     -- five 32x32 sprites from tile 128 (rows 8-15)
local function logo_tile(j) return LOGO_TILE + (j % 4) * 4 + (j // 4) * 64 end

-- Maps ----------------------------------------------------------------------

local function map_addr(base, tx, ty)
  tx, ty = tx % 64, ty % 64
  return base + (tx % 32) + (ty % 32) * 32 + (tx >= 32 and 0x400 or 0) + (ty >= 32 and 0x800 or 0)
end

local function draw_block(tx, ty, ch)
  local b = ART.blocks[ch]
  if not b then return end
  local lo, hi = b.lo or b.t, b.hi
  local pal = (b.p + 2) << 10
  for k = 0, 3 do
    local x, y = tx + k % 2, ty + k // 2
    vram[map_addr(MAP1, x, y)] = lo[k + 1] | pal
    vram[map_addr(MAP2, x, y)] = hi and (hi[k + 1] | pal | 0x2000) or 0
  end
end

local function draw_room(id, tx, ty)
  for r = 0, 11 do
    for c = 0, 15 do draw_block(tx + c * 2, ty + r * 2, room_cell(id, c, r)) end
  end
end

local function bg3(tx, ty, ch)
  local i = ART.hudch[ch] or 0
  vram[MAP3 + ty * 32 + tx] = i | (ART.hud[i + 1].p << 10) | 0x2000
end

local function bg3_text(tx, ty, s)
  for i = 1, string.len(s) do bg3(tx + i - 1, ty, string.sub(s, i, i)) end
end

local function bg3_clear(ty0, ty1)
  for ty = ty0, ty1 do for tx = 0, 31 do vram[MAP3 + ty * 32 + tx] = 0 end end
end

local function center(ty, s) bg3_text(16 - string.len(s) // 2, ty, s) end

local function draw_hud()
  local rows = hud_rows()
  for r = 1, 4 do
    for c = 0, 31 do bg3(c, r - 1, rows[r][c]) end
  end
end

-- Mode 7 crest: 8bpp tiles in the high bytes, the 128x128 map in the low
-- bytes of VRAM 0x0000-0x3fff. Colours are CGRAM 112-127.
local function upload_crest()
  local img = ART.crest
  local zero = ""
  for _ = 1, 64 do zero = zero .. "0" end
  local ids, tiles = {}, { zero }
  ids[tiles[1]] = 0
  local mapv = {}
  for ty = 0, 15 do
    for tx = 0, 15 do
      local s = ""
      for y = 0, 7 do s = s .. string.sub(img.rows[ty * 8 + y + 1], tx * 8 + 1, tx * 8 + 8) end
      if ids[s] == nil then
        tiles[#tiles + 1] = s
        ids[s] = #tiles - 1
      end
      mapv[ty * 128 + tx] = ids[s]
    end
  end
  local words = {}
  for w = 0, 0x3fff do
    local t, i = w // 64, w % 64
    local hi = 0
    if t < #tiles then
      local v = hexv(tiles[t + 1], i + 1)
      hi = v > 0 and 112 + v or 0
    end
    words[w + 1] = (hi << 8) | (mapv[w] or 0)
  end
  vr(0, words)
end

-- Setup ---------------------------------------------------------------------

local SKY = { 0x101030, 0x182048, 0x203060, 0x284078, 0x305090, 0x4060a0, 0x6070a8, 0x8070a0,
              0xa07090, 0xc07878, 0xd88868, 0xe8a060, 0xf0b868, 0xf8d080 }

local function room_palettes()
  for p = 1, #ART.bgpal do
    for i = 1, 15 do cgram[(p + 1) * 16 + i] = c24(ART.bgpal[p][i + 1]) end
  end
end

function init()
  for i, s in ipairs(ART.tiles) do hex_tile(CHR + (i - 1) * 16, s, 4) end
  for i, h in ipairs(ART.hud) do hex_tile(CHR3 + (i - 1) * 8, h.px, 2) end
  local k = 0
  for name, s in pairs(ART.spr) do
    if name ~= "solid" and string.sub(name, 1, 6) ~= "canopy" then
      local t = (k % 8) * 2 + (k // 8) * 32
      SPRITE_TILE[name], SPRITE_PAL[name] = t, s.p
      for q = 0, 3 do
        local ox, oy = (q % 2) * 8, (q // 2) * 8
        tile_words(OBJ + (t + (q % 2) + (q // 2) * 16) * 16, function(x, y)
          return hexv(s.px, (oy + y) * 16 + ox + x + 1)
        end, 4)
      end
      k = k + 1
    end
  end
  -- logo: 160x32 as five 32x32 sprites (4x4 tiles each in the 16-wide grid)
  local logo = ART.logo
  for j = 0, 4 do
    for ty = 0, 3 do
      for tx = 0, 3 do
        local t = logo_tile(j) + tx + ty * 16
        tile_words(OBJ + t * 16, function(x, y)
          return hexv(logo.rows[ty * 8 + y + 1], j * 32 + tx * 8 + x + 1)
        end, 4)
      end
    end
  end

  -- CGRAM: HUD (BG3) palettes 0-3 at 0-15, rooms in BG palettes 2-6, the
  -- crest at 112-127, sprites in OBJ palettes 0-5, the logo in OBJ 7.
  for h = 1, #ART.hudpal do
    for i = 0, 3 do cgram[(h - 1) * 4 + i] = c24(ART.hudpal[h][i + 1]) end
  end
  room_palettes()
  for sp = 1, 6 do
    for i = 1, 15 do cgram[128 + (sp - 1) * 16 + i] = c24(ART.sprpal[sp][i + 1]) end
  end
  for i = 1, 15 do cgram[240 + i] = c24(ART.logopal[1][i + 1]) end

  bg3_clear(0, 31)
  dsp.mvol = { l = 80, r = 80 }
  game_init()
end

-- Sound ---------------------------------------------------------------------

local LEAD = bank("lead", { vol = 52, pan = -0.1, adsr = { a = 15, d = 4, s = 5, r = 18 } })
local BASS = bank("bass", { vol = 90 })
local KICK = bank("kick", { vol = 96 })
local SNARE = bank("snare", { vol = 64 })
local HAT = bank("hat", { vol = 40, pan = 0.25 })
local BELL = bank("bell", { vol = 72 })
local PLUCK = bank("pluck", { vol = 72 })
local TOM = bank("tom", { vol = 90 })
local CRASH = bank("crash", { vol = 56 })

-- effects: { frame offset, instrument, voice, note }
local SFX = {
  sword = { { 0, HAT, 4, "G5" } },
  cut = { { 0, SNARE, 4, "C5" } },
  hit = { { 0, TOM, 5, "C5" }, { 0, HAT, 4, "C5" } },
  kill = { { 0, SNARE, 5, "C4" }, { 3, TOM, 4, "G3" } },
  hurt = { { 0, KICK, 5, "C4" }, { 0, LEAD, 6, "C3" }, { 4, LEAD, 6, "A2" } },
  gem = { { 0, BELL, 6, "C6" }, { 4, BELL, 7, "G6" } },
  heart = { { 0, PLUCK, 6, "C5" }, { 4, PLUCK, 7, "E5" }, { 8, PLUCK, 6, "G5" } },
  key = { { 0, BELL, 6, "A5" }, { 6, BELL, 7, "C#6" }, { 12, BELL, 6, "E6" }, { 18, BELL, 7, "A6" } },
  door = { { 0, TOM, 5, "C3" }, { 6, TOM, 4, "G2" }, { 6, CRASH, 7, "C4" } },
  stairs = { { 0, PLUCK, 6, "C5" }, { 4, PLUCK, 7, "A4" }, { 8, PLUCK, 6, "F4" }, { 12, PLUCK, 7, "D4" } },
  die = { { 0, LEAD, 6, "A4" }, { 8, LEAD, 6, "F4" }, { 16, LEAD, 6, "D4" }, { 24, LEAD, 6, "A3" } },
}
local pending = {}

local function sound()
  local ev, silent = music_events()
  if silent and not G.song then koff(0, 1) end
  if ev[1] then
    if ev[1] == -1 then koff(0) else sfx(LEAD, 0, ev[1]) end
  end
  if ev[2] then
    if ev[2] == -1 then koff(1) else sfx(BASS, 1, ev[2]) end
  end
  if ev[3] == "K" then sfx(KICK, 2)
  elseif ev[3] == "S" then sfx(SNARE, 2)
  elseif ev[3] == "H" then sfx(HAT, 3) end
  for _, name in ipairs(G.sfx) do
    for _, e in ipairs(SFX[name] or {}) do pending[#pending + 1] = { e[1], e[2], e[3], e[4] } end
  end
  local keep = {}
  for _, e in ipairs(pending) do
    if e[1] <= 0 then sfx(e[2], e[3], e[4]) else e[1] = e[1] - 1; keep[#keep + 1] = e end
  end
  pending = keep
end

-- Frame ---------------------------------------------------------------------

local in_title = false
local shown_msg = nil

local function common_regs()
  mode = 1
  brightness = 15
  bg3_priority = true
  bg[1].char_base, bg[1].map_base, bg[1].screen_size = CHR, MAP1, 3
  bg[2].char_base, bg[2].map_base, bg[2].screen_size = CHR, MAP2, 3
  bg[3].char_base, bg[3].map_base, bg[3].screen_size = CHR3, MAP3, 0
  bg[3].scroll.x, bg[3].scroll.y = 0, -1
  obj.char_base, obj.size_sel = OBJ, 3   -- 16x16 and 32x32
  screen.main.bg1, screen.main.bg2, screen.main.bg3, screen.main.obj = true, true, true, true
  screen.sub.bg1, screen.sub.bg2, screen.sub.bg3, screen.sub.obj = true, true, false, false
  CGWSEL, CGADSUB = 0x30, 0
  win.color.w1 = false
end

local function title(t)
  local mt = G.mt
  common_regs()
  -- the crest spins and zooms in on Mode 7, then breathes
  local s = min(1, mt / 100)
  s = s * s * (3 - 2 * s)
  local z = 0.04 + 0.96 * s + sin(mt * 0.03) * 0.02 * s
  local a = (1 - s) * 6.283 * 1.5
  mode = 7
  m7.a, m7.b, m7.c, m7.d = cos(a) / z, sin(a) / z, -sin(a) / z, cos(a) / z
  m7.cx, m7.cy = 64, 64
  m7.wrap = 2
  bg[1].scroll.x, bg[1].scroll.y = 64 - 128, 64 - 89
  screen.main.bg2, screen.main.bg3 = false, false
  -- below the crest: Mode 1 with the BG3 text
  hdma(152, 223, function()
    mode = 1
    screen.main.bg1, screen.main.bg3 = false, true
  end)
  -- a smooth backdrop gradient (one CGRAM write per line)
  hdma(0, 223, function(y)
    local p = y / 16
    local i = min(13, floor(p))
    local j = min(13, i + 1)
    cgram[0] = lerp_color(c24(SKY[i + 1]), c24(SKY[j + 1]), p - i)
  end)
  -- logo sprites
  for j = 0, 4 do
    local o = obj[j]
    o.x, o.y, o.tile, o.pal, o.prio = 48 + j * 32, 151, logo_tile(j), 7, 3
    o.large, o.flip_x, o.flip_y, o.on = true, false, false, true
  end
  for i = 5, 127 do obj[i].on = false end
  local on = (mt // 30) % 2 == 0
  bg3_text(10, 25, on and ("PRESS " .. START_LABEL) or "           ")
end

local LAYER_ORDER = { fx = 1, sword = 2, hero = 3, foe = 4, item = 5, wisp = 6, shadow = 7 }

local function sprites()
  local list = sprite_list()
  local sorted = {}
  for i, s in ipairs(list) do sorted[i] = s end
  for i = 2, #sorted do
    local v = sorted[i]
    local j = i - 1
    while j >= 1 and LAYER_ORDER[sorted[j][6]] > LAYER_ORDER[v[6]] do
      sorted[j + 1] = sorted[j]
      j = j - 1
    end
    sorted[j + 1] = v
  end
  local n = 0
  for _, s in ipairs(sorted) do
    if n < 128 and s[2] > -16 and s[2] < 256 and s[3] > 16 and s[3] < 224 then
      local o = obj[n]
      o.x, o.y = s[2], s[3] - 1
      o.tile, o.pal, o.prio = SPRITE_TILE[s[1]], SPRITE_PAL[s[1]], 2
      o.flip_x, o.flip_y, o.large, o.on = s[4], s[5], false, true
      n = n + 1
    end
  end
  for i = n, 127 do obj[i].on = false end
end

-- Colour window 1 as a circle of radius r around (cx, cy), line by line.
local function circle_window(cx, cy, r)
  hdma(32, 223, function(y)
    local dy = y - cy
    if abs(dy) < r then
      local h = floor(sqrt(r * r - dy * dy))
      win.w1.lo, win.w1.hi = max(0, cx - h), min(255, cx + h)
    else
      win.w1.lo, win.w1.hi = 1, 0
    end
  end)
end

function frame(t, f)
  game_update(pad)

  if G.mode == "title" then
    if not in_title then
      in_title = true
      upload_crest()
      for i = 1, 15 do cgram[112 + i] = c24(ART.crestpal[1][i + 1]) end
      bg3_clear(0, 31)
      center(23, "- THE SUNSTONE -")
    end
    title(t)
    sound()
    return
  end
  if in_title then
    in_title = false
    bg3_clear(0, 31)
    room_palettes()
    G.hud_dirty = true
  end

  for _, j in ipairs(G.jobs) do
    if j[1] == "room" then draw_room(j[2], j[3], j[4]) else draw_block(j[2], j[3], j[4]) end
  end
  G.jobs = {}
  if G.hud_dirty then draw_hud(); G.hud_dirty = false end
  if G.msg ~= shown_msg then
    shown_msg = G.msg
    bg3_clear(4, 27)
    if G.msg then
      local lines = MESSAGES[G.msg]
      for i, s in ipairs(lines) do
        if s == "PRESS START" then s = "PRESS " .. START_LABEL end
        center(13 + (i - 1) * 2 + (#lines == 2 and 1 or 0), s)
      end
    end
  end

  -- water and torch palette cycling
  local wp = ART.bgpal[3]
  local wk = f // 8
  for i = 0, 3 do cgram[4 * 16 + 4 + i] = c24(wp[5 + (i + wk) % 4]) end
  local tp = ART.bgpal[4]
  local k = (f // 6) % 3
  cgram[5 * 16 + 9] = c24(tp[10 + k])
  cgram[5 * 16 + 10] = c24(tp[10 + (k + 1) % 3])

  common_regs()
  cgram[0] = c24(ART.hudpal[1][1])
  bg[1].scroll.x, bg[1].scroll.y = G.cam_x % 512, (G.cam_y - 33) % 512
  bg[2].scroll.x, bg[2].scroll.y = bg[1].scroll.x, bg[1].scroll.y
  -- HUD lines: only BG3
  hdma(0, 31, function()
    screen.main.bg1, screen.main.bg2, screen.main.obj = false, false, false
  end)
  if G.msg then
    screen.main.bg1, screen.main.bg2, screen.main.obj = false, false, false
  else
    sprites()
  end

  local h = G.hero
  local cx, cy = floor(h.x + 8), floor(h.y + 8) + 32
  if G.iris then
    -- iris: clip to black outside a circle around the hero
    win.color.w1 = true
    circle_window(cx, cy, floor(G.iris))
    CGWSEL = 0x70  -- clip outside the colour window, no maths
  elseif G.flash > 0 and G.flash % 4 >= 2 then
    CGWSEL, CGADSUB = 0x00, 0x33   -- add fixed red to BG1, BG2, OBJ, backdrop
    color.fixed = rgb(160, 24, 24)
  elseif G.dark and G.mode ~= "dying" then
    -- lantern light: subtract outside a circle around the hero
    win.color.w1 = true
    circle_window(cx, cy, 60)
    CGWSEL, CGADSUB = 0x20, 0x83
    color.fixed = rgb(72, 72, 60)
  else
    -- shadows and wisps: half-add OBJ palettes 4-7 onto the ground
    CGWSEL, CGADSUB = 0x02, 0x50
  end
  sound()
end
