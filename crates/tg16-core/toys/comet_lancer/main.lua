-- Comet Lancer: an on-rails 3D shooter for the TurboGrafx-16 model.
--
-- Fly through rings (shield back), dodge towers and shoot down Hornets.
-- Controls (the viewer's keys in brackets):
--   d-pad   steer, up climbs (arrows)
--   I       fire twin lasers, hold for auto fire (X)
--   II      barrel roll: enemy shots miss while rolling (Z)
--   RUN     start (Enter); until then the ship flies itself
-- After a game over, any button plays again.
--
-- What is 3D and how (the TG16 has no polygon hardware):
--   * The Lancer is real 3D: 13 flat-shaded polygons filled by the CPU into
--     a 64x64 buffer every frame and sent to VRAM as two 32x64 sprites.
--     cpu() charges the fill time to the budget meter.
--   * Hornets, towers, rings, bolts and explosions are sprites picked from
--     sets pre-scaled in init() (the Hornet sets are rendered by the same
--     polygon code), positioned with a perspective divide.
--   * The floor is one background layer with a BYR reload and a scroll on
--     every line (see 30_world.lua). Mountains and sky are a band and
--     backdrop writes.

HOR_BASE = 92     -- horizon line when flying level
SHIP_D = 90       -- the ship flies this far ahead of the camera
SPEED = 8         -- world units per frame
FAR = 1700        -- objects appear this far ahead
SHIELD_MAX = 64

-- Game state -------------------------------------------------------------

local ship, cam, objs, lasers
local mode, mode_t, score, shield = "attract", 0, 0, SHIELD_MAX
local next_spawn, wave_i, fire_cd, roll_t, hurt_t
local last_buttons = false
local seed = 1

local function rnd()
  seed = (seed * 1103515245 + 12345) % 2147483648
  return seed / 2147483648
end

local function reset(new_mode)
  mode, mode_t = new_mode, 0
  ship = { x = 0, y = 60, vx = 0, vy = 0, roll = 0, alive = true }
  cam = { x = 0, y = 70, z = 0 }
  objs, lasers = {}, {}
  next_spawn, wave_i, fire_cd, roll_t, hurt_t = 600, 0, 0, 0, 0
  score, shield = 0, SHIELD_MAX
  seed = 7
end

local function add(o) objs[#objs + 1] = o; return o end

-- The course: a repeating list of encounters, placed by distance.
local COURSE = { "towers", "ring", "wave", "towers", "rings", "wave", "slalom", "wave2" }

local function spawn(z)
  local kind = COURSE[wave_i % #COURSE + 1]
  wave_i = wave_i + 1
  if kind == "towers" then
    for i = 0, 5 do
      local side = (i % 2 == 0) and -1 or 1
      add({ kind = "tower", x = side * (60 + rnd() * 180), y = 0, z = z + i * 130 })
    end
    return z + 900
  elseif kind == "slalom" then
    for i = 0, 7 do
      add({ kind = "tower", x = ((i % 2 == 0) and -70 or 70) + (rnd() - 0.5) * 40, y = 0, z = z + i * 160 })
    end
    return z + 1400
  elseif kind == "ring" or kind == "rings" then
    local n = kind == "ring" and 1 or 3
    local x, y = (rnd() - 0.5) * 200, 40 + rnd() * 70
    for i = 0, n - 1 do
      add({ kind = "ring", x = x + i * 50 * (rnd() - 0.5), y = y + (rnd() - 0.5) * 30, z = z + i * 260 })
    end
    return z + 500 + n * 260
  else
    local n = kind == "wave" and 3 or 5
    local cx, cy = (rnd() - 0.5) * 160, 60 + rnd() * 50
    for i = 0, n - 1 do
      local off = i - (n - 1) / 2
      add({ kind = "hornet", bx = cx + off * 45, by = cy + abs(off) * 14, x = 0, y = 0,
            z = z + abs(off) * 60, phase = i * 0.9, t = 0, shot = rnd() < 0.6, hit = 0 })
    end
    return z + 1100
  end
end

-- Autopilot for attract mode: chase rings and Hornets, steer around
-- towers, roll when a shot is close.
local function autopilot()
  local want_x, want_y, fire, roll = ship.x * 0.9, 70, false, false
  local best = 1e9
  for _, o in ipairs(objs) do
    local dz = o.z - (cam.z + SHIP_D)
    if dz > 60 and dz < 900 and (o.kind == "ring" or o.kind == "hornet") and dz < best then
      best = dz
      want_x, want_y = o.x, o.y
    end
    if o.kind == "hornet" and dz > 0 and dz < 1200 and abs(o.x - ship.x) < 30 and abs(o.y - ship.y) < 30 then
      fire = true
    end
    if o.kind == "eshot" and dz > 0 and dz < 160 then roll = true end
  end
  for _, o in ipairs(objs) do
    local dz = o.z - (cam.z + SHIP_D)
    if o.kind == "tower" and dz > 0 and dz < 300 and abs(o.x - want_x) < 45 then
      want_x = o.x + ((want_x >= o.x) and 50 or -50)
    end
  end
  local p = { left = false, right = false, up = false, down = false, i = fire, ii = roll }
  if want_x < ship.x - 8 then p.left = true elseif want_x > ship.x + 8 then p.right = true end
  if want_y < ship.y - 8 then p.down = true elseif want_y > ship.y + 8 then p.up = true end
  return p
end

-- Sound effects on channels 3 (lasers) and 6 (noise bursts).
local sfx_laser, sfx_boom, sfx_ding = 0, 0, 0

local function explode(x, y, z, big)
  add({ kind = "boom", x = x, y = y, z = z, t = 0, big = big })
  sfx_boom = big and 40 or 18
end

local function damage(n)
  if roll_t > 0 or hurt_t > 0 or not ship.alive then return end
  shield = shield - n
  hurt_t = 30
  sfx_boom = 12
  if shield <= 0 then
    shield = 0
    ship.alive = false
    explode(ship.x, ship.y, cam.z + SHIP_D, true)
    mode, mode_t = (mode == "attract") and "attract_dead" or "dead", 0
  end
end

local function update(input)
  -- Ship: accelerate, damp, clamp.
  local ax = (input.right and 1 or 0) - (input.left and 1 or 0)
  local ay = (input.up and 1 or 0) - (input.down and 1 or 0)
  if ship.alive then
    ship.vx = (ship.vx + ax * 0.9) * 0.88
    ship.vy = (ship.vy + ay * 0.7) * 0.88
    ship.x = clamp(ship.x + ship.vx, -190, 190)
    ship.y = clamp(ship.y + ship.vy, 14, 150)
    if input.ii and roll_t == 0 then roll_t = 28 end
    if roll_t > 0 then roll_t = roll_t - 1 end
    if hurt_t > 0 then hurt_t = hurt_t - 1 end
    fire_cd = max(0, fire_cd - 1)
    if input.i and fire_cd == 0 then
      fire_cd = 7
      for _, side in ipairs({ -11, 11 }) do
        lasers[#lasers + 1] = { x = ship.x + side, y = ship.y - 1, z = cam.z + SHIP_D + 12 }
      end
      sfx_laser = 8
    end
  end
  -- Camera: follows the ship part of the way; forward at a steady speed.
  cam.z = cam.z + SPEED
  cam.x = ship.x * 0.75
  cam.y = 36 + ship.y * 0.75
  local ship_z = cam.z + SHIP_D

  if cam.z + FAR > next_spawn then next_spawn = spawn(next_spawn) end

  -- Lasers.
  local kept = {}
  for _, l in ipairs(lasers) do
    local z0 = l.z
    l.z = l.z + SPEED + 28
    local hit = false
    for _, o in ipairs(objs) do
      if o.kind == "hornet" and not o.dead and abs(l.x - o.x) < 20 and abs(l.y - o.y) < 15
         and o.z >= z0 - 20 and o.z <= l.z + 20 then
        o.dead = true
        hit = true
        score = score + 100
        explode(o.x, o.y, o.z, false)
        break
      end
    end
    if not hit and l.z < cam.z + FAR then kept[#kept + 1] = l end
  end
  lasers = kept

  -- Objects.
  kept = {}
  for _, o in ipairs(objs) do
    local before = o.z - ship_z
    if o.kind == "hornet" then
      o.t = o.t + 1
      local dz = o.z - cam.z
      local peel = clamp((420 - dz) / 260, 0, 1)
      o.z = o.z - 3
      o.x = o.bx + sin(o.t * 0.05 + o.phase) * 40 + (o.bx >= 0 and 1 or -1) * peel * peel * 220
      o.y = o.by + cos(o.t * 0.04 + o.phase) * 14 + peel * peel * 120
      o.bank = cos(o.t * 0.05 + o.phase) + (o.bx >= 0 and 1 or -1) * peel * 2
      if o.shot and dz < 900 and ship.alive then
        o.shot = false
        local t = (o.z - ship_z) / (SPEED + 6)
        add({ kind = "eshot", x = o.x, y = o.y, z = o.z,
              vx = (ship.x - o.x) / t, vy = (ship.y - o.y) / t, vz = -6 })
      end
    elseif o.kind == "eshot" then
      o.x, o.y, o.z = o.x + o.vx, o.y + o.vy, o.z + o.vz
    elseif o.kind == "boom" then
      o.t = o.t + 1
      if o.t > 24 then o.dead = true end
    end
    local after = o.z - ship_z
    -- Passing the ship's plane: collide, collect or dodge.
    if before > 0 and after <= 0 and ship.alive and not o.dead then
      local dx, dy = abs(o.x - ship.x), abs(o.y - ship.y)
      if o.kind == "tower" and dx < 34 and ship.y < 100 then
        damage(16)
      elseif o.kind == "hornet" and dx < 26 and dy < 16 then
        damage(12); o.dead = true; explode(o.x, o.y, o.z, false)
      elseif o.kind == "eshot" and dx < 14 and dy < 11 then
        damage(8); o.dead = true
      elseif o.kind == "ring" and dx * dx + dy * dy < 30 * 30 then
        score = score + 200
        shield = min(SHIELD_MAX, shield + 10)
        o.got = true
        sfx_ding = 20
      end
    end
    if not o.dead and o.z > cam.z + 10 then kept[#kept + 1] = o end
  end
  objs = kept
end

-- Drawing ------------------------------------------------------------------

local P_SHIP          -- two 32x64 sprites, rewritten every frame
local buf
local was_used = {}   -- which 16x16 cells of the ship buffer had pixels

local list, nlist

local function put(depth, x, y, pattern, opts)
  nlist = nlist + 1
  list[nlist] = { depth, x, y, pattern, opts }
end

-- Queue a level of a sprite set. ax, ay: where the canvas's anchor point
-- (given in canvas pixels) lands on screen.
local function put_set(depth, lv, x, y, ax, ay, pal_, flip)
  local o = { pal = pal_, w = lv.sw, h = lv.sh, flip_x = flip }
  for _, part in ipairs(lv.parts) do
    local px = part[1]
    if flip then px = lv.w - part[1] - lv.sw end
    local fx = flip and (lv.w - ax) or ax
    put(depth, floor(x - fx + px), floor(y - ay + part[2]), part[3], o)
  end
end

-- Four mirrored quarters around a centre.
local function put_mirror(depth, lv, x, y, pal_)
  for q = 0, 3 do
    local fx, fy = q % 2 == 1, q >= 2
    local o = { pal = pal_, w = lv.sw, h = lv.sh, flip_x = fx, flip_y = fy }
    for _, part in ipairs(lv.parts) do
      local px = fx and (lv.w - part[1] - lv.sw) or part[1]
      local py = fy and (lv.h - part[2] - lv.sh) or part[2]
      local sx = fx and x + px or x - lv.w + px
      local sy = fy and y + py or y - lv.h + py
      put(depth, floor(sx), floor(sy), part[3], o)
    end
  end
end

local function project(x, y, z, hor)
  local dz = z - cam.z
  if dz < 1 then return nil end
  local k = F / dz
  return 128 + (x - cam.x) * k, hor - (y - cam.y) * k, k, dz
end

-- Fill the Lancer into the buffer and upload the cells that changed.
local function draw_ship(f)
  R3.clear(buf)
  local roll = -ship.vx * 0.09
  if roll_t > 0 then roll = roll + (28 - roll_t) / 28 * 2 * pi end
  local cycles = R3.draw(SHIP, buf, {
    yaw = ship.vx * 0.025, pitch = ship.vy * 0.035, roll = roll,
    scale = 1.25, dist = 160, view = 0.5, cx = 32, cy = 32,
  })
  local uploads = 0
  for s = 0, 1 do
    for cy = 0, 3 do
      for cx = 0, 1 do
        local id = s * 8 + cy * 2 + cx
        local used = false
        local bx, by = s * 32 + cx * 16, cy * 16
        for y = 0, 15 do
          local row = (by + y) * 64 + bx + 1
          for x = 0, 15 do
            if buf[row + x] ~= 0 then used = true; break end
          end
          if used then break end
        end
        if used or was_used[id] then
          local px = {}
          for y = 0, 15 do
            local row = (by + y) * 64 + bx + 1
            for x = 0, 15 do px[y * 16 + x + 1] = buf[row + x] end
          end
          pattern(P_SHIP + s * 8 + cx + cy * 2, px)
          uploads = uploads + 1
        end
        if was_used[id] then cycles = cycles + 128 * 6 end   -- clear that cell
        was_used[id] = used
      end
    end
  end
  cpu(cycles)
end

-- Per-line sprite units: drop the farthest sprites rather than let the
-- hardware drop random ones (what TG16 games did).
local units = {}

local function flush()
  -- nearest first: lower sprite numbers win overlaps
  for i = 2, nlist do
    local v = list[i]
    local j = i - 1
    while j >= 1 and list[j][1] > v[1] do
      list[j + 1] = list[j]
      j = j - 1
    end
    list[j + 1] = v
  end
  for y = 0, 223 do units[y] = 0 end
  local n = 0
  for i = 1, nlist do
    local e = list[i]
    local x, y, o = e[2], e[3], e[5]
    local w, h = o.w or 16, o.h or 16
    if n < 64 and x < 256 and x + w > 0 and y < 224 and y + h > 0 and x > -32 and y > -64 then
      local cost = w == 32 and 2 or 1
      local fits = true
      for ly = max(0, y), min(223, y + h - 1) do
        if units[ly] + cost > 16 then fits = false; break end
      end
      if fits then
        for ly = max(0, y), min(223, y + h - 1) do units[ly] = units[ly] + cost end
        spr(n, x, y, e[4], o)
        n = n + 1
      end
    end
  end
  cpu(nlist * 60)
end

local function draw(f, hor)
  list, nlist = {}, 0
  local ship_sx, ship_sy

  -- The ship and its shadow.
  if ship.alive and (hurt_t == 0 or f % 4 < 2) then
    draw_ship(f)
    local sx, sy = project(ship.x, ship.y, cam.z + SHIP_D, hor)
    ship_sx, ship_sy = sx, sy
    put(SHIP_D, floor(sx) - 32, floor(sy) - 32, P_SHIP, { pal = 0, w = 32, h = 64 })
    put(SHIP_D, floor(sx), floor(sy) - 32, P_SHIP + 8, { pal = 0, w = 32, h = 64 })
    if f % 2 == 0 then
      local gx, gy, k = project(ship.x, 0, cam.z + SHIP_D, hor)
      local li = R3.level(SET_SHADOW, 40 * k)
      if li then
        local lv = SET_SHADOW[li]
        put_set(SHIP_D + 1, lv, gx, gy, lv.w / 2, lv.h / 2, 7)
      end
    end
  end

  for _, o in ipairs(objs) do
    local sx, sy, k, dz = project(o.x, o.y, o.z, hor)
    if sx and dz < FAR + 50 then
      if o.kind == "hornet" then
        local bank = o.bank or 0
        local set = abs(bank) > 0.45 and SET_HORNET_BANK or SET_HORNET
        local li = R3.level(set, 48 * k)
        if li then
          local lv = set[li]
          put_set(dz, lv, sx, sy, lv.w / 2, lv.h / 2, 1, bank < 0)
        end
      elseif o.kind == "tower" then
        local li = R3.level(SET_TOWER, 40 * k)
        if li and 40 * k < SET_TOWER[#SET_TOWER].size * 1.4 then
          local lv = SET_TOWER[li]
          local flip = o.x > cam.x
          put_set(dz, lv, sx, sy, lv.size / 2, lv.h, 2, flip)
        end
      elseif o.kind == "ring" and not o.got then
        local li = R3.level(SET_RING, 70 * k)
        if li then put_mirror(dz, SET_RING[li], sx, sy, 3) end
      elseif o.kind == "eshot" then
        local li = R3.level(SET_BOLT, 10 * k)
        if li then put_set(dz, SET_BOLT[li], sx, sy, 8, 8, 5) end
      elseif o.kind == "boom" then
        local frame_ = min(4, o.t // 6 + 1)
        local big = o.big or 30 * k > 20
        local set = big and SET_BOOM32[frame_] or SET_BOOM16[frame_]
        local lv = set[1]
        if o.big then
          -- the ship's explosion: four bursts around it
          for q = 0, 3 do
            put_set(dz, lv, sx + (q % 2 * 2 - 1) * 12, sy + (q // 2 * 2 - 1) * 8, 16, 16, 6)
          end
        else
          put_set(dz, lv, sx, sy, lv.w / 2, lv.h / 2, 6)
        end
      end
    end
  end
  for _, l in ipairs(lasers) do
    local sx, sy, k, dz = project(l.x, l.y, l.z, hor)
    if sx then
      local li = R3.level(SET_BOLT, 8 * k)
      if li then put_set(dz, SET_BOLT[li], sx, sy, 8, 8, 4) end
    end
  end
  flush()
end

-- HUD --------------------------------------------------------------------

local function pad6(n)
  local s = "" .. floor(n)
  while #s < 6 do s = "0" .. s end
  return s
end

local function hud(f)
  text(1, HUD_ROW, "SCORE " .. pad6(score))
  text(17, HUD_ROW, "SHIELD")
  shield_bar(24, HUD_ROW, shield, SHIELD_MAX)
  local attract = mode == "attract" or mode == "attract_dead"
  if attract then
    text(10, TITLE_ROW, "COMET LANCER")
    text(10, TITLE_ROW + 2, (f // 30 % 2 == 0) and " PRESS RUN  " or "            ")
  elseif mode == "dead" and mode_t > 40 then
    text(10, TITLE_ROW, "  GAME OVER ")
    text(10, TITLE_ROW + 2, "            ")
  else
    text(10, TITLE_ROW, "            ")
    text(10, TITLE_ROW + 2, "            ")
  end
end

-- Music ------------------------------------------------------------------

local LEAD = { "D5", "F5", "A5", "D6", "C6", "A5", "G5", "A5",
               "A#4", "D5", "F5", "A#5", "A5", "F5", "E5", "C5" }
local BASS = { "D2", "D2", "A#1", "A#1", "F2", "F2", "C2", "A2" }

local function sound(f)
  local step = f // 7
  psg(1, { on = f % 7 < 5, note = LEAD[step % 16 + 1] })
  psg(2, { on = f % 14 < 12, note = BASS[(step // 4) % 8 + 1] })
  -- drums on 5: low thump on beats, tick between
  if f % 14 == 0 then psg(5, { on = true, noise = (step // 2) % 2 == 0 and 22 or 30, vol = 18 })
  elseif f % 14 == 3 then psg(5, { on = false }) end
  -- lasers: a falling square blip on 3
  if sfx_laser > 0 then
    psg(3, { on = true, hz = 1400 - (8 - sfx_laser) * 150, vol = 22 })
    sfx_laser = sfx_laser - 1
  else
    psg(3, { on = false })
  end
  -- ring chime on 4
  if sfx_ding > 0 then
    psg(4, { on = true, note = (sfx_ding > 10) and "E6" or "B6", vol = 10 + sfx_ding // 2 })
    sfx_ding = sfx_ding - 1
  else
    psg(4, { on = false })
  end
  -- explosions: noise on 6, fading
  if sfx_boom > 0 then
    psg(6, { on = true, noise = 8 + sfx_boom % 4, vol = min(31, 10 + sfx_boom) })
    sfx_boom = sfx_boom - 1
  else
    psg(6, { on = false })
  end
end

-- Hooks ------------------------------------------------------------------

function init()
  build_world()
  palettes()
  R3.patterns_from((tiles_used() * 16 + 63) // 64)
  P_SHIP = R3.alloc(32, 64)
  R3.alloc(32, 64)
  buf = R3.buffer(64, 64)
  build_sets()
  reset("attract")

  wave(1, "pulse25")
  wave(2, "triangle")
  wave(3, "square")
  wave(4, "sine")
  psg(1, { vol = 20, left = 15, right = 12 })
  psg(2, { vol = 26 })
  psg(5, { vol = 18 })
  psg(6, { noise = 10 })
end

function frame(t, f)
  local any = pad.up or pad.down or pad.left or pad.right or pad.i or pad.ii or pad.run
  local pressed = any and not last_buttons
  last_buttons = any
  mode_t = mode_t + 1

  if (mode == "attract" or mode == "attract_dead") and pressed and pad.run then
    reset("play")
  elseif mode == "attract_dead" and mode_t > 90 then
    reset("attract")
  elseif mode == "dead" and mode_t > 60 and pressed then
    reset("play")
  elseif mode == "dead" and mode_t > 600 then
    reset("attract")
  end

  local input = pad
  if mode == "attract" or mode == "attract_dead" then input = autopilot() end
  update(input)
  cpu(3000 + 120 * #objs)

  local hor = clamp(HOR_BASE + floor(ship.vy * 2), 72, 112)
  local attract = mode ~= "play"
  world_frame(cam, hor, attract and 56 or 16)
  hud(f)
  draw(f, hor)
  sound(f)
end
