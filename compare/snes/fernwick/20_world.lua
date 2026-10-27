-- 20_world.lua: Fernwick's map, enemies and music. Shared, unchanged, by
-- the TG16 toy (crates/tg16-core/toys/fernwick) and the SNES toy
-- (compare/snes/fernwick); build_art.py copies it across and a test checks
-- the copies match.
--
-- A room is 16 x 12 blocks of 16x16 pixels (256 x 192). Map characters:
--   .  ,  *  _   grass, tufts, flowers, path        b  bush (cut it)
--   A B / C D    a tree: canopy tops (walk under them) over the trunk
--   r rock   w cliff   o cave mouth   ~ water
--   # dungeon wall   ! torch   : floor   s stairs out   k key pedestal
--   S shrine wall   = locked door   + shrine floor   x the Sunstone's altar

W = {}

W.SOLID = { C = true, D = true, b = true, r = true, w = true, ["~"] = true,
            ["#"] = true, ["!"] = true, S = true, ["="] = true }

-- The overworld is 3 x 2 rooms: room id = 1 + gx + gy * 3. Room 7 is the cave.
W.GRID_W, W.GRID_H = 3, 2
W.CAVE = 7

W.rooms = {
  { name = "WILDWOOD", rows = {
    "wwwwwwwwwwwwwwww",
    "wwwwwwwowwwwwwww",
    "AB.,...__...b.AB",
    "CD..b..__.,...CD",
    "AB.....__.......",
    "CD.r...__..,....",
    "AB..,..________.",
    "CD.........,....",
    "AB..*..__...b.AB",
    "CD.....__.....CD",
    "ABABAB.__.ABABAB",
    "CDCDCD.__.CDCDCD",
  }, foes = { { "blob", 4, 4 }, { "blob", 11, 8 } } },
  { name = "MEADOW", rows = {
    "ABABABABABABABAB",
    "CDCDCDCDCDCDCDCD",
    "AB..,....*...,AB",
    "CD..bb.....b..CD",
    "................",
    "..,..*....,.....",
    "....b.....bb....",
    "......,.........",
    "AB.*....,....*AB",
    "CD....b.......CD",
    "ABABAB....ABABAB",
    "CDCDCD....CDCDCD",
  }, foes = { { "blob", 5, 5 }, { "blob", 11, 8 }, { "beetle", 8, 3 } } },
  { name = "STILLWATER", rows = {
    "ABABABABABABABAB",
    "CDCDCDCDCDCDCDCD",
    "AB....~~~~~~..AB",
    "CD...~~~~~~~~.CD",
    "....~~~~~~~~~.AB",
    "..,.~~~~~~~~~.CD",
    "....~~~~~~~~..AB",
    "......~~~~~...CD",
    "AB.*.......,..AB",
    "CD....r.......CD",
    "ABABAB....ABABAB",
    "CDCDCD....CDCDCD",
  }, foes = { { "wisp", 3, 8 }, { "wisp", 12, 8 } } },
  { name = "FERN GLADE", rows = {
    "ABABAB.__.ABABAB",
    "CDCDCD.__.CDCDCD",
    "AB.....__.....AB",
    "CD..*..__.b...CD",
    "AB.....__.......",
    "CD.....________.",
    "AB..,.........,.",
    "CD.......,......",
    "AB.b........*.AB",
    "CD....,.......CD",
    "ABABABABABABABAB",
    "CDCDCDCDCDCDCDCD",
  }, foes = { { "blob", 11, 7 } } },
  { name = "CROSSROADS", rows = {
    "ABABAB....ABABAB",
    "CDCDCD....CDCDCD",
    "AB...r.__.r...AB",
    "CD.....__.....CD",
    "..b....__...b...",
    "________________",
    "________________",
    "..,....__..,....",
    "AB.r...__...r.AB",
    "CD..*..__..*..CD",
    "ABABABABABABABAB",
    "CDCDCDCDCDCDCDCD",
  }, foes = { { "beetle", 4, 7 }, { "beetle", 11, 3 }, { "blob", 9, 8 } } },
  { name = "OLD SHRINE", rows = {
    "ABABAB....ABABAB",
    "CDCDCD....CDCDCD",
    "AB............AB",
    "CD..SSSSSSSS..CD",
    "....S++x+++S..AB",
    "....S++++++S..CD",
    "....S++++++S..AB",
    "....SSS=SSSS..CD",
    "AB.....__.....AB",
    "CD.*...__..,..CD",
    "ABABABABABABABAB",
    "CDCDCDCDCDCDCDCD",
  }, foes = { { "wisp", 13, 2 }, { "beetle", 2, 8 } } },
  { name = "HOLLOW CAVE", dark = true, rows = {
    "###!######!#####",
    "#::::::::::::::#",
    "#::##::::::##::#",
    "#::##::k:::##::#",
    "#::::::::::::::#",
    "#::::::::::::::#",
    "#::::::::::::::#",
    "#::##::::::##::#",
    "#::##::::::##::#",
    "#::::::::::::::#",
    "#::::::::::::::#",
    "#######s########",
  }, foes = { { "beetle", 2, 5 }, { "beetle", 13, 6 }, { "blob", 5, 9 }, { "blob", 10, 9 },
              { "wisp", 7, 6 } } },
}

