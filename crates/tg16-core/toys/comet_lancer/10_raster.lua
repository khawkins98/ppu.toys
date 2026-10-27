-- Comet Lancer, part 1: a tiny software 3D renderer and scaled sprite sets.
--
-- The TurboGrafx-16 has no polygon hardware (the SNES Star Fox carts carried
-- a Super FX chip for that). So, like the few TG16 games that tried, this
-- toy fills flat-shaded polygons with the CPU into a small off-screen buffer
-- and uploads it as sprite patterns. Everything else in 3D is a sprite drawn
-- from a pre-scaled set (rendered once, in init(), by the same code).
--
-- Every live render charges its estimated HuC6280 time with cpu(), so the
-- budget meter stays honest about what filling polygons costs.

R3 = {}

-- Rough cycle costs for hand-written 6280 code doing this work.
local COST_CLEAR = 6      -- per byte of a 4bpp planar buffer (block clear)
local COST_VERT = 150     -- rotate + project one vertex (table multiplies)
local COST_FACE = 260     -- set up one polygon: normal, light, edges
local COST_SPAN = 40      -- start one horizontal span
local COST_PIXEL = 6      -- plot one pixel into planar 4bpp

function R3.buffer(w, h)
  local b = { w = w, h = h }
  for i = 1, w * h do b[i] = 0 end
  return b
end

function R3.clear(b)
  for i = 1, b.w * b.h do b[i] = 0 end
end

-- Insertion sort on a list of {key, ...} tables, biggest key first.
function R3.sort_desc(list, n)
  for i = 2, n do
    local v = list[i]
    local j = i - 1
    while j >= 1 and list[j][1] < v[1] do
      list[j + 1] = list[j]
      j = j - 1
    end
    list[j + 1] = v
  end
end

-- Fill a convex polygon (screen points {x1, y1, x2, y2, ...}) with color c.
-- Returns spans and pixels drawn.
function R3.fill(b, pts, c)
  local n = #pts // 2
  local ymin, ymax = 1e9, -1e9
  for i = 1, n do
    local y = pts[i * 2]
    if y < ymin then ymin = y end
    if y > ymax then ymax = y end
  end
  local y0 = max(0, ceil(ymin - 0.5))
  local y1 = min(b.h - 1, floor(ymax - 0.5))
  local spans, pixels = 0, 0
  local w = b.w
  for y = y0, y1 do
    local py = y + 0.5
    local xl, xr = 1e9, -1e9
    for i = 1, n do
      local j = i % n + 1
      local ax, ay = pts[i * 2 - 1], pts[i * 2]
      local bx, by = pts[j * 2 - 1], pts[j * 2]
      if (ay <= py and by > py) or (by <= py and ay > py) then
        local x = ax + (py - ay) * (bx - ax) / (by - ay)
        if x < xl then xl = x end
        if x > xr then xr = x end
      end
    end
    local a = max(0, ceil(xl - 0.5))
    local e = min(w - 1, floor(xr - 0.5))
    if e >= a then
      local row = y * w + 1
      for x = a, e do b[row + x] = c end
      spans = spans + 1
      pixels = pixels + e - a + 1
    end
  end
  return spans, pixels
end

-- A model is { v = {{x, y, z}, ...}, f = {{i1, i2, i3[, i4], c = color,
-- two = double_sided}, ...} } in model space: x right, y up, z forward.
-- Face normals are pointed away from the model's centre once, so solid
-- parts cull their back faces; thin parts (wings, fins) set two = true.
function R3.prepare(m)
  local cx, cy, cz = 0, 0, 0
  for _, p in ipairs(m.v) do cx, cy, cz = cx + p[1], cy + p[2], cz + p[3] end
  local n = #m.v
  cx, cy, cz = cx / n, cy / n, cz / n
  for _, f in ipairs(m.f) do
    local a, b, c = m.v[f[1]], m.v[f[2]], m.v[f[3]]
    local ux, uy, uz = b[1] - a[1], b[2] - a[2], b[3] - a[3]
    local vx, vy, vz = c[1] - a[1], c[2] - a[2], c[3] - a[3]
    local nx, ny, nz = uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx
    local fx, fy, fz = 0, 0, 0
    for k = 1, #f do
      local p = m.v[f[k]]
      fx, fy, fz = fx + p[1], fy + p[2], fz + p[3]
    end
    fx, fy, fz = fx / #f - cx, fy / #f - cy, fz / #f - cz
    if nx * fx + ny * fy + nz * fz < 0 then
      -- Reverse the winding so the normal points outward.
      local r = {}
      for k = #f, 1, -1 do r[#r + 1] = f[k] end
      for k = 1, #f do f[k] = r[k] end
    end
  end
  return m
end

-- Light comes from the upper left, slightly in front of the camera.
local LX, LY, LZ = -0.45, 0.75, -0.48

