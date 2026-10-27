-- Fernwick (TurboGrafx-16): a small top-down sword-and-key adventure.
--
-- The game itself (map, enemies, collision, the attract-mode autopilot,
-- music sequencing) is 30_game.lua, shared unchanged with the SNES version
-- in compare/snes/fernwick. This file draws it the TG16 way, with one
-- background layer, 64 sprites and the PSG:
--
--   * HUD: a raster split. Lines 0-31 show map rows 24-27 (the HUD); a BYR
--     write on line 32 starts the playfield at the room. During a sideways
--     scroll the HUD lines also get their own BXR (32 more raster lines).
--   * Tree canopies over the hero: the canopy is in the map, and when the
--     hero walks under a tree its canopy cells are redrawn as front sprites
--     (only the ones touching him, to save the 16-per-line sprite budget).
--   * Translucency: shadows and the wisps flicker on alternate frames.
--   * Room scrolls: the next room is drawn beside the current one in the
--     64x64 map and BXR/BYR slide over; then it is redrawn at home.
--   * Cave iris: a closing box: masked lines above and below (layers off)
--     and solid 32x64 sprites at the sides.
--   * The dark cave: palettes dimmed, with raster palette writes that light
--     a band of lines around the hero (the SNES lights a circle).
--   * Damage flash and fades: palette rewrites (no colour math, no
--     brightness register).
--   * Title: the crest unfolds with a BYR reload per line (vertical
--     scaling by row picking), a decaying BXR wave, and a backdrop gradient.
--   * Music and effects on the PSG. cpu() charges the Lua game logic.

START_LABEL = "RUN"

local T0 = 256          -- block tiles (ART.tiles[i] -> T0 + i - 1)
local TH = 384          -- HUD/font tiles
local TL = 448          -- logo tiles (20 x 4)
local TC = 528          -- crest tiles (16 x 16)
local P0 = 200          -- sprite patterns
local PSOLID = 240      -- 8 solid patterns for the 32x64 iris masks (aligned)
local HUD_ROW = 24      -- HUD in map rows 24-27
local TITLE_ROW = 28    -- the title screen is map rows 28-55, laid out as shown

local PATTERN = {}      -- sprite name -> pattern
local SPAL = {}         -- sprite name -> palette

local function scale_color(c, k)
  local r, g, b = (c >> 16) & 255, (c >> 8) & 255, c & 255
  return rgb(floor(r * k), floor(g * k), floor(b * k))
end
local function c24(c) return rgb((c >> 16) & 255, (c >> 8) & 255, c & 255) end

-- Palettes ----------------------------------------------------------------

local bg_state = {}     -- last written colour per BG entry, to write only changes
local function set_bg(p, i, c)
  local e = p * 16 + i
  if bg_state[e] ~= c then
    bg_state[e] = c
    color(e, c)
  end
end

local function load_bg_palettes(k, tint)
  for p = 1, #ART.bgpal do
    for i = 1, 15 do
      local c = ART.bgpal[p][i + 1]
      if tint then
        local r, g, b = (c >> 16) & 255, (c >> 8) & 255, c & 255
        c = (min(255, r + 120) << 16) | (floor(g * 0.6) << 8) | floor(b * 0.6)
      end
      set_bg(p - 1, i, scale_color(c, k))
    end
  end
end

-- Map drawing -------------------------------------------------------------

local function draw_block(tx, ty, ch)
  local b = ART.blocks[ch]
  if not b then return end
  local t = b.t
  map(tx % 64, ty % 64, T0 + t[1], b.p)
  map((tx + 1) % 64, ty % 64, T0 + t[2], b.p)
  map(tx % 64, (ty + 1) % 64, T0 + t[3], b.p)
  map((tx + 1) % 64, (ty + 1) % 64, T0 + t[4], b.p)
end

local function draw_room(id, tx, ty)
  for r = 0, 11 do
    for c = 0, 15 do draw_block(tx + c * 2, ty + r * 2, room_cell(id, c, r)) end
  end
end

local function glyph(ch)
  local i = ART.hudch[ch] or 0
  return TH + i, 8 + ART.hud[i + 1].p
end

local function text(tx, ty, s, pal_override)
  for i = 1, string.len(s) do
    local t, p = glyph(string.sub(s, i, i))
    map(tx + i - 1, ty, t, pal_override or p)
  end
end

