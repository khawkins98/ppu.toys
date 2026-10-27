-- swarm (SNES side of crates/tg16-core/toys/swarm)
--
-- The same flock as the TG16 toy: 10_swarm.lua is an identical copy, so the
-- drones move exactly as they do there. What differs is the CPU that would
-- run it. Neither engine runs CPU code, so this file prices the flock's work
-- as hand-written 65816 code and publishes the estimate for the comparison
-- page (through `sram.meter`, the one table ppu-core hands back to its host;
-- `sram.meter` is a readout, not a save).
--
-- How the estimate is made:
--   * Cycles in a frame: 262 lines x 1364 master clocks = 357,368, less DRAM
--     refresh (40 per line) = 346,888. A CPU cycle is 6 master clocks from
--     FastROM or I/O and 8 from WRAM or SlowROM; FastROM code working on
--     tables in WRAM averages about 7, so about 49,500 cycles a frame. That
--     is generous: a SlowROM game gets 43,400.
--   * Each kind of work is priced as the TG16 side prices it, written for
--     the 65816 instead (COST below). The 65816's 16-bit registers make the
--     per-drone update cheaper in cycles than on the 8-bit HuC6280; the pair
--     checks are 8-bit work and cost about the same. The SNES has to run
--     them at well under half the clock.
--   * Sprites: building an OAM entry, including the shared high-table bits
--     for X bit 8 and size, about 50 cycles. A line holds 34 eight-pixel
--     slivers, so 17 drones, nearly the TG16's 16: the same band counting
--     runs here too.
--   * Over 100% the frame does not fit: a real SNES would miss vertical
--     blank and slow the game down. This engine draws every frame anyway.

local SNES_CYCLES = 49500
local COST = {
  pair = 33,    -- lda abs,y / sec / sbc abs,x / abs / cmp / bcs (8-bit), y on a quarter, loop
  close = 55,   -- four 16-bit adds into the push accumulators (16-bit A)
  drone = 210,  -- steer, add pushes, clamp, move, bounce, heading (16-bit A)
  cull = 34,    -- band counting per sprite tried
  oam = 50,     -- one OAM entry with its high-table bits
  base = 2200,  -- NMI, OAM DMA (544 bytes, ~620 cycles of stall), frame set-up
}

local function tile4(addr, px)
  for y = 0, 7 do
    local p0, p1, p2, p3 = 0, 0, 0, 0
    for x = 0, 7 do
      local c = px[y * 8 + x + 1]
      local bit = 1 << (7 - x)
      if c & 1 ~= 0 then p0 = p0 | bit end
      if c & 2 ~= 0 then p1 = p1 | bit end
      if c & 4 ~= 0 then p2 = p2 | bit end
      if c & 8 ~= 0 then p3 = p3 | bit end
    end
    vram[addr + y] = p0 | (p1 << 8)
    vram[addr + 8 + y] = p2 | (p3 << 8)
  end
end
-- A 16x16 drawing (256 indices) as OBJ tiles t, t+1, t+16, t+17.
local function sprite16(base, t, px)
  for cy = 0, 1 do
    for cx = 0, 1 do
      local cell = {}
      for y = 0, 7 do
        for x = 0, 7 do cell[y * 8 + x + 1] = px[(cy * 8 + y) * 16 + cx * 8 + x + 1] end
      end
      tile4(base + (t + cy * 16 + cx) * 16, cell)
    end
  end
end
local function c3(r, g, b) -- the TG16 side's 3-bit colours, for the same look
  return rgb(floor(r * 255 / 7 + 0.5), floor(g * 255 / 7 + 0.5), floor(b * 255 / 7 + 0.5))
end

local MAP, CHR, OBJ = 0x0000, 0x1000, 0x4000

function init()
  sram.meter = nil
  for k = 1, 3 do tile4(CHR + k * 16, SW.star(k)) end
  for ty = 0, 31 do
    for tx = 0, 31 do
      local h = (tx * 131 + ty * 197 + tx * ty * 7) % 61
      vram[MAP + ty * 32 + tx] = h < 3 and h + 1 or 0
    end
  end
  cgram[0] = c3(0, 0, 1)
  cgram[1] = c3(3, 3, 4); cgram[2] = c3(6, 6, 7); cgram[3] = c3(4, 3, 6)

  for d = 0, 2 do sprite16(OBJ, d * 2, SW.shape(d)) end
  sprite16(OBJ, 6, SW.orb())
  local DRONE = {
    { c3(1, 3, 6), c3(2, 5, 7), c3(6, 7, 7) },
    { c3(5, 1, 3), c3(7, 3, 4), c3(7, 6, 6) },
    { c3(1, 4, 2), c3(3, 6, 3), c3(6, 7, 5) },
    { c3(5, 3, 0), c3(7, 5, 1), c3(7, 7, 5) },
  }
  for p = 0, 3 do
    for c = 1, 3 do cgram[128 + p * 16 + c] = DRONE[p + 1][c] end
  end
  cgram[128 + 64 + 1] = c3(4, 2, 0); cgram[128 + 64 + 2] = c3(7, 5, 1); cgram[128 + 64 + 3] = c3(7, 7, 6)
end

local function pct(x) return floor(x * 100 + 0.5) .. "%" end

function frame(t, f)
  mode = 1; brightness = 15
  screen.main.bg1 = true; screen.main.obj = true
  bg[1].char_base = CHR; bg[1].map_base = MAP
  bg[1].scroll.y = -1
  obj.char_base = OBJ; obj.size_sel = 0 -- small 8x8, large 16x16

  local n = SW.count(t)
  local pairs, close = SW.step(t, n)
  local bx, by = SW.beacon(t)
  for i = 0, 127 do obj[i].on = false end
  local o = obj[0]
  o.x, o.y, o.tile, o.pal, o.large, o.on, o.prio = bx, by, 6, 4, true, true, 2
  local vis = SW.visible(n, 17, f, by)
  for k, i in ipairs(vis) do
    o = obj[k]
    o.x, o.y, o.tile, o.pal = SW.X[i], SW.Y[i], SW.DIR[i] * 2, i % 4
    o.flip_x, o.flip_y, o.large, o.on, o.prio = SW.FX[i], SW.FY[i], true, true, 2
  end

  local cycles = COST.base + pairs * COST.pair + close * COST.close + n * COST.drone
    + n * COST.cull + (#vis + 1) * COST.oam
  local share = cycles / SNES_CYCLES
  sram.meter = {
    rows = {
      { label = "CPU (65816 estimate)", value = pct(share) .. " (" .. cycles .. " of " .. SNES_CYCLES .. ")", frac = share },
      { label = "Drones / pair checks", value = n .. " / " .. pairs },
    },
    warn = share > 1 and "over a frame: a real SNES would slow down here (not emulated)" or nil,
  }
end
