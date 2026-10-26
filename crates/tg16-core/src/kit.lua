-- tg16 kit: SNES-style effects, faked the way TurboGrafx-16 games did it.
-- Every helper is plain Lua over the core calls (hdma, spr, color, show,
-- bg.scroll, BYR), so its cost shows up in the budget meter like your own
-- code would. Nothing here adds hardware the console does not have.

kit = {}

-- Parallax from one background layer: each band of scanlines scrolls at its
-- own speed. bands = {{y0, y1, speed}, ...}; a band can also give
-- {y0, y1, speed_top, speed_bottom} to ramp the speed line by line (a floor
-- that speeds up toward the viewer). Costs one raster line per scanline
-- in a band where the speed changes.
function kit.parallax(cam_x, bands)
  for _, b in ipairs(bands) do
    local y0, y1, s0, s1 = b[1], b[2], b[3], b[4]
    if s1 == nil then
      local x = floor(cam_x * s0)
      hdma(y0, y0, function() bg.scroll.x = x end)
      if y1 > y0 then hdma(y0 + 1, y1, function() bg.scroll.x = x end) end
    else
      hdma(y0, y1, function(y)
        local k = (y1 == y0) and 0 or (y - y0) / (y1 - y0)
        bg.scroll.x = floor(cam_x * (s0 + (s1 - s0) * k))
      end)
    end
  end
end

-- A second "background" made of sprites: a row of identical sprites that
-- wraps around as it scrolls, like clouds or a foreground fence.
-- opts: first (SAT index), count, pattern, w, h, y, x (scroll), spacing,
-- pal, front, width (wrap width, default display.width + spacing).
-- Returns the next free SAT index. Watch the line meter: a row of 32-wide
-- sprites costs two units each on every line it covers.
function kit.sprite_layer(opts)
  local w = opts.w or 16
  local spacing = opts.spacing or w
  local count = opts.count or floor(display.width / spacing) + 2
  local wrap = opts.width or count * spacing
  local x0 = -((opts.x or 0) % wrap)
  for i = 0, count - 1 do
    local x = (x0 + i * spacing) % wrap
    if x > display.width then x = x - wrap end
    spr((opts.first or 0) + i, floor(x), opts.y or 0, opts.pattern or 0, {
      w = w, h = opts.h or 16, pal = opts.pal or 0,
      front = opts.front ~= false,
    })
  end
  return (opts.first or 0) + count
end

-- Soft Mode 7: a perspective floor without rotation hardware. Below the
-- horizon, each line picks a map row by depth (a raw BYR reload) and shifts
-- sideways by depth times curve, the way TG16 racing games drew roads.
-- opts: horizon, bottom, z (distance travelled, in pixels), x (camera),
-- height (camera height, bigger = flatter), curve (sideways bend per depth),
-- row0 and rows (the floor texture is map pixel rows row0..row0+rows-1;
-- default the whole map).
-- Costs a raster line and two VDC writes per floor line: expensive, and the
-- meter says so.
function kit.floor(opts)
  local horizon = opts.horizon or 112
  local bottom = opts.bottom or display.height - 1
  local height = opts.height or 32
  local row0 = opts.row0 or 0
  local rows = opts.rows or bat.h * 8 - row0
  local z, cx, curve = opts.z or 0, opts.x or 0, opts.curve or 0
  hdma(horizon, bottom, function(y)
    local depth = height * 32 / (y - horizon + 1)
    BYR = row0 + floor(z + depth) % rows
    bg.scroll.x = floor(cx + curve * depth)
  end)
end

-- Flicker transparency: draw a sprite on alternating frames so it reads as
-- half-transparent on a CRT. phase picks even (0) or odd (1) frames.
function kit.flicker(f, phase)
  return f % 2 == (phase or 0)
end

function kit.flicker_spr(f, i, x, y, pattern, opts)
  if kit.flicker(f, opts and opts.phase) then spr(i, x, y, pattern, opts) end
end

-- Horizontal mask (letterbox, split-screen gap, iris wipe rows): lines
-- y0..y1 show only a flat color. Hides the layer and sprites there, sets the
-- backdrop, and puts the old backdrop back on the next line.
function kit.mask_lines(y0, y1, c)
  local old = color(0)
  hdma(y0, y1, function(y)
    show.bg, show.sprites = false, false
    if y == y0 then color(0, c) end
  end)
  if y1 + 1 < display.height then
    hdma(y1 + 1, y1 + 1, function() color(0, old) end)
  end
end

-- Vertical mask (a window edge, a side bar): a grid of solid 16x16 front
-- sprites covering x..x+width-1 on lines y0..y1. pattern must be a solid
-- pattern in palette pal. Costs one sprite unit per 16 pixels of width on
-- every line it covers, and one SAT entry per 16x16 cell, so keep it small.
-- Returns the next free SAT index.
function kit.mask_columns(first, x, width, y0, y1, pattern, pal)
  local i = first
  for cx = x, x + width - 1, 16 do
    for cy = y0, y1, 16 do
      if i > 63 then error("kit.mask_columns: out of sprites", 2) end
      spr(i, cx, cy, pattern, { pal = pal or 0, front = true })
      i = i + 1
    end
  end
  return i
end
