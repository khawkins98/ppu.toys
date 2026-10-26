-- tg16 prelude: the friendly register names and the per-line resolver.
-- Runs before every user chunk. Names starting with __ are internal.

floor, ceil, abs = math.floor, math.ceil, math.abs
min, max, sqrt = math.min, math.max, math.sqrt
sin, cos, atan = math.sin, math.cos, math.atan
pi = math.pi

-- piccolo's string library has no rep().
if not string.rep then
  function string.rep(s, n)
    local out = ""
    for _ = 1, n do out = out .. s end
    return out
  end
end

function clamp(v, lo, hi) return min(max(v, lo), hi) end
function lerp(a, b, t) return a + (b - a) * t end

-- Display and map size. Read when frame() returns; set them in init().
display = { width = 256, height = 224 }
bat = { w = 32, h = 32 }

-- CR enable bits.
show = { bg = true, sprites = true }

-- Background scroll. scroll.x is BXR. scroll.y is an offset like on the SNES
-- toys: inside hdma(), line y shows background row scroll.y + y. Assign the
-- raw BYR instead to get the hardware's reload behavior (line y shows row BYR,
-- and the lines below carry on from there).
local scroll_vals = { x = 0, y = 0 }
local in_hook = false
__byr_write = nil
bg = { scroll = setmetatable({}, {
  __index = scroll_vals,
  __newindex = function(_, k, v)
    if k ~= "x" and k ~= "y" then error("bg.scroll has only x and y", 2) end
    scroll_vals[k] = v
  end,
}) }
setmetatable(_G, {
  __newindex = function(t, k, v)
    if k == "BYR" then
      if in_hook then __byr_write = v else scroll_vals.y = v end
    elseif k == "BXR" then
      scroll_vals.x = v
    else
      rawset(t, k, v)
    end
  end,
  __index = function(_, k)
    if k == "BXR" then return scroll_vals.x end
    if k == "BYR" then return scroll_vals.y end
    return nil
  end,
})

-- Palette sugar over color(entry, value).
function pal(p, i, c) color(p * 16 + i, c) end
function spal(p, i, c) color(256 + p * 16 + i, c) end

-- Scanline hooks. Registered inside frame(), every frame.
__hooks = {}
function hdma(y0, y1, fn)
  if type(fn) ~= "function" then error("hdma(y0, y1, fn): fn must be a function", 2) end
  __hooks[#__hooks + 1] = { y0, y1, fn }
end
raster = hdma

-- Called by the engine after frame().
-- Writes 5 numbers per line into __lines: scroll x, BYR reload (-1 for
-- none), bg on, sprites on, and whether a hook ran on the line.
function __resolve(h)
  local bx, by = scroll_vals.x, scroll_vals.y
  local sbg, sspr = show.bg, show.sprites
  local out = {}
  local counter = floor(by)
  local offset_dirty = false
  local hooks = __hooks
  for y = 0, h - 1 do
    scroll_vals.x, scroll_vals.y = bx, by
    show.bg, show.sprites = sbg, sspr
    __byr_write = nil
    local touched = false
    for i = 1, #hooks do
      local hk = hooks[i]
      if y >= hk[1] and y <= hk[2] then
        if not touched then __line(y); in_hook = true; touched = true end
        hk[3](y)
      end
    end
    if y > 0 then counter = counter + 1 end
    -- A raw BYR write reloads the counter. A friendly scroll.y change asks
    -- for row scroll.y + y; once the hook stops, the frame-wide offset comes
    -- back. Untouched lines after a raw write keep counting from it.
    local want
    if __byr_write ~= nil then
      want = floor(__byr_write)
      offset_dirty = false
    elseif scroll_vals.y ~= by or offset_dirty then
      want = floor(scroll_vals.y) + y
      offset_dirty = scroll_vals.y ~= by
    end
    local reload = -1
    if want ~= nil and want ~= counter then reload = want % 1024; counter = want end
    local n = #out
    out[n + 1] = floor(scroll_vals.x) % 1024
    out[n + 2] = reload
    out[n + 3] = show.bg and 1 or 0
    out[n + 4] = show.sprites and 1 or 0
    out[n + 5] = touched and 1 or 0
  end
  in_hook = false
  __line(-1)
  scroll_vals.x, scroll_vals.y = bx, by
  show.bg, show.sprites = sbg, sspr
  __lines = out
  __frame_y = floor(by) % 1024
end