local function clear_rows(ty0, ty1, tx0, tx1)
  for ty = ty0, ty1 do
    for tx = tx0 or 0, tx1 or 31 do map(tx, ty, T0, 0) end
  end
end

-- Jobs from the game, within a per-frame VRAM budget: a whole room (768
-- words) fits, so the redraw at home happens in one frame.
local BUDGET = 1100
local function run_jobs()
  local used = 0
  while #G.jobs > 0 do
    local j = G.jobs[1]
    local cost = (j[1] == "room") and 768 or 4
    if used > 0 and used + cost > BUDGET then break end
    if j[1] == "room" then draw_room(j[2], j[3], j[4]) else draw_block(j[2], j[3], j[4]) end
    used = used + cost
    local rest = {}
    for i = 2, #G.jobs do rest[#rest + 1] = G.jobs[i] end
    G.jobs = rest
  end
  return used
end

local function draw_hud()
  local rows = hud_rows()
  for r = 1, 4 do
    for c = 0, 31 do
      local t, p = glyph(rows[r][c])
      map(c, HUD_ROW + r - 1, t, p)
    end
  end
end

local function center_text(ty, s, pal)
  text(16 - string.len(s) // 2, ty, s, pal)
end

local shown_msg = nil
local function draw_message()
  clear_rows(0, 23)
  local lines = MESSAGES[G.msg]
  for i, s in ipairs(lines) do
    if s == "PRESS START" then s = "PRESS " .. START_LABEL end
    center_text(9 + (i - 1) * 2 + (#lines == 2 and 1 or 0), s)
  end
end

-- Setup -------------------------------------------------------------------

function init()
  bat.w, bat.h = 64, 64
  for i, hx in ipairs(ART.tiles) do tile(T0 + i - 1, hx) end
  for i, h in ipairs(ART.hud) do tile(TH + i - 1, h.px) end
  -- logo (20x4 tiles) and crest (16x16 tiles) from their pixel rows
  local function img_tiles(img, base)
    local n = 0
    for ty = 0, img.h // 8 - 1 do
      for tx = 0, img.w // 8 - 1 do
        local s = ""
        for y = 0, 7 do s = s .. string.sub(img.rows[ty * 8 + y + 1], tx * 8 + 1, tx * 8 + 8) end
        tile(base + n, s)
        n = n + 1
      end
    end
  end
  img_tiles(ART.logo, TL)
  img_tiles(ART.crest, TC)

  -- sprites
  local p = P0
  for name, s in pairs(ART.spr) do
    pattern(p, s.px)
    PATTERN[name], SPAL[name] = p, s.p
    p = p + 1
  end
  for k = 0, 7 do pattern(PSOLID + k, ART.spr.solid.px) end

  -- palettes
  load_bg_palettes(1)
  for i = 1, 15 do
    set_bg(5, i, c24(ART.crestpal[1][i + 1]))
    set_bg(6, i, c24(ART.logopal[1][i + 1]))
  end
  for h = 1, #ART.hudpal do
    for i = 1, 3 do set_bg(7 + h, i, c24(ART.hudpal[h][i + 1])) end
  end
  for i = 1, 3 do set_bg(12, i, c24(ART.hudpal[1][i + 1])) end
  for sp = 1, #ART.sprpal do
    for i = 1, 15 do spal(sp - 1, i, c24(ART.sprpal[sp][i + 1])) end
  end
  spal(4, 1, c24(ART.hudpal[1][1]))
  color(0, c24(ART.hudpal[1][1]))

  -- the title screen, laid out in map rows 28-55 exactly as it shows
  clear_rows(TITLE_ROW, 63, 0, 63)
  for ty = 0, 15 do
    for tx = 0, 15 do map(8 + tx, TITLE_ROW + 3 + ty, TC + ty * 16 + tx, 5) end
  end
  for ty = 0, 3 do
    for tx = 0, 19 do map(6 + tx, TITLE_ROW + 19 + ty, TL + ty * 20 + tx, 6) end
  end
  center_text(TITLE_ROW + 23, "- THE SUNSTONE -")
  center_text(TITLE_ROW + 25, "PRESS RUN", 12)

  wave(1, "pulse25")
  wave(2, "triangle")
  wave(3, "square")
  wave(4, "sine")
  psg(1, { vol = 20, left = 15, right = 13 })
  psg(2, { vol = 26 })
  psg(3, { vol = 22 })
  psg(4, { vol = 18 })
  psg(5, { vol = 18 })
  psg(6, { vol = 24 })

  game_init()
  draw_room(G.room, 0, 0)
  draw_hud()
end

-- Sound -------------------------------------------------------------------

local drum_t, sfx_tone, sfx_noise = 0, nil, nil
local SFX = {
  -- tone channel 3: list of {hz, vol} per frame;  noise channel 6: {rate, vol}
  sword = { noise = { { 28, 20 }, { 26, 18 }, { 24, 14 }, { 22, 8 } } },
  cut = { noise = { { 18, 22 }, { 16, 18 }, { 14, 12 }, { 12, 6 } } },
  hit = { tone = { { 660, 24 }, { 520, 22 }, { 400, 18 }, { 300, 12 } } },
  kill = { noise = { { 10, 26 }, { 9, 24 }, { 8, 22 }, { 7, 18 }, { 6, 14 }, { 5, 10 }, { 4, 6 } } },
  hurt = { tone = { { 220, 26 }, { 180, 24 }, { 150, 22 }, { 120, 18 }, { 100, 14 }, { 90, 8 } } },
  gem = { tone = { { 1568, 22 }, { 1568, 22 }, { 2093, 22 }, { 2093, 20 }, { 2637, 18 }, { 2637, 14 }, { 2637, 8 } } },
  heart = { tone = { { 784, 22 }, { 988, 22 }, { 1175, 22 }, { 1568, 20 }, { 1568, 14 } } },
  key = { tone = { { 880, 24 }, { 880, 24 }, { 1109, 24 }, { 1109, 24 }, { 1319, 24 }, { 1319, 24 },
                   { 1760, 24 }, { 1760, 22 }, { 1760, 18 }, { 1760, 12 }, { 1760, 6 } } },
  door = { noise = { { 6, 26 }, { 5, 26 }, { 4, 24 }, { 4, 20 }, { 3, 16 }, { 3, 10 } } },
  stairs = { tone = { { 523, 20 }, { 494, 20 }, { 440, 20 }, { 392, 20 }, { 349, 18 }, { 330, 16 },
                      { 294, 14 }, { 262, 10 } } },
  die = { tone = { { 440, 24 }, { 415, 24 }, { 392, 22 }, { 370, 22 }, { 349, 20 }, { 330, 18 },
                   { 311, 16 }, { 294, 12 }, { 277, 8 } } },
}

local function sound()
  local ev, silent = music_events()
  if silent and not G.song then
    psg(1, { on = false }); psg(2, { on = false })
  end
  if ev[1] then
    if ev[1] == -1 then psg(1, { on = false }) else psg(1, { on = true, note = ev[1] }) end
  end
  if ev[2] then
    if ev[2] == -1 then psg(2, { on = false }) else psg(2, { on = true, note = ev[2] }) end
  end
  if ev[3] then
    local d = ev[3]
    if d == "K" then psg(5, { on = true, noise = 22, vol = 22 }); drum_t = 3
    elseif d == "S" then psg(5, { on = true, noise = 27, vol = 20 }); drum_t = 3
    elseif d == "H" then psg(5, { on = true, noise = 31, vol = 10 }); drum_t = 1 end
  elseif drum_t > 0 then
    drum_t = drum_t - 1
    if drum_t == 0 then psg(5, { on = false }) end
  end
  for _, name in ipairs(G.sfx) do
    local s = SFX[name]
    if s and s.tone then sfx_tone = { s.tone, 1 } end
    if s and s.noise then sfx_noise = { s.noise, 1 } end
  end
  if sfx_tone then
    local step = sfx_tone[1][sfx_tone[2]]
    if step then
      psg(3, { on = true, hz = step[1], vol = step[2] })
      sfx_tone[2] = sfx_tone[2] + 1
    else
      psg(3, { on = false }); sfx_tone = nil
    end
  end
  if sfx_noise then
    local step = sfx_noise[1][sfx_noise[2]]
    if step then
      psg(6, { on = true, noise = step[1], vol = step[2] })
      sfx_noise[2] = sfx_noise[2] + 1
    else
      psg(6, { on = false }); sfx_noise = nil
    end
  end
end

-- Sprites -------------------------------------------------------------------

local LAYER_ORDER = { fx = 1, sword = 2, hero = 3, foe = 4, item = 5, wisp = 6, shadow = 7 }

local function draw_sprites(f, first)
  local n = first
  local list = sprite_list()
  -- canopy sprites in front of the hero, for the tree tops he is under
  if (G.mode == "play" or G.mode == "dying") and G.cam_x == 0 and G.cam_y == 0 then
    local h = G.hero
    for r = floor(h.y / 16), floor((h.y + 15) / 16) do
      for c = floor(h.x / 16), floor((h.x + 15) / 16) do
        if r >= 0 and r <= 11 and c >= 0 and c <= 15 then
          local ch = room_cell(G.room, c, r)
          if ch == "A" or ch == "B" then
            local name = (ch == "A") and "canopy_l" or "canopy_r"
            spr(n, c * 16, r * 16 + 32, PATTERN[name], { pal = SPAL[name] })
            n = n + 1
          end
        end
      end
    end
  end
  -- stable sort by layer
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
  for _, s in ipairs(sorted) do
    local layer = s[6]
    local show_it = true
    -- no colour math: shadows and wisps show on alternate frames
    if layer == "shadow" then show_it = kit.flicker(f, 0)
    elseif layer == "wisp" then show_it = kit.flicker(f, 1) end
    if show_it and n < 64 and s[2] > -16 and s[2] < 256 and s[3] > 16 and s[3] < 224 then
      spr(n, s[2], s[3], PATTERN[s[1]], { pal = SPAL[s[1]], flip_x = s[4], flip_y = s[5] })
      n = n + 1
    end
  end
  cpu(#list * 90)
  return n
end

-- Iris: a box closing on the hero. Lines outside it have both layers off;
-- inside, solid 32x64 sprites cover the sides.
local function iris(first)
  local r = G.iris
  local h = G.hero
  local cx, cy = floor(h.x + 8), floor(h.y + 8) + 32
  local hw, hh = floor(r), floor(r * 0.75)
  local n = first
  if hw < 40 then hh = -1 end
  local top, bot = max(32, cy - hh), min(223, cy + hh)
  local function off() show.bg, show.sprites = false, false end
  if hh < 0 then
    hdma(32, 223, off)
    return n
  end
  if top > 32 then hdma(32, top - 1, off) end
  if bot < 223 then hdma(bot + 1, 223, off) end
  for y = top, bot, 64 do
    local x = cx - hw - 32
    while x > -32 and n < 64 do
      spr(n, x, y, PSOLID, { pal = 4, w = 32, h = 64 }); n = n + 1; x = x - 32
    end
    x = cx + hw
    while x < 256 and n < 64 do
      spr(n, x, y, PSOLID, { pal = 4, w = 32, h = 64 }); n = n + 1; x = x + 32
    end
  end
  return n
end

-- The dark cave: palettes dimmed for the frame; raster palette writes light
-- the lines around the hero and dim them again below.
local DIM, HALF, LIT = 0.3, 0.6, 1.0
-- The dungeon palette's entries apart from the torch flames (9-11), in two
-- halves: a raster hook has time for about ten palette writes in its line,
-- so a light step is spread over two lines.
local CAVE_ENTRIES = { { 1, 2, 3, 4, 5, 6 }, { 7, 8, 12, 13, 14, 15 } }
local function cave_light(k, in_hook, half)
  local put = in_hook and color or function(e, c)
    if bg_state[e] ~= c then bg_state[e] = c; color(e, c) end
  end
  for h = half or 1, half or 2 do
    for _, i in ipairs(CAVE_ENTRIES[h]) do put(3 * 16 + i, scale_color(ART.bgpal[4][i + 1], k)) end
  end
end

-- Title ---------------------------------------------------------------------

local SKY = { 0x101030, 0x182048, 0x203060, 0x284078, 0x305090, 0x4060a0, 0x6070a8, 0x8070a0,
              0xa07090, 0xc07878, 0xd88868, 0xe8a060, 0xf0b868, 0xf8d080 }

local function title(f)
  local t = G.mt
  bg.scroll.x, bg.scroll.y = 0, TITLE_ROW * 8
  -- backdrop gradient: a palette write every 16 lines
  for i = 0, 13 do
    local c = c24(SKY[i + 1])
    hdma(i * 16, i * 16, function() color(0, c) end)
  end
  -- the crest unfolds: lines 24-151 pick crest rows by a vertical scale
  local s = min(1, t / 90)
  s = s * s * (3 - 2 * s)
  local amp = 24 * (1 - s)
  if s < 1 or t % 240 < 40 then
    local glint = (t % 240) - 4
    hdma(24, 151, function(y)
      local ly = (y - 88) / max(s, 0.02)
      if ly < -64 or ly >= 64 then
        BYR = TITLE_ROW * 8           -- a blank row
      else
        BYR = (TITLE_ROW + 3) * 8 + floor(ly + 64)
      end
      if s < 1 then
        bg.scroll.x = floor(sin(y * 0.15 + t * 0.2) * amp)
      elseif abs(y - 24 - glint * 4) < 3 then
        bg.scroll.x = (y % 2 == 0) and 1 or -1
      end
    end)
    hdma(152, 152, function() BYR = (TITLE_ROW + 19) * 8 end)
  end
  -- PRESS RUN blinks (one palette entry)
  local on = (t // 30) % 2 == 0
  set_bg(12, 2, on and c24(ART.hudpal[1][3]) or c24(0x101030))
  set_bg(12, 3, on and c24(ART.hudpal[1][4]) or c24(0x101030))
end

-- Frame ---------------------------------------------------------------------

local last_mode = nil
local water_k = 0

function frame(t, f)
  game_update(pad)
  cpu(1500 + G.work * 70 + #G.foes * 500)

  local used = run_jobs()
  cpu(used * 12)
  if G.msg ~= shown_msg then
    shown_msg = G.msg
    if G.msg and #G.jobs == 0 then draw_message() end
  end
  if G.hud_dirty then
    draw_hud()
    G.hud_dirty = false
  end

  -- palettes: water and torch cycling, damage flash
  if f % 8 == 0 then water_k = water_k + 1 end
  local wp = ART.bgpal[3]
  for i = 0, 3 do set_bg(2, 4 + i, c24(wp[5 + (i + water_k) % 4])) end
  local tp = ART.bgpal[4]
  if G.dark then
    -- handled by cave_light
  else
    load_bg_palettes(1, G.flash > 0 and G.flash % 4 >= 2)
  end
  if tp then
    local k = (f // 6) % 3
    set_bg(3, 9, c24(tp[10 + k]))
    set_bg(3, 10, c24(tp[10 + (k + 1) % 3]))
  end

  if G.mode == "title" then
    if last_mode ~= "title" and last_mode ~= nil then
      -- scrolls leave rooms in columns 32-63; the title's wave shows a few
      -- of those columns, so blank them (896 words, one vertical blank)
      clear_rows(TITLE_ROW, TITLE_ROW + 27, 32, 63)
    end
    last_mode = G.mode
    title(f)
    sound()
    return
  end
  last_mode = G.mode
  color(0, c24(ART.hudpal[1][1]))
  set_bg(12, 2, c24(ART.hudpal[1][3]))

  -- the HUD split: lines 0-31 show map rows 24-27; line 32 starts the room
  bg.scroll.x = G.cam_x % 512
  bg.scroll.y = HUD_ROW * 8
  if G.cam_x % 512 ~= 0 then
    hdma(0, 31, function() bg.scroll.x = 0 end)
  end
  local cam_y = G.cam_y % 512
  local msg = G.msg ~= nil
  hdma(32, 32, function()
    BYR = msg and 0 or cam_y
    if msg then bg.scroll.x = 0 end
  end)
  if msg and G.cam_x % 512 ~= 0 then hdma(33, 223, function() bg.scroll.x = 0 end) end

  local n = 0
  if G.iris then n = iris(n) end
  if not msg then n = draw_sprites(f, n) end

  if G.dark and G.mode ~= "iris" then
    -- dim everywhere, lit band around the hero
    cave_light(DIM)
    local hy = floor(G.hero.y) + 32
    -- a soft edge: dim -> half -> lit -> half -> dim
    local y0, y1 = max(33, hy - 44), min(222, hy + 60)
    local function at(y, k)
      if y > 32 and y < 223 then
        hdma(y, y, function() cave_light(k, true, 1) end)
        hdma(y + 1, y + 1, function() cave_light(k, true, 2) end)
      end
    end
    at(y0, HALF); at(y0 + 12, LIT); at(y1 - 12, HALF); at(y1, DIM)
  elseif G.dark then
    cave_light(0.6)
  else
    cave_light(1)
  end
  sound()
end
