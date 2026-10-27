-- Comet Lancer, part 2: models, palettes and the pre-scaled sprite sets.
-- All art here is original and made from code.

-- The player's ship, the Lancer: an arrowhead hull, swept wings and twin
-- red tail fins. Materials: 1 hull, 5 blue trim, 9 red, 13 engine (flat).
SHIP = R3.prepare({
  v = {
    { 0, 0, 30 },      -- 1 nose
    { 0, 4, 10 },      -- 2 canopy front
    { 0, 5, -2 },      -- 3 canopy back
    { -6, 0, 2 },      -- 4 hull left
    { 6, 0, 2 },       -- 5 hull right
    { 0, -4, 4 },      -- 6 belly
    { 0, 3, -14 },     -- 7 tail top
    { -5, 0, -14 },    -- 8 tail left
    { 5, 0, -14 },     -- 9 tail right
    { 0, -3, -14 },    -- 10 tail bottom
    { -5, 0, 8 },      -- 11 left wing root, front
    { -15, -2, -4 },   -- 12 left wing, middle front
    { -26, -4, -15 },  -- 13 left wing tip
    { -5, 0, -12 },    -- 14 left wing root, back
    { -14, -2, -13 },  -- 15 left wing, middle back
    { 5, 0, 8 },       -- 16..20 the right wing
    { 15, -2, -4 },
    { 26, -4, -15 },
    { 5, 0, -12 },
    { 14, -2, -13 },
    { -3, 2, -4 },     -- 21 left fin root, front
    { -11, 12, -18 },  -- 22 left fin tip
    { -5, 2, -14 },    -- 23 left fin root, back
    { 3, 2, -4 },      -- 24..26 the right fin
    { 11, 12, -18 },
    { 5, 2, -14 },
  },
  f = {
    { 1, 4, 2, c = 1 }, { 1, 2, 5, c = 1 },              -- nose deck
    { 2, 4, 3, c = 15, flat = true },                    -- canopy
    { 2, 3, 5, c = 15, flat = true },
    { 3, 4, 8, 7, c = 5 }, { 3, 7, 9, 5, c = 5 },        -- rear deck
    { 1, 6, 4, c = 1 }, { 1, 5, 6, c = 1 },              -- chin
    { 4, 6, 10, 8, c = 1 }, { 5, 9, 10, 6, c = 1 },      -- belly
    { 7, 8, 10, 9, c = 13, flat = true },                -- engine
    { 11, 12, 15, 14, c = 1, two = true },               -- wings
    { 12, 13, 15, c = 5, two = true },
    { 16, 17, 20, 19, c = 1, two = true },
    { 17, 18, 20, c = 5, two = true },
    { 21, 22, 23, c = 9, two = true },                   -- fins
    { 24, 25, 26, c = 9, two = true },
  },
})

-- The enemy, a Hornet: a dark diamond with a yellow eye and four blades.
-- Its nose points at +z, so it is drawn turned around to face the player.
HORNET = R3.prepare({
  v = {
    { -2, 2, 14 }, { 2, 2, 14 }, { 2, -2, 14 }, { -2, -2, 14 },  -- 1..4 eye
    { 0, 8, -4 },     -- 5 top
    { 8, 0, -4 },     -- 6 right
    { 0, -8, -4 },    -- 7 bottom
    { -8, 0, -4 },    -- 8 left
    { 0, 0, -12 },    -- 9 back
    { -22, 9, 8 },    -- 10 left upper blade tip
    { 22, 9, 8 },     -- 11 right upper blade tip
    { -19, -8, 6 },   -- 12 left lower blade tip
    { 19, -8, 6 },    -- 13 right lower blade tip
  },
  f = {
    { 1, 2, 3, 4, c = 13, flat = true },
    { 1, 5, 2, c = 1 }, { 2, 5, 6, c = 9 }, { 2, 6, 3, c = 1 }, { 3, 6, 7, c = 9 },
    { 3, 7, 4, c = 1 }, { 4, 7, 8, c = 9 }, { 4, 8, 1, c = 1 }, { 1, 8, 5, c = 9 },
    { 9, 5, 8, c = 1 }, { 9, 6, 5, c = 1 }, { 9, 7, 6, c = 1 }, { 9, 8, 7, c = 1 },
    { 8, 10, 5, c = 5, two = true }, { 6, 5, 11, c = 5, two = true },
    { 8, 7, 12, c = 5, two = true }, { 6, 13, 7, c = 5, two = true },
  },
})

