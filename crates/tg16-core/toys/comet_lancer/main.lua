-- Comet Lancer: an on-rails 3D shooter for the TurboGrafx-16 model.
-- See README.md in this directory for how it works.
--
-- Fly through rings (shield back), dodge towers and shoot down Hornets.
-- Controls (the viewer's keys in brackets):
--   d-pad   steer, up climbs (arrows)
--   I       fire twin lasers, hold for auto fire (X)
--   II      barrel roll: enemy shots miss while rolling; uses the ROLL meter (Z)
--   SELECT  nova bomb: clears the sky of Hornets and shots, 3 per game (Shift)
--   RUN     start (Enter); until then the ship flies itself
-- After a game over, any button plays again.
--
-- What is 3D and how (the TG16 has no polygon, scaling or rotation hardware):
--   * The Lancer is real 3D: 17 flat-shaded polygons filled by the CPU into
--     a 64x64 buffer every frame and sent to VRAM as two 32x64 sprites.
--     cpu() charges the fill time to the budget meter.
--   * Hornets, towers, rings, bolts, specks, clouds and explosions are
--     sprites picked from sets pre-scaled in init(), placed with a
--     perspective divide and turned with the horizon.
--   * Sky and ground are one background layer: every line picks a map row
--     and a scroll so the horizon tilts with the ship (see 30_world.lua).

HOR_BASE = 78     -- horizon line at the screen centre when flying level
SHIP_D = 90       -- the ship flies this far ahead of the camera
SPEED = 8         -- world units per frame
FAR = 1700        -- objects appear this far ahead
SHIELD_MAX = 64
ROLL_MAX = 64
TILT = 0.5        -- how much of the ship's bank the horizon follows

-- Game state -------------------------------------------------------------

local ship, cam, objs, lasers, specks
local mode, mode_t, score, shield, roll_energy, bombs = "attract", 0, 0, SHIELD_MAX, ROLL_MAX, 3
local next_spawn, wave_i, fire_cd, roll_t, hurt_t, flash_t
local last_buttons, last_select = false, false
local tilt, hor = 0, HOR_BASE
local seed = 1

local function rnd()
  seed = (seed * 1103515245 + 12345) % 2147483648
  return seed / 2147483648
end

-- Co-pilot messages in the info bar, typed out a letter at a time.
local msg, msg_t, msg_hold = "", 0, 0
local function say(s, hold)
  if s ~= msg or msg_t > #s * 2 + 30 then
    msg, msg_t, msg_hold = s, 0, hold or 120
  end
end

local function reset(new_mode)
  mode, mode_t = new_mode, 0
  ship = { x = 0, y = 60, vx = 0, vy = 0, roll = 0, alive = true }
  cam = { x = 0, y = 70, z = 0 }
  objs, lasers = {}, {}
  next_spawn, wave_i, fire_cd, roll_t, hurt_t, flash_t = 600, 0, 0, 0, 0, 0
  score, shield, roll_energy, bombs = 0, SHIELD_MAX, ROLL_MAX, 3
  seed = 7
  specks = {}
  for i = 1, 14 do
    specks[i] = { x = (rnd() - 0.5) * 900, z = rnd() * 1200 }
  end
  if new_mode == "play" then say("ROVY HERE. LET'S GO!", 120) end
end

local function add(o) objs[#objs + 1] = o; return o end

-- The course: a repeating list of encounters, placed by distance.
local COURSE = { "towers", "ring", "wave", "towers", "rings", "wave", "slalom", "wave2" }
local SAYINGS = {
  towers = "WATCH THOSE TOWERS!", ring = "RING AHEAD. FLY THROUGH!",
  rings = "MORE RINGS. GRAB THEM!", wave = "HORNETS INBOUND!",
  slalom = "TIGHT SQUEEZE. STEADY...", wave2 = "BIG WAVE. LOOK OUT!",
}

local function spawn(z)
  local kind = COURSE[wave_i % #COURSE + 1]
  wave_i = wave_i + 1
  if mode == "play" then say(SAYINGS[kind], 120) end
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
            z = z + abs(off) * 60, phase = i * 0.9, t = 0, shot = rnd() < 0.6 })
    end
    return z + 1100
  end
end

-- Autopilot for attract mode: chase rings and Hornets, steer around
-- towers, roll when a shot is close.
local function autopilot()
  -- with nothing to chase, swoop from side to side (it shows off the
  -- tilting horizon)
  local want_x = sin(cam.z / 700) * 150
  local want_y, fire, roll = 70 + sin(cam.z / 1100) * 30, false, false
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
  local p = { i = fire, ii = roll }
  p.left, p.right = want_x < ship.x - 8, want_x > ship.x + 8
  p.down, p.up = want_y < ship.y - 8, want_y > ship.y + 8
  return p
end

-- Sound effects on channels 3 (lasers), 4 (chimes) and 6 (noise bursts).
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
    if mode == "dead" then say("NOOO! ROVY TO BASE...", 600) end
  elseif mode == "play" then
    say(shield < 20 and "SHIELD LOW! FIND A RING!" or "WE'RE HIT!", 120)
  end
end

local function nova()
  bombs = bombs - 1
  flash_t = 12
  sfx_boom = 40
  local n = 0
  for _, o in ipairs(objs) do
    if (o.kind == "hornet" or o.kind == "eshot") and not o.dead and o.z - cam.z < FAR then
      o.dead = true
      if o.kind == "hornet" then
        n = n + 1
        explode(o.x, o.y, o.z, false)
      end
    end
  end
  score = score + n * 100
  -- (built in a local first: piccolo mis-compiles a concatenation with a
  -- variable in the middle when it is written straight into call arguments)
  local label = "NOVA BOMB! " .. n .. " DOWN!"
  if mode == "play" then say(label, 120) end
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
    if input.ii and roll_t == 0 and roll_energy >= 32 then
      roll_t = 28
      roll_energy = roll_energy - 32
    end
    roll_energy = min(ROLL_MAX, roll_energy + 0.25)
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
    if input.select and not last_select and bombs > 0 then nova() end
  end
  last_select = input.select
  if flash_t > 0 then flash_t = flash_t - 1 end

  -- Camera: follows the ship part of the way; forward at a steady speed.
  cam.z = cam.z + SPEED
  cam.x = ship.x * 0.75
  cam.y = 36 + ship.y * 0.75
  local ship_z = cam.z + SHIP_D

  if cam.z + FAR > next_spawn then next_spawn = spawn(next_spawn) end
  for _, s in ipairs(specks) do
    if s.z < cam.z + 20 then
      s.z = s.z + 1200
      s.x = cam.x + (rnd() - 0.5) * 900
    end
  end

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
        if mode == "play" and rnd() < 0.4 then say("NICE SHOT!", 60) end
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
        if roll_t > 0 and mode == "play" then say("DEFLECTED! NICE ROLL!", 60) end
        damage(8); o.dead = true
      elseif o.kind == "ring" and dx * dx + dy * dy < 30 * 30 then
        score = score + 200
        shield = min(SHIELD_MAX, shield + 10)
        o.got = true
        sfx_ding = 20
        if mode == "play" then say("GOT IT! SHIELD UP.", 60) end
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
local tc, ts = 1, 0   -- cos and sin of the horizon's tilt

local function put(depth, x, y, pattern, opts)
  nlist = nlist + 1
  list[nlist] = { depth, x, y, pattern, opts }
end

-- Queue a level of a sprite set. ax, ay: the canvas pixel that lands on
-- screen point x, y.
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

-- World point -> screen, turned with the horizon around the screen centre.
local function project(x, y, z)
  local dz = z - cam.z
  if dz < 1 then return nil end
  local k = F / dz
  local ux, uy = (x - cam.x) * k, -(y - cam.y) * k
  return 128 + ux * tc - uy * ts, hor + ux * ts + uy * tc, k, dz
end

-- Fill the Lancer into the buffer and upload the cells that changed.
local function draw_ship()
  R3.clear(buf)
  local roll = -ship.vx * 0.09
  if roll_t > 0 then roll = roll + (28 - roll_t) / 28 * 2 * pi end
  local cycles = R3.draw(SHIP, buf, {
    yaw = ship.vx * 0.025, pitch = ship.vy * 0.035, roll = roll - tilt * 0.5,
    scale = 1.25, dist = 160, view = 0.5, cx = 32, cy = 32,
  })
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
  for y = 0, VIEW_H - 1 do units[y] = 0 end
  local n = 0
  for i = 1, nlist do
    local e = list[i]
    local x, y, o = e[2], e[3], e[5]
    local w, h = o.w or 16, o.h or 16
    if n < 64 and x < 256 and x + w > 0 and y < VIEW_H and y + h > 0 and x > -32 and y > -64 then
      local cost = w == 32 and 2 or 1
      local fits = true
      for ly = max(0, y), min(VIEW_H - 1, y + h - 1) do
        if units[ly] + cost > 16 then fits = false; break end
      end
      if fits then
        for ly = max(0, y), min(VIEW_H - 1, y + h - 1) do units[ly] = units[ly] + cost end
        spr(n, x, y, e[4], o)
        n = n + 1
      end
    end
  end
  cpu(nlist * 60)
end

local CLOUDS = { { -0.9, 0.42 }, { -0.25, 0.55 }, { 0.5, 0.38 }, { 1.2, 0.5 } }

local function draw(f)
  list, nlist = {}, 0

  -- The ship and its dithered shadow.
  if ship.alive and (hurt_t == 0 or f % 4 < 2) then
    draw_ship()
    local sx, sy = project(ship.x, ship.y, cam.z + SHIP_D)
    put(SHIP_D, floor(sx) - 32, floor(sy) - 32, P_SHIP, { pal = 0, w = 32, h = 64 })
    put(SHIP_D, floor(sx), floor(sy) - 32, P_SHIP + 8, { pal = 0, w = 32, h = 64 })
    local gx, gy, k = project(ship.x, 0, cam.z + SHIP_D)
    local li = R3.level(SET_SHADOW, 40 * k)
    if li then
      local lv = SET_SHADOW[li]
      put_set(SHIP_D + 1, lv, gx, gy, lv.w / 2, lv.h / 2, 7, false)
    end
  end

  -- Clouds: far away, so they only slide sideways and turn with the sky.
  for i, c in ipairs(CLOUDS) do
    local ux = ((c[1] * 300 - cam.x * 0.05 - f * 0.05) + 450) % 900 - 450
    local uy = -c[2] * 120 - 16
    local x, y = 128 + ux * tc - uy * ts, hor + ux * ts + uy * tc
    local lv = SET_CLOUD[(i % 2) + 1]
    put_set(5000 + i, lv, x, y, lv.w / 2, lv.h / 2, 10, false)
  end

  for _, s in ipairs(specks) do
    local sx, sy, k, dz = project(s.x, 0, s.z)
    if sx then
      local li = R3.level(SET_SPECK, 2.2 * k * 1.5)
      if li then put_set(dz + 2, SET_SPECK[li], sx, sy, 8, 8, 9, false) end
    end
  end

  for _, o in ipairs(objs) do
    local sx, sy, k, dz = project(o.x, o.y, o.z)
    if sx and dz < FAR + 50 then
      if o.kind == "hornet" then
        local bank = o.bank or 0
        local set = (abs(bank) > 0.45 and 48 * k < 34) and SET_HORNET_BANK or SET_HORNET
        local li = R3.level(set, 48 * k)
        if li then
          local lv = set[li]
          put_set(dz, lv, sx, sy, lv.w / 2, lv.h / 2, 1, bank < 0)
        end
      elseif o.kind == "tower" then
        local li = R3.level(SET_TOWER, 40 * k)
        if li and 40 * k < SET_TOWER[#SET_TOWER].size * 1.4 then
          local lv = SET_TOWER[li]
          put_set(dz, lv, sx, sy, lv.size / 2, lv.h, 2, o.x > cam.x)
        end
      elseif o.kind == "ring" and not o.got then
        local li = R3.level(SET_RING, 70 * k)
        if li then put_mirror(dz, SET_RING[li], sx, sy, 3) end
      elseif o.kind == "eshot" then
        local li = R3.level(SET_BOLT, 10 * k)
        if li then put_set(dz, SET_BOLT[li], sx, sy, 8, 8, 5, false) end
      elseif o.kind == "boom" then
        local frame_ = min(4, o.t // 6 + 1)
        local big = o.big or 30 * k > 20
        local lv = (big and SET_BOOM32[frame_] or SET_BOOM16[frame_])[1]
        if o.big then
          for q = 0, 3 do
            put_set(dz, lv, sx + (q % 2 * 2 - 1) * 12, sy + (q // 2 * 2 - 1) * 8, 16, 16, 6, false)
          end
        else
          put_set(dz, lv, sx, sy, lv.w / 2, lv.h / 2, 6, false)
        end
      end
    end
  end
  for _, l in ipairs(lasers) do
    local sx, sy, k, dz = project(l.x, l.y, l.z)
    if sx then
      local li = R3.level(SET_BOLT, 8 * k)
      if li then put_set(dz, SET_BOLT[li], sx, sy, 8, 8, 4, false) end
    end
  end
  flush()
end

-- Info bar -----------------------------------------------------------------
-- Map rows 0..3, shown on lines 192..223:
--   portrait (cols 1-4) | SHIELD + bar          | SCORE + digits
--                       | message line          | ROLL meter + bombs

local function pad6(n)
  local s = "" .. floor(n)
  while #s < 6 do s = "0" .. s end
  return s
end

local shield_color = -1

local function info_bar(f)
  local row = BAR_ROW
  text(6, row + 1, "SHIELD", 0)
  meter(6, row + 2, shield, SHIELD_MAX, 0)
  text(19, row + 1, "SCORE " .. pad6(score), 0)
  text(19, row + 2, "ROLL", 0)
  meter(23, row + 2, roll_energy, ROLL_MAX, 4)
  bomb_icons(15, row + 2, bombs)
  -- The shield bar goes yellow, then red.
  local sc = shield > 32 and 0 or (shield > 16 and 1 or 2)
  if sc ~= shield_color then
    shield_color = sc
    pal(0, 2, ({ rgb(90, 230, 120), rgb(240, 210, 60), rgb(240, 60, 50) })[sc + 1])
  end

  local attract = mode == "attract" or mode == "attract_dead"
  if attract then
    if f % 240 < 120 then say("COMET LANCER  PRESS RUN", 1000)
    else say("ARROWS, X FIRE, Z ROLL", 1000) end
  elseif mode == "dead" and mode_t > 60 then
    say("GAME OVER. ANY BUTTON!", 1000)
  end
  msg_t = msg_t + 1
  local n = min(#msg, msg_t // 2)
  local line = string.sub(msg, 1, n)
  if msg_t > #msg * 2 + msg_hold and not attract then line = "" end
  while #line < 26 do line = line .. " " end
  text(6, row + 3, line, 0)
  show_portrait(1, row, n < #msg and (msg_t // 4) % 2 == 0)
end

-- Music --------------------------------------------------------------------

local LEAD = { "D5", "F5", "A5", "D6", "C6", "A5", "G5", "A5",
               "A#4", "D5", "F5", "A#5", "A5", "F5", "E5", "C5" }
local BASS = { "D2", "D2", "A#1", "A#1", "F2", "F2", "C2", "A2" }

local function sound(f)
  local step = f // 7
  psg(1, { on = f % 7 < 5, note = LEAD[step % 16 + 1] })
  psg(2, { on = f % 14 < 12, note = BASS[(step // 4) % 8 + 1] })
  if f % 14 == 0 then psg(5, { on = true, noise = (step // 2) % 2 == 0 and 22 or 30, vol = 18 })
  elseif f % 14 == 3 then psg(5, { on = false }) end
  if sfx_laser > 0 then
    psg(3, { on = true, hz = 1400 - (8 - sfx_laser) * 150, vol = 22 })
    sfx_laser = sfx_laser - 1
  else
    psg(3, { on = false })
  end
  if sfx_ding > 0 then
    psg(4, { on = true, note = (sfx_ding > 10) and "E6" or "B6", vol = 10 + sfx_ding // 2 })
    sfx_ding = sfx_ding - 1
  else
    psg(4, { on = false })
  end
  if sfx_boom > 0 then
    psg(6, { on = true, noise = 8 + sfx_boom % 4, vol = min(31, 10 + sfx_boom) })
    sfx_boom = sfx_boom - 1
  else
    psg(6, { on = false })
  end
end

-- Hooks --------------------------------------------------------------------

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

local was_flashing = false

function frame(t, f)
  local any = pad.up or pad.down or pad.left or pad.right or pad.i or pad.ii or pad.run or pad.select
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
  if mode == "attract" or mode == "attract_dead" then
    input = autopilot()
    if mode_t % 600 == 400 and bombs > 0 then input.select = true end
  end
  update(input)
  cpu(3000 + 120 * #objs)

  -- The horizon follows the ship's bank (not its barrel rolls) and pitch.
  local want = clamp(-ship.vx * 0.09 * TILT, -0.38, 0.38)
  tilt = tilt + (want - tilt) * 0.25
  tc, ts = cos(tilt), sin(tilt)
  hor = clamp(HOR_BASE + floor(ship.vy * 2), 58, 100)

  if flash_t > 0 then
    world_palettes(flash_t / 12)
    was_flashing = true
  elseif was_flashing then
    world_palettes(0)
    was_flashing = false
  end
  world_frame(hor, tilt)
  info_bar(f)
  draw(f)
  sound(f)
end
