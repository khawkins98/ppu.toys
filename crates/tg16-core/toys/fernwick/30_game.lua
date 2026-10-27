-- 30_game.lua: Fernwick's game logic, shared unchanged by the TG16 and SNES
-- toys (build_art.py copies it; a test checks the copies match). Pure Lua:
-- no machine calls. Each machine's main.lua reads the state in G, draws it
-- its own way, and carries out the work this file asks for:
--
--   G.jobs     map writes: { "room", id, tx, ty } draws a room at map tile
--              (tx, ty) of the 64x64-tile map; { "cell", tx, ty, ch } one
--              block. The machine takes them in order, as fast as its VRAM
--              budget allows, and removes them; the game waits for them.
--   G.cam_x/y  map pixel shown at the playfield's top-left (lines 32..223)
--   G.sfx      sound effects started this frame (names)
--   G.hud_dirty, G.msg, G.iris, G.flash, G.dark ...
--
-- Map layout (tiles): the current room lives at (0, 0); a scroll transition
-- draws the next room at (32, 0) (left/right) or copies the current room to
-- (32, 0) and the next to (32, 24) / (32, 40) (down/up), scrolls, then
-- redraws the new room at home. The TG16 keeps its HUD at tiles (0, 24).

local floor, abs, min, max, sqrt = math.floor, math.abs, math.min, math.max, math.sqrt
local sin = math.sin

G = {}

-- Deterministic random numbers (an LCG), reseeded for every game.
local seed = 1
local function rnd(n)
  seed = (seed * 1103515245 + 12345) % 2147483648
  return floor(seed / 2147483648 * n)
end

local DIRS = { down = { 0, 1 }, up = { 0, -1 }, left = { -1, 0 }, right = { 1, 0 } }
local DIR_LIST = { "down", "up", "left", "right" }

-- Room cells --------------------------------------------------------------

local cells = {}       -- cells[room][row][col] = char (mutable: bushes, door)

local function load_cells()
  for id, room in ipairs(W.rooms) do
    local rows = {}
    for r = 0, 11 do
      local s = room.rows[r + 1]
      local row = {}
      for c = 0, 15 do row[c] = string.sub(s, c + 1, c + 1) end
      rows[r] = row
    end
    cells[id] = rows
  end
end

function room_cell(id, col, row)
  return cells[id][row][col]
end

local function neighbour(id, dx, dy)
  if id == W.CAVE then return nil end
  local gx, gy = (id - 1) % W.GRID_W + dx, (id - 1) // W.GRID_W + dy
  if gx < 0 or gy < 0 or gx >= W.GRID_W or gy >= W.GRID_H then return nil end
  return 1 + gx + gy * W.GRID_W
end

-- Is map pixel (x, y) of the current room solid? Outside the room it asks
-- the neighbouring room (so exits are wherever both sides are open).
local function solid_at(x, y, walls_only)
  local id = G.room
  local col, row = floor(x / 16), floor(y / 16)
  if col < 0 or col > 15 or row < 0 or row > 11 then
    if walls_only then return true end
    local dx = col < 0 and -1 or (col > 15 and 1 or 0)
    local dy = row < 0 and -1 or (row > 11 and 1 or 0)
    id = neighbour(id, dx, dy)
    if not id then return true end
    col, row = col % 16, row % 12
  end
  return W.SOLID[cells[id][row][col]] == true
end

local function box_solid(x, y, w, h, walls_only)
  G.work = G.work + 1
  return solid_at(x, y, walls_only) or solid_at(x + w - 1, y, walls_only)
      or solid_at(x, y + h - 1, walls_only) or solid_at(x + w - 1, y + h - 1, walls_only)
end

local function overlap(ax, ay, aw, ah, bx, by, bw, bh)
  return ax < bx + bw and bx < ax + aw and ay < by + bh and by < ay + ah
end

-- The hero's feet: what collides with walls.
local HB = { 3, 8, 10, 8 }

-- Entities -----------------------------------------------------------------