function palettes()
  -- Sprite palette 0: the Lancer.
  local ship = {
    rgb(80, 86, 104), rgb(135, 145, 165), rgb(185, 195, 212), rgb(228, 232, 242),
    rgb(20, 40, 110), rgb(40, 80, 180), rgb(70, 130, 230), rgb(140, 190, 255),
    rgb(110, 20, 20), rgb(170, 40, 30), rgb(225, 70, 50), rgb(255, 130, 100),
    rgb(255, 150, 40), rgb(255, 230, 120), rgb(140, 220, 255),
  }
  for i, c in ipairs(ship) do spal(0, i, c) end
  -- 1: Hornets. 8: the same, flashing white when hit.
  local hornet = {
    rgb(40, 30, 50), rgb(70, 55, 85), rgb(110, 90, 125), rgb(150, 130, 165),
    rgb(90, 20, 80), rgb(150, 40, 130), rgb(210, 70, 180), rgb(250, 130, 230),
    rgb(120, 20, 10), rgb(190, 50, 20), rgb(240, 110, 30), rgb(255, 200, 60),
    rgb(255, 240, 80),
  }
  for i, c in ipairs(hornet) do spal(1, i, c); spal(8, i, rgb(255, 255, 255)) end
  -- 2: towers. 3: rings. 4: lasers. 5: enemy shots. 6: explosions. 7: shadow.
  local tower = { rgb(60, 70, 90), rgb(95, 105, 125), rgb(140, 150, 170),
                  rgb(185, 195, 210), rgb(25, 30, 45), rgb(255, 220, 120) }
  for i, c in ipairs(tower) do spal(2, i, c) end
  local ring = { rgb(140, 90, 10), rgb(220, 160, 30), rgb(255, 220, 90), rgb(255, 255, 220) }
  for i, c in ipairs(ring) do spal(3, i, c) end
  local laser = { rgb(20, 140, 60), rgb(60, 230, 120), rgb(220, 255, 230) }
  for i, c in ipairs(laser) do spal(4, i, c) end
  local shot = { rgb(150, 20, 90), rgb(255, 80, 170), rgb(255, 220, 240) }
  for i, c in ipairs(shot) do spal(5, i, c) end
  local boom = { rgb(170, 30, 20), rgb(240, 110, 30), rgb(255, 210, 60), rgb(255, 255, 230) }
  for i, c in ipairs(boom) do spal(6, i, c) end
  spal(7, 1, rgb(20, 60, 30))
end

-- Deterministic noise for the art (the same every run).
local seed = 12345
local function rnd()
  seed = (seed * 1103515245 + 12345) % 2147483648
  return seed / 2147483648
end

local function disc(b, cx, cy, r, c)
  for y = max(0, floor(cy - r)), min(b.h - 1, ceil(cy + r)) do
    for x = max(0, floor(cx - r)), min(b.w - 1, ceil(cx + r)) do
      local dx, dy = x + 0.5 - cx, y + 0.5 - cy
      if dx * dx + dy * dy <= r * r then b[y * b.w + x + 1] = c end
    end
  end
end

