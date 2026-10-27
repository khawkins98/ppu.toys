-- 10_copper.lua: the copper stripes, the same on both machines.
--
-- This file is identical in crates/tg16-core/toys/copper/ and
-- compare/snes/copper/. Fourteen 16-pixel stripes run down the screen; stripe
-- i on line y shows step (y + CU.offset(i, t)) of one 256-step colour cycle,
-- so every stripe slides its colours at its own speed and direction.

CU = {}
CU.N = 14          -- stripes
CU.X0 = 16         -- first stripe's left edge; each is 16 pixels wide

-- Step s (0..255) of the colour cycle as 0..1 channels: three sines a third
-- of a turn apart, with a slower brightness swell.
function CU.colour(s)
  local a = s * 2 * pi / 256
  local v = 0.62 + 0.38 * sin(a * 2 + 0.5)
  return v * (0.5 + 0.5 * sin(a)), v * (0.5 + 0.5 * sin(a + 2.09)), v * (0.5 + 0.5 * sin(a + 4.19))
end

-- Stripe i's offset into the cycle at time t (integer).
function CU.offset(i, t)
  local speed = ((i * 5) % 7 - 3) * 22 + (i % 2 == 0 and 9 or -13)
  return floor(i * 37 + t * speed) % 256
end