-- Where things are: cave mouth and stairs, the key and the Sunstone.
W.START = { room = 4, x = 7 * 16, y = 7 * 16 - 3 }
W.CAVE_IN = { room = 1, col = 7, row = 1 }      -- step into it from below
W.CAVE_ARRIVE = { x = 7 * 16, y = 10 * 16 - 3 }
W.CAVE_OUT = { col = 7, row = 11 }
W.CAVE_RETURN = { x = 7 * 16, y = 2 * 16 + 1 }
W.KEY = { room = 7, col = 7, row = 3 }
W.STONE = { room = 6, col = 7, row = 4 }
W.DOOR = { room = 6, col = 7, row = 7 }

-- Enemy types: hit points, contact damage (half hearts), speed.
W.FOES = {
  blob = { hp = 1, dmg = 1, speed = 0.5 },
  beetle = { hp = 2, dmg = 2, speed = 0.75 },
  wisp = { hp = 1, dmg = 1, speed = 0.7 },
}

-- Music: one 16th-note step every `tempo` frames. Tracks are space
-- separated tokens: a note (C4, F#5), "." holds, "-" stops; drums are
-- K (kick), S (snare), H (hat). All original tunes.
W.MUSIC = {
  overworld = { tempo = 8, loop = true, tracks = {
    "D5 . . A4 . . D5 E5 F#5 . G5 . A5 . . . " ..
    "B5 . A5 . G5 . F#5 . E5 . . D5 E5 . . . " ..
    "F#5 . . D5 . . F#5 G5 A5 . B5 . C#6 . . . " ..
    "D6 . C#6 . A5 . F#5 . E5 . . . - . . . " ..
    "G5 . . B5 . . A5 G5 F#5 . . A5 . . D5 . " ..
    "E5 . F#5 . G5 . E5 . C#5 . . . - . . . " ..
    "D5 . F#5 . A5 . D6 . C#6 . B5 . A5 . G5 . " ..
    "F#5 . E5 . D5 . . . - . . . - . . . ",
    "D3 . . . A2 . . . D3 . . . A2 . . . " ..
    "G2 . . . D3 . . . A2 . . . A2 . C#3 . " ..
    "D3 . . . B2 . . . D3 . . . F#2 . . . " ..
    "G2 . . . A2 . . . A2 . . . A2 . . . " ..
    "G2 . . . G2 . . . D3 . . . D3 . . . " ..
    "E2 . . . E2 . . . A2 . . . A2 . . . " ..
    "D3 . . . F#2 . . . G2 . . . A2 . . . " ..
    "D3 . . . A2 . . . D2 . . . - . . . ",
    "K . H . S . H . K . H . S . H H ",
  } },
  cave = { tempo = 10, loop = true, tracks = {
    "A4 . . . C5 . . . B4 . . . E4 . . . " ..
    "A4 . . . D5 . . . C5 . B4 . A4 . . . " ..
    "F4 . . . A4 . . . G#4 . . . E4 . . . " ..
    "A4 . . . . . . . - . . . - . . . ",
    "A2 . . . . . . . E2 . . . . . . . " ..
    "F2 . . . . . . . E2 . . . . . . . " ..
    "D2 . . . . . . . E2 . . . . . . . " ..
    "A2 . . . . . . . A2 . . . - . . . ",
    "K . . . . . . . H . . . . . H . ",
  } },
  title = { tempo = 9, loop = true, tracks = {
    "D5 . . . A4 . . . D5 . E5 . F#5 . . . " ..
    "G5 . . . F#5 . . . E5 . D5 . E5 . . . " ..
    "B4 . . . D5 . . . G5 . F#5 . E5 . . . " ..
    "F#5 . . . E5 . . . D5 . . . - . . . ",
    "D3 . . . . . . . A2 . . . . . . . " ..
    "G2 . . . . . . . A2 . . . . . . . " ..
    "G2 . . . . . . . E2 . . . A2 . . . " ..
    "D3 . . . A2 . . . D2 . . . - . . . ",
    "K . . . H . . . S . . . H . . . ",
  } },
  win = { tempo = 7, loop = false, tracks = {
    "D5 F#5 A5 D6 . . A5 D6 . . . . . . . . E6 . . . F#6 . . . . . . . - . . . ",
    "D3 . . . . . . . A2 . . . . . . . A2 . . . D3 . . . . . . . - . . . ",
    "K . . . S . . . K . . . S . . . K . K . S . . . . . . . - . . . ",
  } },
  gameover = { tempo = 10, loop = false, tracks = {
    "A4 . G4 . F4 . E4 . D4 . . . . . . . - . . . ",
    "D3 . . . A2 . . . D2 . . . . . . . - . . . ",
    "K . . . . . . . S . . . . . . . - . . . ",
  } },
}