-- Size levels: { size in pixels, canvas w, canvas h }.
local function levels(list)
  local out = {}
  for _, l in ipairs(list) do out[#out + 1] = { size = l[1], w = l[2], h = l[3] } end
  return out
end

function build_sets()
  -- Hornets: a level frame and a banked frame (mirrored for the other way).
  -- size = wingspan in pixels.
  local hl = levels({ { 6, 16, 16 }, { 9, 16, 16 }, { 12, 16, 16 }, { 15, 16, 16 },
                      { 19, 32, 32 }, { 23, 32, 32 }, { 27, 32, 32 }, { 31, 32, 32 },
                      { 44, 64, 64 }, { 60, 64, 64 } })
  local function hornet(roll)
    return function(b, size)
      R3.draw(HORNET, b, { yaw = pi, roll = roll, pitch = 0.15, scale = size / 40,
                           dist = 300, view = 0.1, cx = b.w / 2, cy = b.h / 2 })
    end
  end
  SET_HORNET = R3.set(hl, hornet(0))
  SET_HORNET_BANK = R3.set(hl, hornet(0.6))

  -- Towers: size = width of the front face; they stand on their bottom edge.
  -- The canvas shows the right-hand side; flip it for towers on the right.
  local tl = levels({ { 4, 16, 32 }, { 6, 16, 32 }, { 8, 16, 32 }, { 11, 16, 32 },
                      { 14, 32, 64 }, { 18, 32, 64 }, { 22, 32, 64 },
                      { 30, 64, 128 }, { 40, 64, 128 }, { 46, 64, 128 } })
  SET_TOWER = R3.set(tl, function(b, size)
    local side = max(1, floor(size * 0.3))
    local hgt = floor(size * 2.4)
    local top = max(1, floor(size * 0.18))
    local y0 = b.h - hgt
    for y = y0, b.h - 1 do
      for x = 0, size - 1 do
        local c = 3
        if size >= 8 and x % 4 >= 1 and x % 4 <= 2 and (y - y0) % 6 >= 2 and (y - y0) % 6 <= 3
           and x > 0 and x < size - 1 then
          c = rnd() < 0.3 and 6 or 5
        end
        b[y * b.w + x + 1] = c
      end
      for x = size, size + side - 1 do
        -- The side face slants up towards the back.
        if y >= y0 - floor((x - size + 1) * top / side) then b[y * b.w + x + 1] = 1 end
      end
    end
    for y = y0 - top, y0 - 1 do
      local k = y0 - y
      for x = 0, size + side - 1 do
        local slant = floor(k * side / top)
        if x >= slant and x < size + slant and x < b.w then b[y * b.w + x + 1] = 4 end
      end
    end
    if size >= 4 then
      for y = y0, b.h - 1 do b[y * b.w + 1] = 2 end
    end
  end)

  -- Rings, drawn as a top-left quarter and mirrored. size = diameter.
  SET_RING = R3.set(levels({ { 8, 16, 16 }, { 12, 16, 16 }, { 18, 16, 16 }, { 26, 16, 16 },
                             { 36, 32, 32 }, { 46, 32, 32 }, { 58, 32, 32 },
                             { 76, 64, 64 }, { 98, 64, 64 }, { 124, 64, 64 } }),
    function(b, size)
      local r = size / 2
      local t = max(1.2, r * 0.17)
      local cx, cy = b.w, b.h
      for y = 0, b.h - 1 do
        for x = 0, b.w - 1 do
          local dx, dy = x + 0.5 - cx, y + 0.5 - cy
          local d = sqrt(dx * dx + dy * dy)
          if d <= r and d >= r - t then
            local k = (d - (r - t)) / t
            local c = 2
            if k > 0.75 then c = 1 elseif k < 0.35 then c = 3 end
            if size >= 26 and dx < -r * 0.5 and dy < -r * 0.5 and k > 0.3 and k < 0.6 then c = 4 end
            b[y * b.w + x + 1] = c
          end
        end
      end
    end, true)

  -- Bolts (lasers and enemy shots share patterns, not palettes).
  SET_BOLT = R3.set(levels({ { 2, 16, 16 }, { 3, 16, 16 }, { 5, 16, 16 }, { 7, 16, 16 },
                             { 10, 16, 16 }, { 14, 16, 16 } }),
    function(b, size)
      disc(b, 8, 8, size / 2 + 0.3, 1)
      disc(b, 8, 8, size / 2 * 0.7, 2)
      if size >= 5 then disc(b, 8, 8, size / 2 * 0.35, 3) end
    end)

  -- Explosions: four frames each in two sizes.
  local function boom(canvas)
    local sets = {}
    for k = 1, 4 do
      sets[k] = R3.set(levels({ { canvas, canvas, canvas } }), function(b)
        local c = canvas / 2
        local r = canvas * (0.18 + k * 0.08)
        for _ = 1, 10 + k * 3 do
          local a, d = rnd() * 2 * pi, rnd() * r * 0.8
          local pr = canvas * (0.07 + rnd() * 0.12) * (1.25 - k * 0.15)
          local col = k >= 4 and 1 or (k == 3 and 2 or 3)
          disc(b, c + cos(a) * d, c + sin(a) * d, pr, col)
          if k <= 2 then disc(b, c + cos(a) * d * 0.6, c + sin(a) * d * 0.6, pr * 0.5, 4) end
        end
      end)
    end
    return sets
  end
  SET_BOOM16, SET_BOOM32 = boom(16), boom(32)

  -- The ship's shadow on the ground.
  SET_SHADOW = R3.set(levels({ { 10, 16, 16 }, { 16, 16, 16 }, { 24, 32, 16 }, { 32, 32, 16 } }),
    function(b, size)
      for y = 0, b.h - 1 do
        for x = 0, b.w - 1 do
          local dx, dy = (x + 0.5 - b.w / 2) / (size / 2), (y + 0.5 - b.h / 2) / (size / 6)
          if dx * dx + dy * dy <= 1 then b[y * b.w + x + 1] = 1 end
        end
      end
    end)
end
