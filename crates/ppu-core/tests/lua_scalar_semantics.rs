use ppu_core::LuaEngine;

#[test]
fn signed_integer_division_and_modulo_follow_lua_without_overflow() {
    let mut e = LuaEngine::new();
    e.set_source(
        r#"
function frame()
  local a, b = -7, 3
  assert(a//b == -3 and a%b == 2)
  assert(b//a == -1 and b%a == -4)
  assert(-7//3 == -3 and 7//-3 == -3)
  local min = -9223372036854775807 - 1
  assert(min//-1 == min and min%-1 == 0)
  local max = 9223372036854775807
  assert((max-1)%max == max-1)
  assert((min+1)%min == min+1)
end
"#,
    )
    .unwrap();
    e.frame(0., 0).unwrap();
}

#[test]
fn parallel_assignment_snapshots_values_and_indexed_targets() {
    let mut e = LuaEngine::new();
    e.set_source(
        r#"
local up = 7
function values() return 4, 8, 12 end
function frame()
  local a, b = 3, 5
  a, b = b, a
  assert(a == 5 and b == 3)
  local t, i = {}, 1
  i, t[i] = 2, 42
  assert(i == 2 and t[1] == 42 and t[2] == nil)
  a, b, up = values()
  assert(a == 4 and b == 8 and up == 12)
  a, b = 9
  assert(a == 9 and b == nil)
  a, a = 1, 2
  assert(a == 1)
  t.x, t.x = 1, 2
  assert(t.x == 1)
  local old = t
  t, t.x = {}, 33
  assert(old.x == 33 and t.x == nil)
  g, h = 10, 20
  g, h = h, g
  assert(g == 20 and h == 10)
end
"#,
    )
    .unwrap();
    e.frame(0., 0).unwrap();
}

#[test]
fn parameters_a_caller_leaves_out_are_nil_not_stale_registers() {
    let mut e = LuaEngine::new();
    e.set_source(
        r##"
local function second(a, b) return b end
local function third(a, b, c) return c end
local function va(a, b, ...) return b, select("#", ...) end
local mt = { __index = function(t, k, extra) return extra end }
function frame()
  -- Fill registers above the call with live values first.
  local t = {1, 2, 3}
  local n = 2
  assert(second(t[1]) == nil)
  assert(second("A " .. n .. " B") == nil)
  assert(third(t[1], t[2]) == nil)
  -- A call whose result is kept in a register.
  local x = second(t[3])
  assert(x == nil)
  -- Fixed parameters before varargs.
  local b, count = va(t[1])
  assert(b == nil and count == 0)
  -- A generic-for iterator gets two arguments; a third parameter is nil,
  -- even with live locals in the loop body above the call.
  local seen = 0
  local function iter(s, c, extra)
    assert(extra == nil)
    if c < 3 then return c + 1 end
  end
  for i in iter, nil, 0 do
    local p, q, r = "x", {}, i * 2
    seen = seen + 1
  end
  assert(seen == 3)
  -- A metamethod called with fewer arguments than it declares.
  assert(setmetatable({}, mt).missing == nil)
end
"##,
    )
    .unwrap();
    for f in 0..3 {
        e.frame(f64::from(f) / 60., f).unwrap();
    }
}