local function spawn_foes()
  G.foes = {}
  for _, f in ipairs(W.rooms[G.room].foes) do
    local def = W.FOES[f[1]]
    G.foes[#G.foes + 1] = { kind = f[1], x = f[2] * 16, y = f[3] * 16, hp = def.hp,
      dir = DIR_LIST[rnd(4) + 1], t = 20 + rnd(40), inv = 0, kb = 0, kdx = 0, kdy = 0,
      vx = 0, vy = 0, phase = rnd(100) }
  end
  G.items, G.fx = {}, {}
  if G.room == W.KEY.room and not G.has_key and not G.key_used then
    G.items[1] = { kind = "key", x = W.KEY.col * 16, y = W.KEY.row * 16 }
  end
  if G.room == W.STONE.room then
    G.items[#G.items + 1] = { kind = "sunstone", x = W.STONE.col * 16, y = W.STONE.row * 16 }
  end
end

local function sfx(name) G.sfx[#G.sfx + 1] = name end

local function play_song(name)
  if G.song ~= name then
    G.song, G.song_step, G.song_sub = name, 0, 0
  end
end

local function room_song()
  return G.room == W.CAVE and "cave" or "overworld"
end

local function enter_room(id)
  G.room = id
  G.ap_t = 0
  G.jobs[#G.jobs + 1] = { "room", id, 0, 0 }
  G.dark = W.rooms[id].dark == true
  spawn_foes()
  play_song(room_song())
  G.hud_dirty = true
end

function new_game(demo)
  seed = 20261027
  load_cells()
  G.mode, G.mt = "play", 0
  G.demo = demo or false
  G.demo_t = 0
  G.hero = { x = W.START.x, y = W.START.y, dir = "up", step = 0, atk = 0, inv = 0,
             kb = 0, kdx = 0, kdy = 0, moving = false }
  G.hp, G.hp_max, G.gems, G.kills = 6, 6, 0, 0
  G.has_key, G.key_used, G.door_open = false, false, false
  G.cam_x, G.cam_y = 0, 0
  G.iris, G.flash = nil, 0
  G.msg = nil
  G.jobs = {}
  enter_room(W.START.room)
end

function game_init()
  G.sfx, G.jobs, G.work = {}, {}, 0
  G.frame = 0
  load_cells()
  G.room = W.START.room
  G.foes, G.items, G.fx = {}, {}, {}
  G.hero = { x = W.START.x, y = W.START.y, dir = "down", step = 0, atk = 0, inv = 0,
             kb = 0, kdx = 0, kdy = 0 }
  G.hp, G.hp_max, G.gems, G.kills = 6, 6, 0, 0
  G.cam_x, G.cam_y = 0, 0
  G.mode, G.mt, G.idle = "title", 0, 0
  G.song, G.song_step, G.song_sub = nil, 0, 0
  G.hud_dirty = true
  G.flash = 0
  play_song("title")
end

-- Hero ----------------------------------------------------------------------

-- Move the hero by (dx, dy) on one axis, sliding round corners like the
-- 16-bit adventure games do: a blocked move tries a nudge sideways.
local function hero_move(dx, dy)
  local h = G.hero
  local x, y = h.x + dx, h.y + dy
  if not box_solid(x + HB[1], y + HB[2], HB[3], HB[4]) then
    h.x, h.y = x, y
    return true
  end
  for o = 1, 6 do
    for _, s in ipairs({ -1, 1 }) do
      local ox, oy = (dx == 0) and o * s or 0, (dy == 0) and o * s or 0
      if not box_solid(x + ox + HB[1], y + oy + HB[2], HB[3], HB[4]) then
        local nx, ny = h.x + ((dx == 0) and s or 0), h.y + ((dy == 0) and s or 0)
        if not box_solid(nx + HB[1], ny + HB[2], HB[3], HB[4]) then h.x, h.y = nx, ny end
        return false
      end
    end
  end
  return false
end

local function hurt_hero(dmg, from_x, from_y)
  local h = G.hero
  if h.inv > 0 or G.mode ~= "play" then return end
  G.hp = max(0, G.hp - dmg)
  h.inv, h.kb = 48, 8
  local dx, dy = h.x - from_x, h.y - from_y
  if abs(dx) > abs(dy) then h.kdx, h.kdy = (dx > 0) and 3 or -3, 0
  else h.kdx, h.kdy = 0, (dy > 0) and 3 or -3 end
  G.flash = 8
  G.hud_dirty = true
  sfx("hurt")
  if G.hp == 0 then
    G.mode, G.mt = "dying", 0
    G.song = nil
    sfx("die")
  end
end

-- Sword reach for the current swing, or nil outside its active frames.
function sword_box()
  local h = G.hero
  if h.atk < 3 or h.atk > 11 then return nil end
  local d = h.dir
  if d == "down" then return h.x + 4, h.y + 14, 8, 14 end
  if d == "up" then return h.x + 2, h.y - 12, 8, 16 end
  if d == "right" then return h.x + 13, h.y + 5, 15, 8 end
  return h.x - 12, h.y + 5, 15, 8
end

local function drop_item(x, y)
  local r = rnd(100)
  if r < 40 then
    G.items[#G.items + 1] = { kind = "gem", x = x, y = y, t = 480 }
  elseif r < 55 then
    G.items[#G.items + 1] = { kind = "heart", x = x, y = y, t = 480 }
  end
end

local function cut_bushes(bx, by, bw, bh)
  for _, p in ipairs({ { bx + bw // 2, by + bh // 2 }, { bx + 2, by + 2 }, { bx + bw - 3, by + bh - 3 } }) do
    local col, row = floor(p[1] / 16), floor(p[2] / 16)
    if col >= 0 and col <= 15 and row >= 0 and row <= 11 and cells[G.room][row][col] == "b" then
      cells[G.room][row][col] = ","
      G.jobs[#G.jobs + 1] = { "cell", col * 2, row * 2, "," }
      G.fx[#G.fx + 1] = { kind = "leaf", x = col * 16, y = row * 16, t = 0 }
      sfx("cut")
      if rnd(100) < 35 then
        G.items[#G.items + 1] = { kind = "gem", x = col * 16, y = row * 16, t = 480 }
      end
    end
  end
end

local function update_hero(input)
  local h = G.hero
  if h.inv > 0 then h.inv = h.inv - 1 end
  if h.kb > 0 then
    h.kb = h.kb - 1
    if h.kdx ~= 0 then hero_move(h.kdx, 0) else hero_move(0, h.kdy) end
    return
  end
  if h.atk > 0 then
    h.atk = h.atk + 1
    if h.atk > 15 then h.atk = 0 end
  elseif input.attack_pressed then
    h.atk = 1
    sfx("sword")
  end
  h.moving = false
  if h.atk == 0 then
    local dx = (input.right and 1 or 0) - (input.left and 1 or 0)
    local dy = (input.down and 1 or 0) - (input.up and 1 or 0)
    if dx ~= 0 or dy ~= 0 then
      -- face the newly pressed direction; keep facing on diagonals
      local fx = dx > 0 and "right" or (dx < 0 and "left" or nil)
      local fy = dy > 0 and "down" or (dy < 0 and "up" or nil)
      if not ((fx and h.dir == fx) or (fy and h.dir == fy)) then h.dir = fy or fx end
      local sp = (dx ~= 0 and dy ~= 0) and 1 or 1.5
      if dx ~= 0 then hero_move(dx * sp, 0) end
      if dy ~= 0 then hero_move(0, dy * sp) end
      h.moving = true
      h.step = h.step + 1
    end
  end
  -- Door: walk into it holding the key.
  if G.room == W.DOOR.room and not G.door_open and G.has_key and h.dir == "up" then
    local col, row = floor((h.x + 8) / 16), floor((h.y + HB[2] - 2) / 16)
    if col == W.DOOR.col and row == W.DOOR.row then
      G.door_open, G.has_key, G.key_used = true, false, true
      cells[G.room][row][col] = "+"
      G.jobs[#G.jobs + 1] = { "cell", col * 2, row * 2, "+" }
      G.hud_dirty = true
      sfx("door")
    end
  end
end

-- Enemies -------------------------------------------------------------------

local function foe_move(e, dx, dy)
  local x, y = e.x + dx, e.y + dy
  if e.kind == "wisp" then
    e.x, e.y = min(max(x, 0), 240), min(max(y, 0), 176)
    return true
  end
  if box_solid(x + 2, y + 6, 12, 10, true) then return false end
  e.x, e.y = x, y
  return true
end

local function update_foe(e)
  local h = G.hero
  local def = W.FOES[e.kind]
  e.phase = e.phase + 1
  if e.inv > 0 then e.inv = e.inv - 1 end
  if e.kb > 0 then
    e.kb = e.kb - 1
    foe_move(e, e.kdx, 0)
    foe_move(e, 0, e.kdy)
    return
  end
  if e.kind == "blob" then
    e.t = e.t - 1
    if e.t <= 0 then
      local r = rnd(6)
      e.dir = r < 4 and DIR_LIST[r + 1] or nil
      e.t = 24 + rnd(48)
    end
    if e.dir and e.phase % 2 == 0 then
      local d = DIRS[e.dir]
      if not foe_move(e, d[1], d[2]) then e.t = 0 end
    end
  elseif e.kind == "beetle" then
    if e.charge then
      local d = DIRS[e.dir]
      if not foe_move(e, d[1] * 2.5, d[2] * 2.5) then
        e.charge, e.t = false, 30
      end
    else
      e.t = e.t - 1
      if e.t <= 0 then e.dir, e.t = DIR_LIST[rnd(4) + 1], 40 + rnd(60) end
      if e.t < 200 then
        local d = DIRS[e.dir]
        if not foe_move(e, d[1] * def.speed, d[2] * def.speed) then e.t = 0 end
      end
      -- charge when the hero is lined up in front
      local ddx, ddy = h.x - e.x, h.y - e.y
      local d = DIRS[e.dir]
      if (abs(ddx) < 8 and d[2] ~= 0 and ddy * d[2] > 0 and abs(ddy) < 112)
        or (abs(ddy) < 8 and d[1] ~= 0 and ddx * d[1] > 0 and abs(ddx) < 112) then
        e.charge = true
      end
    end
  else -- wisp: drifts towards the hero, through walls, bobbing
    local tx, ty = h.x - e.x, h.y - e.y
    local len = sqrt(tx * tx + ty * ty) + 0.001
    e.vx = e.vx * 0.96 + tx / len * 0.03
    e.vy = e.vy * 0.96 + ty / len * 0.03
    local sp = sqrt(e.vx * e.vx + e.vy * e.vy)
    if sp > def.speed then e.vx, e.vy = e.vx / sp * def.speed, e.vy / sp * def.speed end
    foe_move(e, e.vx + sin(e.phase * 0.07) * 0.4, e.vy + sin(e.phase * 0.05) * 0.3)
  end
end

local function update_foes()
  local h = G.hero
  local sx, sy, sw, sh = sword_box()
  if sx then cut_bushes(sx, sy, sw, sh) end
  local kept = {}
  for _, e in ipairs(G.foes) do
    update_foe(e)
    G.work = G.work + 4
    local dead = false
    if sx and e.inv == 0 and overlap(sx, sy, sw, sh, e.x + 1, e.y + 2, 14, 13) then
      e.hp = e.hp - 1
      e.inv, e.kb = 16, 8
      local d = DIRS[h.dir]
      e.kdx, e.kdy = d[1] * 3, d[2] * 3
      e.charge = false
      G.fx[#G.fx + 1] = { kind = "spark", x = e.x, y = e.y, t = 0 }
      if e.hp <= 0 then
        dead = true
        G.kills = G.kills + 1
        G.ap_t = 0
        G.fx[#G.fx + 1] = { kind = "puff", x = e.x, y = e.y, t = 0 }
        sfx("kill")
        drop_item(e.x, e.y)
      else
        sfx("hit")
      end
    end
    if not dead and e.kb == 0 and overlap(h.x + 3, h.y + 4, 10, 11, e.x + 2, e.y + 4, 12, 11) then
      hurt_hero(W.FOES[e.kind].dmg, e.x, e.y)
    end
    if not dead then kept[#kept + 1] = e end
  end
  G.foes = kept
end

local function update_items()
  local h = G.hero
  local kept = {}
  for _, it in ipairs(G.items) do
    local keep = true
    if it.t then
      it.t = it.t - 1
      if it.t <= 0 then keep = false end
    end
    if keep and overlap(h.x + 2, h.y + 2, 12, 14, it.x + 3, it.y + 3, 10, 10) then
      keep = false
      if it.kind == "gem" then G.gems = min(999, G.gems + 1); sfx("gem")
      elseif it.kind == "heart" then G.hp = min(G.hp_max, G.hp + 2); sfx("heart")
      elseif it.kind == "key" then G.has_key = true; sfx("key")
      elseif it.kind == "sunstone" then
        G.mode, G.mt = "win", 0
        play_song("win")
        sfx("key")
      end
      G.hud_dirty = true
    end
    if keep then kept[#kept + 1] = it end
  end
  G.items = kept
  local fx = {}
  for _, e in ipairs(G.fx) do
    e.t = e.t + 1
    if e.t < 18 then fx[#fx + 1] = e end
  end
  G.fx = fx
end

-- Transitions ---------------------------------------------------------------

local function start_scroll(dx, dy)
  local to = neighbour(G.room, dx, dy)
  G.mode, G.mt = "scroll", 0
  G.tr = { dx = dx, dy = dy, to = to, from = G.room, waiting = true }
  G.foes, G.items, G.fx = {}, {}, {}
  if dx ~= 0 then
    G.jobs[#G.jobs + 1] = { "room", to, 32, 0 }
  else
    G.jobs[#G.jobs + 1] = { "room", G.room, 32, 0 }
    G.jobs[#G.jobs + 1] = { "room", to, 32, dy > 0 and 24 or 40 }
  end
end

local function update_scroll()
  local tr, h = G.tr, G.hero
  if tr.waiting then
    if #G.jobs > 0 then return end
    tr.waiting = false
    tr.k = 0
    if tr.dy ~= 0 then G.cam_x = 256 end
    return
  end
  tr.k = tr.k + 1
  local total = tr.dx ~= 0 and 64 or 48
  if tr.dx ~= 0 then G.cam_x = tr.dx * 4 * tr.k else G.cam_y = tr.dy * 4 * tr.k end
  -- the hero walks a few pixels into the new room as it slides in
  h.x = h.x + tr.dx * 0.25
  h.y = h.y + tr.dy * 0.25
  h.step = h.step + 1
  h.moving = true
  if tr.k >= total then
    h.x = h.x - tr.dx * 256
    h.y = h.y - tr.dy * 192
    G.cam_x, G.cam_y = 0, 0
    G.mode, G.mt = "play", 0
    G.tr = nil
    enter_room(tr.to)
  end
end

local function start_iris(to)
  G.mode, G.mt = "iris", 0
  G.iris_to = to
  G.foes, G.items, G.fx = {}, {}, {}
  sfx("stairs")
end

local IRIS_T = 30

local function update_iris()
  local t = G.mt
  if t < IRIS_T then
    G.iris = 180 * (1 - t / IRIS_T)
  elseif t == IRIS_T then
    G.iris = 0
    local h = G.hero
    if G.iris_to == W.CAVE then
      h.x, h.y, h.dir = W.CAVE_ARRIVE.x, W.CAVE_ARRIVE.y, "up"
    else
      h.x, h.y, h.dir = W.CAVE_RETURN.x, W.CAVE_RETURN.y, "down"
    end
    enter_room(G.iris_to)
  elseif #G.jobs > 0 then
    G.mt = IRIS_T  -- hold the iris shut until the room is drawn
  elseif t < IRIS_T * 2 + 4 then
    G.iris = 180 * max(0, (t - IRIS_T - 4) / IRIS_T)
  else
    G.iris = nil
    G.mode, G.mt = "play", 0
  end
end

-- Autopilot (attract mode) --------------------------------------------------

local ROUTE = { [4] = { 16, 5 }, [5] = { 7, -1 }, [2] = { -1, 5 }, [1] = { 7, 1 } }

local dist = {}
local function bfs(tc, tr)
  for i = 0, 16 * 12 - 1 do dist[i] = nil end
  local q, head = {}, 1
  local function push(c, r, d)
    if c < 0 or c > 15 or r < 0 or r > 11 then return end
    local i = r * 16 + c
    if dist[i] ~= nil or W.SOLID[cells[G.room][r][c]] then return end
    dist[i] = d
    q[#q + 1] = i
  end
  push(min(max(tc, 0), 15), min(max(tr, 0), 11), 0)
  while head <= #q do
    local i = q[head]
    head = head + 1
    local c, r, d = i % 16, i // 16, dist[i]
    push(c + 1, r, d + 1); push(c - 1, r, d + 1); push(c, r + 1, d + 1); push(c, r - 1, d + 1)
  end
  G.work = G.work + #q * 3
end

local function autopilot()
  local h = G.hero
  local p = { up = false, down = false, left = false, right = false, attack = false }
  local hx, hy = h.x + 8, h.y + 11
  -- nearest enemy (unless chasing it has taken too long), else items, else
  -- the next exit on the route
  G.ap_t = (G.ap_t or 0) + 1
  local best, target, tx, ty = 1e9, nil, nil, nil
  if G.ap_t < 420 then
    for _, e in ipairs(G.foes) do
      local d = abs(e.x - h.x) + abs(e.y - h.y)
      if d < best then best, target = d, e end
    end
  end
  for _, it in ipairs(G.items) do
    local d = abs(it.x - h.x) + abs(it.y - h.y)
    if d * 1.5 < best then best, target = d * 1.5, it end
  end
  if target and W.FOES[target.kind] then
    local ddx, ddy = target.x - h.x, target.y - h.y
    -- stand beside it on the axis it is nearer, then face it and swing
    local wx, wy, face
    if abs(ddx) > abs(ddy) then
      wx, wy, face = target.x - (ddx > 0 and 18 or -18), target.y, ddx > 0 and "right" or "left"
    else
      wx, wy, face = target.x, target.y - (ddy > 0 and 18 or -18), ddy > 0 and "down" or "up"
    end
    if abs(wx - h.x) <= 6 and abs(wy - h.y) <= 6 then
      if h.dir ~= face then p[face] = true
      elseif h.atk == 0 then p.attack = true end
      return p
    end
    if abs(wx - h.x) + abs(wy - h.y) < 40 then
      if wx > h.x + 1 then p.right = true elseif wx < h.x - 1 then p.left = true end
      if wy > h.y + 1 then p.down = true elseif wy < h.y - 1 then p.up = true end
      return p
    end
    tx, ty = floor((wx + 8) / 16), floor((wy + 11) / 16)
  elseif target then
    tx, ty = floor((target.x + 8) / 16), floor((target.y + 8) / 16)
  else
    local r = ROUTE[G.room]
    if G.room == W.CAVE then r = { W.CAVE_OUT.col, W.CAVE_OUT.row } end
    if not r then return p end
    tx, ty = r[1], r[2]
  end
  local cx, cy = floor(hx / 16), floor(hy / 16)
  local edge = nil
  if tx > 15 then edge, tx = "right", 15 elseif tx < 0 then edge, tx = "left", 0 end
  if ty > 11 then edge, ty = "down", 11 elseif ty < 0 then edge, ty = "up", 0 end
  if cx == tx and cy == ty then
    if edge then p[edge] = true
    elseif G.room == 1 then p.up = true
    elseif G.room == W.CAVE then p.down = true end
    return p
  end
  bfs(tx, ty)
  local here = dist[min(max(cy, 0), 11) * 16 + min(max(cx, 0), 15)] or 99
  local nc, nr = cx, cy
  for _, d in ipairs({ { 1, 0 }, { -1, 0 }, { 0, 1 }, { 0, -1 } }) do
    local c, r = cx + d[1], cy + d[2]
    if c >= 0 and c <= 15 and r >= 0 and r <= 11 then
      local v = dist[r * 16 + c]
      if v and v < here then here, nc, nr = v, c, r end
    end
  end
  local wx, wy = nc * 16 + 8, nr * 16 + 8
  if wx > hx + 1 then p.right = true elseif wx < hx - 1 then p.left = true end
  if wy > hy + 1 then p.down = true elseif wy < hy - 1 then p.up = true end
  return p
end

-- The frame -----------------------------------------------------------------

local last_attack, last_start = false, false
DEMO_FRAMES = 1620    -- the attract mode plays 27 seconds

local function read_input(pad_)
  local p = { up = pad_.up, down = pad_.down, left = pad_.left, right = pad_.right }
  local atk = (pad_.b or pad_.a) and true or false
  p.attack_pressed = atk and not last_attack
  p.start_pressed = pad_.start and not last_start
  p.any = atk or pad_.start or pad_.up or pad_.down or pad_.left or pad_.right
  last_attack, last_start = atk, pad_.start and true or false
  return p
end

local function check_exits()
  local h = G.hero
  local cx, fy = h.x + 8, h.y + 12
  if cx < 0 then start_scroll(-1, 0)
  elseif cx >= 256 then start_scroll(1, 0)
  elseif fy < 0 then start_scroll(0, -1)
  elseif fy >= 192 then start_scroll(0, 1)
  else
    local col, row = floor(cx / 16), floor((h.y + 11) / 16)
    if G.room == W.CAVE_IN.room and col == W.CAVE_IN.col and row == W.CAVE_IN.row then
      start_iris(W.CAVE)
    elseif G.room == W.CAVE and col == W.CAVE_OUT.col and row == W.CAVE_OUT.row then
      start_iris(W.CAVE_IN.room)
    end
  end
end

-- One frame of the game. pad_ is the machine's pad table (up, down, left,
-- right, a, b, start: both machines name them so).
function game_update(pad_)
  G.sfx = {}
  G.work = 0
  G.frame = G.frame + 1
  G.mt = G.mt + 1
  if G.flash > 0 then G.flash = G.flash - 1 end
  local input = read_input(pad_)

  if G.mode == "title" then
    G.idle = input.any and 0 or G.idle + 1
    if input.start_pressed then
      new_game(false)
    elseif G.idle > 300 then
      new_game(true)
    end
    return
  end
  if G.demo then
    -- attract mode: any button ends it; it also ends by itself
    G.demo_t = G.demo_t + 1
    if input.start_pressed or input.attack_pressed or G.demo_t > DEMO_FRAMES
      or (G.mode == "gameover" or G.mode == "win") and G.mt > 120 then
      game_init()
      return
    end
    local auto = autopilot()
    auto.attack_pressed = auto.attack
    input = auto
  end

  if G.mode == "play" then
    update_hero(input)
    update_foes()
    update_items()
    if G.mode == "play" then check_exits() end
  elseif G.mode == "scroll" then
    update_scroll()
  elseif G.mode == "iris" then
    update_iris()
  elseif G.mode == "dying" then
    if G.mt % 6 == 0 then
      G.hero.dir = DIR_LIST[(G.mt // 6) % 4 + 1]
    end
    if G.mt > 70 then
      G.mode, G.mt = "gameover", 0
      G.msg = "gameover"
      play_song("gameover")
    end
  elseif G.mode == "gameover" then
    if G.mt > 30 and input.start_pressed then new_game(false) end
  elseif G.mode == "win" then
    update_items()
    if G.mt == 60 then G.msg = "win" end
    if G.mt > 90 and input.start_pressed then game_init() end
  end
end

-- Music sequencer -----------------------------------------------------------

local NOTE = { C = 0, D = 2, E = 4, F = 5, G = 7, A = 9, B = 11 }
local parsed = {}

local function parse_track(s)
  local out, i, n = {}, 1, string.len(s)
  while i <= n do
    while i <= n and string.sub(s, i, i) == " " do i = i + 1 end
    local j = i
    while j <= n and string.sub(s, j, j) ~= " " do j = j + 1 end
    if j > i then
      local tok = string.sub(s, i, j - 1)
      local c = string.sub(tok, 1, 1)
      if c == "." then out[#out + 1] = 0
      elseif c == "-" then out[#out + 1] = -1
      elseif c == "K" or c == "S" or c == "H" then out[#out + 1] = c
      else
        local semi, k = NOTE[c], 2
        if string.sub(tok, 2, 2) == "#" then semi, k = semi + 1, 3 end
        local oct = string.byte(tok, k) - 48
        out[#out + 1] = 12 * (oct + 1) + semi
      end
    end
    i = j
  end
  return out
end

-- This frame's music events: ev[track] = MIDI note to start, -1 to stop,
-- "K"/"S"/"H" for a drum, or nil for no change. Tracks: 1 lead, 2 bass, 3 drums.
function music_events()
  local ev = {}
  if not G.song then return ev, true end
  local song = W.MUSIC[G.song]
  if not parsed[G.song] then
    local p = {}
    for i, s in ipairs(song.tracks) do p[i] = parse_track(s) end
    parsed[G.song] = p
  end
  local tracks = parsed[G.song]
  if G.song_sub == 0 then
    local len = #tracks[1]
    local step = G.song_step
    if step >= len then
      if song.loop then step = 0; G.song_step = 0 else G.song = nil; return ev, true end
    end
    for i, tr in ipairs(tracks) do
      local v = tr[step % #tr + 1]
      if v ~= 0 then ev[i] = v end
    end
  end
  G.song_sub = G.song_sub + 1
  if G.song_sub >= song.tempo then
    G.song_sub = 0
    G.song_step = G.song_step + 1
  end
  return ev, false
end

-- HUD -----------------------------------------------------------------------

-- The HUD as 4 rows of 32 characters/icon names for the machine to draw.
function hud_rows()
  local rows = { {}, {}, {}, {} }
  local function put(r, c, s)
    for i = 1, string.len(s) do rows[r][c + i - 1] = string.sub(s, i, i) end
  end
  for r = 1, 4 do for c = 0, 31 do rows[r][c] = " " end end
  put(2, 2, "LIFE")
  put(2, 12, "GEMS")
  put(2, 22, "KEY")
  for i = 0, G.hp_max // 2 - 1 do
    local v = G.hp - i * 2
    rows[3][2 + i] = v >= 2 and "heart_full" or (v == 1 and "heart_half" or "heart_empty")
  end
  rows[3][12] = "gem"
  local g = "" .. floor(G.gems)
  while string.len(g) < 3 do g = "0" .. g end
  put(3, 13, "x" .. g)
  if G.has_key then rows[3][22] = "key" end
  local name = G.room and W.rooms[G.room] and W.rooms[G.room].name or ""
  put(3, 26 - string.len(name) // 2 + 2, "")
  for c = 0, 31 do rows[4][c] = "bar" end
  return rows
end

MESSAGES = {
  gameover = { "GAME OVER", "PRESS START" },
  win = { "THE SUNSTONE IS YOURS!", "FERNWICK IS SAVED", "PRESS START" },
}

-- Sprite list: what to draw this frame, front to back, as
-- { name, x, y, flip_x, flip_y, layer } in screen pixels (playfield lines
-- start at y = 32). layer: "hero", "sword", "foe", "wisp", "item", "fx", "shadow".
function sprite_list()
  local out = {}
  local h = G.hero
  local function add(name, x, y, fx, fy, layer, extra)
    out[#out + 1] = { name, floor(x), floor(y) + 32, fx or false, fy or false, layer, extra }
  end
  local ox, oy = 0, 0
  if G.mode == "scroll" and G.tr and not G.tr.waiting then
    ox, oy = -G.cam_x, -G.cam_y
    if G.tr.dy ~= 0 then ox = 0 end
  end
  if G.mode ~= "title" then
    -- hero
    local blink = h.inv > 0 and (h.inv // 3) % 2 == 1
    if G.mode ~= "gameover" and not blink then
      local d = h.dir
      local key = (d == "left") and "r" or string.sub(d, 1, 1)
      local frame = (h.atk > 0) and "a" or (((h.step // 8) % 2 == 1 and h.moving) and "1" or "0")
      if h.atk > 0 then
        local sx, sy, sfx_, sfy = h.x, h.y, false, false
        local name = "sword_d"
        if d == "down" then sx, sy = h.x + 4, h.y + 11
        elseif d == "up" then sx, sy, sfy = h.x - 5, h.y - 11, true
        elseif d == "right" then sx, sy, name = h.x + 12, h.y + 2, "sword_r"
        else sx, sy, name, sfx_ = h.x - 12, h.y + 2, "sword_r", true end
        add(name, sx + ox, sy + oy, sfx_, sfy, "sword")
      end
      add("hero_" .. key .. frame, h.x + ox, h.y + oy, d == "left", false, "hero")
    end
    for _, e in ipairs(G.fx) do
      if e.kind == "spark" then
        if e.t < 6 then add("spark", e.x, e.y, false, false, "fx") end
      elseif e.kind == "puff" then
        add("puff" .. min(2, e.t // 6), e.x, e.y, false, false, "fx")
      elseif e.kind == "leaf" then
        add("puff" .. min(2, e.t // 6), e.x, e.y, false, false, "fx")
      end
    end
    for _, e in ipairs(G.foes) do
      if e.inv == 0 or (e.inv // 2) % 2 == 0 then
        local a = (e.phase // 10) % 2
        if e.kind == "blob" then add("blob" .. a, e.x, e.y, false, false, "foe")
        elseif e.kind == "beetle" then
          local d = e.dir
          if e.charge then a = (e.phase // 3) % 2 end
          if d == "left" or d == "right" then add("beetle_r" .. a, e.x, e.y, d == "left", false, "foe")
          else add("beetle_d" .. a, e.x, e.y, false, d == "up", "foe") end
        else
          add("wisp" .. ((e.phase // 12) % 2), e.x, e.y - 4 + sin(e.phase * 0.1) * 2, false, false, "wisp")
        end
      end
    end
    for _, it in ipairs(G.items) do
      if not it.t or it.t > 120 or (it.t // 4) % 2 == 0 then
        local bob = (it.kind == "sunstone") and floor(sin(G.frame * 0.08) * 2) or 0
        add(it.kind, it.x, it.y + bob, false, false, "item")
      end
    end
    -- shadows under the hero and walking foes
    if G.mode ~= "gameover" then add("shadow", h.x + ox, h.y + oy + 1, false, false, "shadow") end
    for _, e in ipairs(G.foes) do
      if e.kind == "wisp" then add("shadow", e.x, e.y + 6, false, false, "shadow") end
    end
  end
  return out
end

-- Game state as integers, for tests: mode, room, hero x, hero y, hp, gems,
-- has key, foes alive, kills, door open, cam x, cam y.
local MODE_ID = { title = 0, play = 1, scroll = 2, iris = 3, dying = 4, gameover = 5, win = 6 }
function quest_state()
  local h = G.hero
  return { MODE_ID[G.mode] or -1, G.room, floor(h.x), floor(h.y), G.hp, G.gems,
           G.has_key and 1 or 0, #G.foes, G.kills, G.door_open and 1 or 0,
           floor(G.cam_x), floor(G.cam_y), G.demo and 1 or 0 }
end

-- Enemies as integers, for tests: x, y, hit points for each.
function foe_state()
  local out = {}
  for _, e in ipairs(G.foes) do
    out[#out + 1] = floor(e.x); out[#out + 1] = floor(e.y); out[#out + 1] = e.hp
  end
  return out
end
