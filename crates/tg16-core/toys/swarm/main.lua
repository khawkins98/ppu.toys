-- swarm: a flock of drones circling a beacon, to show the TG16's CPU.
--
-- The HuC6280 runs at 7.16 MHz, against the SNES 65816's 2.68 to 3.58 MHz.
-- Neither engine runs CPU code, so the difference shows in the budget
-- meters: 10_swarm.lua (the same file on both machines) does the flock's
-- work in Lua and returns how much of it there was, and this file charges
-- what hand-written HuC6280 code would take with cpu(). The SNES port in
-- compare/snes/swarm/ charges the same work at 65816 prices.
--
-- The flock grows from 12 to 52 drones and back every 24 seconds; the pair
-- checks grow with the square of that, to about 1300 a frame.

-- Estimated HuC6280 cycles for each piece of work, as hand-written code
-- (8-bit registers; positions are 8.8 fixed point in zero page / RAM tables):
local COST = {
  -- One pair check: lda x,y / sec / sbc x,x / abs / cmp #R / bcs (5+2+5+6+2+2),
  -- the same for y on the quarter of pairs that pass x, and the inner loop
  -- (iny / cpy / bne). About 40.
  pair = 40,
  -- A close pair: four 16-bit adds into the push accumulators.
  close = 70,
  -- One drone: steer (two compares, two 16-bit adds), add the pushes, clamp
  -- both speeds (signed 16-bit), move, bounce, pick a heading. About 300.
  drone = 300,
  -- Sprite band counting for the 16-per-line limit, per sprite tried.
  cull = 40,
}
-- (The engine itself charges 40 cycles per sprite for its SAT entry and
-- 1500 for vertical blank, so those are not repeated here.)

T = 128 -- first tile above a 64x32 BAT
P = 64  -- first sprite pattern (word 4096)

function init()
  bat.w, bat.h = 32, 32
  for k = 1, 3 do tile(T + k, SW.star(k)) end
  for ty = 0, 31 do
    for tx = 0, 31 do
      local h = (tx * 131 + ty * 197 + tx * ty * 7) % 61
      map(tx, ty, h < 3 and T + h + 1 or T, 0)
    end
  end
  color(0, rgb3(0, 0, 1))
  pal(0, 1, rgb3(3, 3, 4))
  pal(0, 2, rgb3(6, 6, 7))
  pal(0, 3, rgb3(4, 3, 6))

  for d = 0, 2 do pattern(P + d, SW.shape(d)) end
  pattern(P + 3, SW.orb())
  -- Four drone colours and the beacon.
  local DRONE = {
    { rgb3(1, 3, 6), rgb3(2, 5, 7), rgb3(6, 7, 7) },
    { rgb3(5, 1, 3), rgb3(7, 3, 4), rgb3(7, 6, 6) },
    { rgb3(1, 4, 2), rgb3(3, 6, 3), rgb3(6, 7, 5) },
    { rgb3(5, 3, 0), rgb3(7, 5, 1), rgb3(7, 7, 5) },
  }
  for p = 0, 3 do
    for c = 1, 3 do spal(p, c, DRONE[p + 1][c]) end
  end
  spal(4, 1, rgb3(4, 2, 0)); spal(4, 2, rgb3(7, 5, 1)); spal(4, 3, rgb3(7, 7, 6))
end

function frame(t, f)
  local n = SW.count(t)
  local pairs, close = SW.step(t, n)
  local bx, by = SW.beacon(t)
  spr(0, bx, by, P + 3, { pal = 4 })
  local vis = SW.visible(n, 16, f, by)
  for k, i in ipairs(vis) do
    spr(k, SW.X[i], SW.Y[i], P + SW.DIR[i], { pal = i % 4, flip_x = SW.FX[i], flip_y = SW.FY[i] })
  end
  cpu(pairs * COST.pair + close * COST.close + n * COST.drone + n * COST.cull)
end
