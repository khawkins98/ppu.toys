-- 10_swarm.lua: the swarm's game logic, the same on both machines.
--
-- This file is identical in crates/tg16-core/toys/swarm/ and
-- compare/snes/swarm/: both toys run exactly this simulation, so the two
-- screens move in step and the only difference is what each machine's CPU
-- would pay for it. Each main.lua turns the counts returned here into a
-- cycle estimate for its own CPU (see COST there).
--
-- The work is a flock: every pair of drones is checked for being too close
-- (and pushed apart if so), then every drone steers toward a moving beacon,
-- clamps its speed and moves. The pair checks grow with the square of the
-- flock, which is where frame time goes. All state is integers (8.8 fixed
-- point), as it would be in assembly.

SW = {}
SW.MAX = 52        -- most drones (the TG16 has 64 sprites; one is the beacon)
SW.R = 14          -- separation radius in pixels
SW.PUSH = 24       -- push per close neighbour (1/256 px per frame^2)
SW.PULL = 10       -- pull toward the beacon
SW.VMAX = 400      -- speed limit, about 1.6 px per frame

local X, Y, VX, VY, AX, AY = {}, {}, {}, {}, {}, {}
SW.X, SW.Y, SW.DIR, SW.FX, SW.FY = {}, {}, {}, {}, {}

for i = 1, SW.MAX do
  X[i] = (16 + (i * 97) % 224) * 256
  Y[i] = (16 + (i * 61) % 176) * 256
  VX[i] = (i * 37) % 512 - 256
  VY[i] = (i * 53) % 512 - 256
end

-- How many drones fly at time t: 12 rising to 52 over 12 s, held for 4 s,
-- back down over 8 s, every 24 s.
function SW.count(t)
  local p = t % 24
  local k = p < 12 and p / 12 or (p < 16 and 1 or (24 - p) / 8)
  return 12 + floor(40 * k + 0.5)
end

function SW.beacon(t)
  return 120 + floor(84 * sin(t * 0.63)), 100 + floor(64 * sin(t * 1.07 + 1))
end

-- One frame of the flock. Returns the pairs checked and the pairs that were
-- close enough to push apart.
function SW.step(t, n)
  local tx, ty = SW.beacon(t)
  local R = SW.R
  for i = 1, n do AX[i] = 0; AY[i] = 0 end
  local close = 0
  for i = 1, n - 1 do
    local xi, yi = X[i] // 256, Y[i] // 256
    for j = i + 1, n do
      local dx = X[j] // 256 - xi
      if dx < R and dx > -R then
        local dy = Y[j] // 256 - yi
        if dy < R and dy > -R then
          close = close + 1
          local sx = dx >= 0 and SW.PUSH or -SW.PUSH
          local sy = dy >= 0 and SW.PUSH or -SW.PUSH
          AX[i] = AX[i] - sx; AX[j] = AX[j] + sx
          AY[i] = AY[i] - sy; AY[j] = AY[j] + sy
        end
      end
    end
  end
  local vmax = SW.VMAX
  for i = 1, n do
    local x, y = X[i] // 256, Y[i] // 256
    local vx = VX[i] + AX[i] + (tx + 4 > x and SW.PULL or -SW.PULL)
    local vy = VY[i] + AY[i] + (ty + 4 > y and SW.PULL or -SW.PULL)
    if vx > vmax then vx = vmax elseif vx < -vmax then vx = -vmax end
    if vy > vmax then vy = vmax elseif vy < -vmax then vy = -vmax end
    local nx, ny = X[i] + vx, Y[i] + vy
    if nx < 0 then nx = 0; vx = -vx elseif nx > 240 * 256 then nx = 240 * 256; vx = -vx end
    if ny < 0 then ny = 0; vy = -vy elseif ny > 208 * 256 then ny = 208 * 256; vy = -vy end
    X[i], Y[i], VX[i], VY[i] = nx, ny, vx, vy
    SW.X[i], SW.Y[i] = nx // 256, ny // 256
    -- Eight headings from three drawings: 0 right, 1 up-right, 2 up, and
    -- the flips of those.
    local ax, ay = abs(vx), abs(vy)
    local d, fx, fy
    if ax > 2 * ay then d = 0 elseif ay > 2 * ax then d = 2 else d = 1 end
    fx = vx < 0
    fy = vy > 0
    SW.DIR[i], SW.FX[i], SW.FY[i] = d, fx, fy
  end
  return n * (n - 1) // 2, close
end

-- Which drones to draw. A line holds `limit` 16-pixel sprites (TG16: 16
-- units; SNES: 34 slivers / 2 = 17), so count sprites per 8-line band and
-- leave out one that would overfill a band, starting from a different drone
-- every frame so the ones left out flicker instead of vanishing (what games
-- did). `taken` sprites (the beacon) are already counted at band `b0`.
local band = {}
function SW.visible(n, limit, f, by0)
  for b = 0, 28 do band[b] = 0 end
  local b0 = by0 // 8
  for b = b0, (by0 + 15) // 8 do band[b] = 1 end
  local out = {}
  for k = 0, n - 1 do
    local i = (k + f * 7) % n + 1
    local y = SW.Y[i]
    local lo, hi = y // 8, (y + 15) // 8
    local ok = true
    for b = lo, hi do if band[b] >= limit then ok = false end end
    if ok then
      for b = lo, hi do band[b] = band[b] + 1 end
      out[#out + 1] = i
    end
  end
  return out
end

-- The drone drawings: d = 0 points right, 1 up-right, 2 up. Colour indices:
-- 1 rim, 2 body, 3 the bright spine.
function SW.shape(d)
  local a = d * pi / 4
  local ca, sa = cos(a), sin(a)
  local px = {}
  for i = 0, 255 do
    local x, y = i % 16 - 7.5, -(i // 16 - 7.5)
    local u, v = x * ca + y * sa, -x * sa + y * ca
    local av = abs(v)
    local c = 0
    if u <= 6.5 and u >= -5.5 and av <= (6.5 - u) * 0.45 and u >= -5.5 + 0.8 * (5 - av) then
      c = 2
      if av < 1 and u > -1.5 then c = 3 end
      if av > (6.5 - u) * 0.45 - 1.1 or u < -4.5 + 0.8 * (5 - av) then c = 1 end
    end
    px[i + 1] = c
  end
  return px
end

-- The beacon: a glowing orb, indices 1 (halo) to 3 (core).
function SW.orb()
  local px = {}
  for i = 0, 255 do
    local x, y = i % 16 - 7.5, i // 16 - 7.5
    local d = x * x + y * y
    px[i + 1] = d < 6 and 3 or (d < 22 and 2 or (d < 50 and 1 or 0))
  end
  return px
end

-- Stars for the background: a few tile shapes as 64 indices.
function SW.star(k)
  local px = {}
  for i = 1, 64 do px[i] = 0 end
  if k == 1 then px[28] = 1
  elseif k == 2 then px[20] = 2; px[19] = 1; px[21] = 1; px[12] = 1; px[28] = 1
  elseif k == 3 then px[45] = 3 end
  return px
end