-- Draw model m into buffer b.
-- o: yaw, pitch, roll (radians), scale (pixels per unit), cx, cy (screen
-- centre in the buffer), dist (perspective distance in model units),
-- view (extra downward look, radians), shades (levels per material, 4).
-- Returns the estimated cycles this took on a TG16.
function R3.draw(m, b, o)
  local cy_, sy_ = cos(o.yaw or 0), sin(o.yaw or 0)
  local cp, sp = cos(o.pitch or 0), sin(o.pitch or 0)
  local cr, sr = cos(o.roll or 0), sin(o.roll or 0)
  local cv, sv = cos(o.view or 0), sin(o.view or 0)
  local scale, dist = o.scale or 1, o.dist or 200
  local ox, oy = o.cx or b.w / 2, o.cy or b.h / 2
  local shades = o.shades or 4
  -- Transform every vertex: roll (z), pitch (x), yaw (y), then the view tilt.
  local tv, sx, sy = {}, {}, {}
  for i, p in ipairs(m.v) do
    local x, y, z = p[1], p[2], p[3]
    x, y = x * cr - y * sr, x * sr + y * cr
    y, z = y * cp - z * sp, y * sp + z * cp
    x, z = x * cy_ + z * sy_, -x * sy_ + z * cy_
    y, z = y * cv + z * sv, -y * sv + z * cv
    tv[i] = { x, y, z }
    local k = dist / (dist + z)
    sx[i] = ox + x * scale * k
    sy[i] = oy - y * scale * k
  end
  -- Shade and cull faces, then sort far to near (painter's algorithm).
  local list, n = {}, 0
  for _, f in ipairs(m.f) do
    local a, bb, c = tv[f[1]], tv[f[2]], tv[f[3]]
    local ux, uy, uz = bb[1] - a[1], bb[2] - a[2], bb[3] - a[3]
    local vx, vy, vz = c[1] - a[1], c[2] - a[2], c[3] - a[3]
    local nx, ny, nz = uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx
    local len = sqrt(nx * nx + ny * ny + nz * nz)
    if len > 0 then
      nx, ny, nz = nx / len, ny / len, nz / len
      -- Facing the camera when the normal points back along the view ray.
      local facing = nx * a[1] + ny * a[2] + nz * (a[3] + dist) < 0
      if facing or f.two then
        if not facing then nx, ny, nz = -nx, -ny, -nz end
        local zsum = 0
        for k = 1, #f do zsum = zsum + tv[f[k]][3] end
        local lum = max(0, nx * LX + ny * LY + nz * LZ)
        local level = clamp(floor(lum * shades), 0, shades - 1)
        n = n + 1
        list[n] = { zsum / #f, f, level }
      end
    end
  end
  R3.sort_desc(list, n)
  local spans, pixels = 0, 0
  for i = 1, n do
    local f = list[i][2]
    local pts = {}
    for k = 1, #f do
      pts[k * 2 - 1] = sx[f[k]]
      pts[k * 2] = sy[f[k]]
    end
    local c = f.flat and f.c or f.c + list[i][3]
    local s, p = R3.fill(b, pts, c)
    spans, pixels = spans + s, pixels + p
  end
  return #m.v * COST_VERT + n * COST_FACE + spans * COST_SPAN + pixels * COST_PIXEL
end

function R3.clear_cost(b) return b.w * b.h // 2 * COST_CLEAR end

-- Upload a w x h region of buffer b (at bx, by) as one sprite's patterns.
-- sw, sh: sprite size (16/32 wide, 16/32/64 high); base: first pattern.
function R3.upload(b, bx, by, sw, sh, base)
  for cy = 0, sh // 16 - 1 do
    for cx = 0, sw // 16 - 1 do
      local px = {}
      for y = 0, 15 do
        local row = (by + cy * 16 + y) * b.w + bx + cx * 16 + 1
        for x = 0, 15 do px[y * 16 + x + 1] = b[row + x] or 0 end
      end
      pattern(base + cx + cy * 2, px)
    end
  end
end

-- Pattern allocator. Sprites wider or taller than 16 need aligned blocks.
local next_pattern = 0
function R3.patterns_from(p) next_pattern = p end
function R3.alloc(sw, sh)
  local block = ((sh > 16) and 2 or sw // 16) * (sh // 16)
  local p = (next_pattern + block - 1) // block * block
  next_pattern = p + block
  return p
end
function R3.patterns_used() return next_pattern end

-- A scaled sprite set: one canvas per size, cut into sprites of up to
-- 32x64, uploaded once. draw(buf, size) paints the canvas.
-- levels: {{size = px, w = canvas w, h = canvas h}, ...}, smallest first.
-- mirror = true means the canvas is the top-left quarter of a symmetric
-- image, drawn four times with flips (how big round things save VRAM).
function R3.set(levels, draw, mirror)
  local set = { mirror = mirror }
  for li, lv in ipairs(levels) do
    local b = R3.buffer(lv.w, lv.h)
    draw(b, lv.size, li)
    local sw, sh = min(lv.w, 32), min(lv.h, 64)
    local parts = {}
    for j = 0, lv.h // sh - 1 do
      for i = 0, lv.w // sw - 1 do
        local p = R3.alloc(sw, sh)
        R3.upload(b, i * sw, j * sh, sw, sh, p)
        parts[#parts + 1] = { i * sw, j * sh, p }
      end
    end
    set[li] = { size = lv.size, w = lv.w, h = lv.h, sw = sw, sh = sh, parts = parts }
  end
  return set
end

-- Pick the level whose size is closest to px (on a log scale), or nil when
-- the object would be smaller than the first level's half.
function R3.level(set, px)
  if px < set[1].size * 0.5 then return nil end
  local best, bd = 1, 1e9
  for i = 1, #set do
    local d = abs(set[i].size - px) / set[i].size
    if px > set[i].size then d = (px - set[i].size) / px end
    if d < bd then best, bd = i, d end
  end
  return best
end
