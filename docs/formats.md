# Data formats

Notes on the file formats used by Erudite's *Great Battles of History* engine,
and the rules as its games implement them. The sections up to *Hannibal and
Caesar* are verified against *The Great Battles of Alexander* (1997, patch
1.3, GOG); Hannibal's and Caesar's differences (GOG releases) follow there,
then the rule options, the *Collector's Edition* scenario editor's files,
and opengboh's own folder. All values little-endian. Status: ✅
understood, 🟡 partial, ❓ not yet looked at.

## EIF: sprite containers ✅

Used for unit sprites, terrain tiles, UI art and fonts (232 of 233 `.EIF` files).

```text
file   := title[128] u16 unk_a u16 unk_b strip*
strip  := name[128] u16 cel_count u32 start u32 end cel{cel_count}
cel    := name[128] u16 width u16 height u16 hot_x u16 hot_y u16 kind
          u32 size u32 prev_end u32 end pixels[size]
```

* Name fields are NUL-terminated; the tools reused buffers so bytes after
  the NUL are stale (e.g. strip names `N\0rip #1`). Padding byte is `0xEB`.
* Unit sprites have one strip per facing (`N`, `NE`, `E`, ...); cels are
  animation frames. `hot_x/hot_y` is the anchor point (feet).
* `unk_a`, `unk_b`, and the strip/cel `start`/`end` offsets are not needed to
  parse and are not yet understood.

Pixel encodings by `kind`:

| kind | encoding | seen in |
|---|---|---|
| 1 | raw, rows padded to 4 bytes | backgrounds, status bar |
| 2 | raw, padded | buttons |
| 3 | raw, padded | most unit sprites |
| 5 | raw, padded | bitmap font |
| 4, 6 | row RLE | arrows, terrain, large art |

Raw cels are 8-bit palette indices, top-down, index 0 = transparent. In unit
sprites (kind 3) index 255 marks shadow pixels (only found at the feet, below
the hotspot), drawn through remap table 0 (confirmed for tree shadows).

RLE rows: `u16 row_len` (including itself), then `u16` commands, op in the top
4 bits and pixel count in the low 12:

| op | meaning | data bytes |
|---|---|---|
| 1 | skip (transparent) | 0 |
| 2 | literal pixels | count |
| 3 | effect span 🟡 | 0 |
| 4 | effect span 🟡 | 0 |

Ops 3 and 4 appear under arrows, javelins and some units; most likely shadow
and/or translucency via a remap table. To confirm from the renderer.

`WALKANIM.EIF` ("Walking Animation Data") is *not* an image container: it holds
how each man of each kind of unit walks (`gboh_formats::walkanim`; loaded
by `FUN_004f7eeb`, records by `FUN_0052c5a7`, `FUN_0052c316`, `FUN_0052bea7`):

```text
file    := title[128] i16 kind_count i16 record_count kind*
kind    := name[128] i16 count i32 unknown u32 next_kind_offset record{count}
record  := name[128] facing{8}
facing  := step{10} i16 x i16 y
step    := leg{3}
leg     := (i16 x, i16 y){3} i32 ms{3}
```

Kinds: `Foot`, `Mounted`, `Companion`, `Thessalian`, `Leader`, `Elephant`,
`Chariot`, `Phalanx`, `Oxybeles`, each with one record per man of the
formation plus one. A facing's point is the man's place (zoom-in pixels;
the first man on foot facing NE is at (−22, −6), as in the formation table);
its ten steps are by the screen direction of the step, each with three
variants of up to three waypoints and the milliseconds to reach each. The
presentation walks each man along these (`FUN_004ee88a`, on the move
command `0x1c`), multiplying the times by 1, or when routed by a factor per
class (`0x60f8e0`: 0.67 for foot and phalanx, 0.9 mounted, 0.5 elephants, 1
otherwise). A plain step (the first variant) takes about 975 ms on foot,
1083 ms for the phalanx, 609 ms mounted, 487 ms for a leader, 808 ms for
elephants, 696 ms for chariots and 1975 ms for the oxybeles. In the plain
variant the first waypoint is relative to the hex being left (toward its
edge) and the second is the man's place in the next hex, relative to it;
the man faces the step's direction (the walking strip `base + direction`).
The rewrite times each walk from these (the slowest man's step, averaged over
the path) and walks each man along his plain variant (the places match the
formation tables exactly for foot, horse and phalanx; elephants and
chariots differ by a few pixels and use the data's own). The variant is
chosen by the two hexes' levels (`FUN_004ee88a`, the landscape's level call
`0xd4` on the hex left and the next): the first on the level, the second
going down, the third going up; the last two have three waypoints with a
250 ms middle leg. The rewrite does the same, timing each step by its
variant; 🟡 which hex the middle waypoint is measured from is inferred from
the data (going down the hex left, going up the next).

## GAME.PAL: palette ✅

```text
file := name[15] ("Erudite Palette", no NUL), RGBQUAD[256], remap[5][256]
RGBQUAD := u8 b, u8 g, u8 r, u8 0
```

Indices 0-9 and 246-255 are the standard Windows static colours.

**Water animation.** During battles the original's palette animator
overrides indices 202–210 with its own colours and cycles them through four
frames, so the `GAME.PAL` values for 202, 205, 207, 208 and 210 are never seen
on the battlefield (202 is white in the file but light blue in game; it is
used for the foam dots along river banks). 203, 204, 206 and 209 are set once,
to their `GAME.PAL` colours. Per frame (RGB):

| index | frame 0 | frame 1 | frame 2 | frame 3 |
|---|---|---|---|---|
| 202 | 150,190,235 | 145,175,215 | 140,160,195 | 145,175,215 |
| 205 | 85,115,145 | 70,100,130 | 75,105,135 | 80,110,140 |
| 207 | 70,100,130 | 75,105,135 | 80,110,140 | 85,115,145 |
| 208 | 80,110,140 | 85,115,145 | 70,100,130 | 75,105,135 |
| 210 | 75,105,135 | 80,110,140 | 85,115,145 | 70,100,130 |

The four water colours are close together, so rivers look almost flat in
game even though the `River` cels have ripple texture. The animator steps
five times a second (its timer callback is registered at rate 5 by
`FUN_004249cd`).

The animator works through `AnimatePalette` (`FUN_00424934`), which only
changes the screen on a 256-colour palette display. On a true-colour display
(Wine, and likely any modern Windows) it has no visible effect and the water
keeps its `GAME.PAL` colours, with their stronger ripple contrast: every
capture of the original under Wine shows indices 205/207–210 at their file
values. The rewrite animates, as a 256-colour display of the time would;
`gboh-tools render … static-water` renders with the plain `GAME.PAL` for
comparing with captures.

The five remap tables map a palette index to another index:

| table | effect (mean luminance ratio) | likely use |
|---|---|---|
| 0 | ×0.74, darker | shadow (index 255 in kind 3 sprites, confirmed; probably also RLE ops 3/4) |
| 1 | ×0.86, darker | lighter shadow / translucency |
| 2 | ×1.39, brighter | highlight |
| 3 | ×1.19, brighter | highlight |
| 4 | ×1.31, brighter | highlight |

## Executable resources (`alex.exe`) 🟡

`gboh_formats::pe` reads the executable's string table (`RT_STRING`) and
dialog templates (`RT_DIALOG`, all of class `EruditeDialog`, font MS Sans
Serif 8). A dialog unit is 7/4 pixels across and 13/8 down: `STARTGAME`
(367 × 295 units) fills the 640 × 480 `Start Game` bitmap and `SCENARIO`
(229 × 243) the 405 × 396 `Campaign Map` one (`DIALOG.EIF`); 90-unit buttons
use `DLGBTN.EIF`'s `Big Pushbutton`, 50-unit ones its `Pushbutton`.

**Choosing a scenario** (`FUN_0059d56b`): each battle, in the scenario table's
order, has a clickable place on the map (pixels): Chaeronea (38, 80) 79 × 11,
Lyginus (50, 38) 56 × 11, Pelium (29, 55) 61 × 11, Granicus (64, 67) 67 × 11,
Issus (119, 86) 41 × 11, Gaugamela (162, 88) 79 × 11, Samarkand (242, 41)
82 × 11, Jaxartes (274, 30) 67 × 11, Arigaeum (283, 64) 68 × 11, Hydaspes
(297, 76) 70 × 11, Paraitacene (211, 135) 91 × 11. Names are strings 93–102
and 80; descriptions 127–142 and 81–82 (one or two lines each). The
`CAMPAIGN.EIF` `Battles` cels are the names painted on the map (exactly, 9 or
10 px into the place for the western battles, 1–2 px for the eastern, 2 px
down).

**Who plays and the rout levels.** `INTRODLG` ("Leader Control") names the two
armies from the nation strings 31–36 (Greek, Macedonian, Persian, Danubian,
Indian, Scythian), which run in a different order from the scenario's nation
bytes (0 Greek, 1 Persian, 2 Macedonian, 3 Indian, 4 Danubian, 5 Scythian).
`ARMYROUTDLG` (`on_RoutLevels` → `FUN_004a2d78`, class `ROUTDLG.CPP`) shows
each army's rout level in a spinner (the scenario's, e.g. Issus 135 and 135)
and the home army's difficulty: in the original Easy gives the home army a
quarter more and the visitor a quarter less (rounded down: 168 and 101),
Hard the reverse, Normal the scenario's, Custom leaves the spinners to set
(measured in game; the code doing it is not traced). The viewer's setup
screens (`gboh-viewer` with no file) follow these;
🟡 how the original marks the chosen battle is not traced (the viewer shows
its name in yellow), and the font's second colour (index 252) is left out.

## Battle window bars 🟡

The toolbar (`FUN_0051eab8`) has twenty 24 × 22 buttons from `BUTTONS1`–`3.EIF`
(four cels each: up, down, disabled, disabled down), centred at x 17 (File
Open), 45 (Finish Leader, art `NextPhase`), 70 (Centre On Leader), 97, 122
(rotate left, right), 147 (About Face), 171 (Missile Volley), 195 (Restore
Cohesion), 219 (Rally), 243 (Unit Details), 272 (Overview), 296 (Hex Grid),
320 (Reverse View, art `Rotate180`), 348, 373, 397 (Zoom In, Normal, Zoom
Out), 425, 449, 475 (Tile Vertical, Tile Horizontal, Cascade) and 502 (Undo),
in a 30 pixel marble strip (measured on the original). The status bar is
`ALEXSB.EIF`'s `sbArt` (left cap, a 294 pixel middle that tiles, right cap,
a 36 pixel divider), 50 pixels tall, in three panes. The overview map is the
battle's strip of `OVERVIEW.EIF` (cel 0 the map on a scroll, cel 1 the same
for the reversed view, cel 2 the bare map, which lies 30 × 26 pixels into the
scroll; `Custom` is a placeholder), at the top right with the units as dots
(home red, visitor blue) and the part in view outlined. The viewer draws all
three over the battlefield (the scroll's shadow left out); clicking the
overview map centres the view there and its button hides it; the buttons
it has no action for are greyed, and 🟡 its pane texts (status, turn,
zoom) are its own.

## Unit details window 🟡

`VIEWUNIT` (242 × 190 units, OK and Help) is drawn by `ViewUnitDlg`'s paint
(`FUN_005a0ac9`) over the `Unit Overview` bitmap (`DIALOG.EIF`) in bold MS Sans
Serif 8, labels from the string table with a colon (string 68): at x 60,
values at x 160, rows y 79 Used This Turn (Yes/No), 96 Condition
(Good/Routing), 113 Troop Quality, 130 Cohesion Hits, 147 Movement, 164 and
181 Allowance and Remaining (labels at x 83), 198 Size; at x 251 Missile
Information (y 79), Type, Range, Allowance, Remaining (y 96–147, values at x
322: Archer, Slinger, Javelin or Bolts, else N/A). The viewer draws it in the
same places with `MS8TEXT.EIF` (a bitmap MS Sans Serif 8 whose 93 glyphs
start at `"`) in black; 🟡 the unit's name at the top and "used this turn"
(moved, fired or done) are its own.

## Combat results dialogs 🟡

After a leader's shocks the original shows `OVERALLCOMBATRESULTS` (the
`Overall Combat Results` bitmap; a list box, id 101 at (23, 22) 91 × 48
units, of "Battle #n"); clicking a battle centres the map on it and
double-clicking opens `SHOWCOMBATRESULTS` (314 × 135 units, the `Combat Results`
bitmap with the column headings painted on), drawn by `FUN_0058c097`: from y
100, a text line apart (16 pixels in captures), attackers' names at x 10
(wrapped past 140 pixels, the rest at x 20 a line down) and their cohesion
hits this combat, in all and still to rout (not below 0) at x 170, 210 and
243; defenders' at x 290 (300), 445, 485 and 520. A name gets `* ` and/or
`** ` before it from two per-unit flags; captures show `*` on units that
routed (e.g. "* Agrian 1" with 0 to rout), so `**` is taken to be eliminated
(🟡). The viewer shows both after the fights have been animated (a second
click on a battle opens it, for the double-click), its own records of each
shock giving the rows.

## Game overview 🟡

At the end of a turn the original shows `GAMEOVERVIEWDLG` (307 × 156 units:
OK, and Army Details for each side) drawn by `FUN_0058f6de` on the `End of
Game` bitmap: each army's nation (home on the left) in `CHISTEXT.EIF` capitals
centred a quarter and three quarters across at y 30, and at x 40 and 330 from
y 75, a line apart, "Army Rout Level (RP): n", "Rout Points Lost: n" and "Rout
Points Remaining: n" (strings 75–77; remaining = level − lost). The chiselled
capitals have two colours, white and blue (index 252): the viewer cuts them
into the marble by lightening and darkening it through `GAME.PAL`'s remap
tables (🟡 which tables the original uses is not traced). `MS8TEXT.EIF` runs
from `!` with both brackets in one glyph (the `(` cel) and none for `)`.
The viewer shows the overview after the combat results at each turn's end;
🟡 Army Details is not wired.

## Running commentary 🟡

`INFODLG` ("Running Commentary", 164 × 47 units, no controls: a rich text box
filled by `AddCommentary` messages) sits in the battle window's bottom right
corner (254 × 110 pixels under Wine). Its sentences are in the executable's
data with mark-up for bold names (`\b…\b`), e.g. "X is now active.", "X
throws javelins at Y for n cohesion hits.", "Y is routing!", "Y is doing a
terminal rout!", "Y is eliminated!", after "--- Turn #n ---". The viewer keeps
its own commentary in that corner from the game's log (turns, the active
leader, volleys, attacks, hits, routs, eliminations), 🟡 in its own words.

## Army details 🟡

The Game Overview's Army Details buttons (102 home, on the left; 101
visitor) open `ARMYDETAILSDLG` (197 × 289 units, Close and Help, column heads
Name, TQ, Hits, Elim. and Rout Pts) on the `Army Details` bitmap (346 × 470).
Its paint (`FUN_00583de7`) writes in bold MS Sans Serif 8 "<nation> Army"
centred on x 170 at y 52, then at x 28 from y 76, 24 apart, strings 123–126:
the rout level, the percentage of it eliminated (eliminated capped at the
level), the TQ routing and the rout points eliminated (the same figure as the
overview's rout points lost). Below, a read-only rich edit at (16, 276), 309 ×
132, is filled by `FUN_005845f7`: for each unit its name in bold (cut to 23
characters), a tab, TQ, a tab, hits; then each leader with `-` for both; a
line out of the battle continues with a tab, `X`, a tab and its rout points.
The tab stops are 189, 216, 243 and 270 pixels. 🟡 What "TQ Routing" counts is
not traced; the viewer sums the TQ of the army's routing units.

## Dialogs during play 🟡

Several of the original's dialogs have no art of their own, just static
texts filled in at run time and buttons on the `EruditeDialog` background:
`MOMENTUMDLG` ("Would you like to attempt momentum?", Yes 1 / No 2, asked
when a leader who may try for momentum, `CanAttemptMomentum`, finishes),
`INITIATIVERESULTS` (its result in static 101), `RALLYDLG` (statics 101–103,
OK, Help), `MISSILERESULTS` (statics 101 and 102, OK, the check box 201
"Never show this again") and `LEADERALMOSTFINISHED` (all its text in the
template; Finish 101 and the check box 17 "Don't show this again", which is
also the Finish Leader Dialog preference). The viewer draws them from their
templates on the game's marble with a caption bar; 🟡 their run-time
sentences are in the executable's data, and the viewer words its own.

## End of game 🟡

When an army reaches its rout level, `FUN_004aeda7` starts the end music and
shows `ENDOFGAMEDLG` (271 × 289 units: Overview 101, View Field 102, Close),
class `EndOfGameDlg` (`FUN_0058ed4c`), whose paint draws nothing but its
bitmap from `EOGDLG.EIF` ("End Of Game Bitmaps"): `<People> Victory` (475 ×
470, "You are victorious") or `<People> Defeat` (460 × 470, "You have been
defeated"; the dialog is moved and narrowed 15 pixels to fit) for the
Macedonian, Persian, Greek, Danubian and Indian peoples. It is a victory when
a human played the winning side, or when no human played. Overview sends
`GameOverview`; View Field closes it to look over the field. 🟡 The
constructor loads the Macedonian pair and stores the other nation's name
without (as far as traced) using it; the viewer shows the player's own
nation (the Scythians with the Persian pictures).

## Reverse view

Map > Reverse View (command 223, the toolbar's Reverse View button) flips bit
4 of the battle view's flags (beside the zoom bits) and rebuilds the
landscape: the battlefield turned half round. The landscape's tile `i` (of
a map `w` hexes wide and `h` tall, `FUN_0045cc35`) is normally hex
`(i / h, h - 1 - i % h)`; reversed it is `x = w - 1 - i / h`,
`y = i % h - (x & 1)`. Every map is an odd number of columns wide, so a map
column's odd-ness pushes it down one tile: in both views the first tile of
an unshifted row has no hex, and the last hex of each odd column is not
shown. Directions turn with it: hex sides by three, screen facings by four
(`FUN_0044ea4d(f, 4)`), as in the tree and stone scatter's side checks.
Towns have their own reversed placement and use their art's second strip:
Chaeronea at hex 202 with offset (380, 51); Pelium at hex 602 with the 11
offsets of the table at `0x618334`. The overview uses the turned scroll
(`OVERVIEW.EIF` cel 1). Checked against the original at Chaeronea: the
units, stream, coast and town line up exactly; 🟡 the plain blocks' random
cels do not, as the original rebuilds the landscape with its random
generator wherever the game has left it. The viewer keeps the same spot in
the middle of the window and turns corpses already lying (their offsets
negated, dropped spears four steps).

## Hex grid 🟡

`HEXGRID.EIF` ("Hex Grid", zoom in), `HEXGRID2.EIF` (half size) and the
quarter-size art each hold 16 cels of a hex's right-hand edges, by the same
neighbour mask the terrain blocks use (bent where a neighbour stands higher);
the landscape layer keeps them with the battle arrows (`FUN_0050bbeb`). The
viewer's Hex Grid button draws, over the terrain and under the units, each
block's cel at its raised origin (🟡 the original's choice of cel is not
traced). The Missile Volley button (enabled while the selected unit can shoot)
arms a volley that the next click on an enemy fires.

## Other files ❓

Alexander's `DATA` (Hannibal's and Caesar's hold the same kinds; Caesar's
also the *Collector's Edition* editor's `.psf` packages and its unit
catalogue, `Scenario Editor/MainUnits.unt`, see the last sections):

| ext | count | notes |
|---|---|---|
| `.MAP` | 12 | battle maps, see below 🟡 |
| `.SCN` | 11 | scenarios, see below 🟡 |
| `.AI` | 45 | plain-text AI plans (`'` comments) |
| `EIFFILE.HED` | 1 | "Activation Unit Flags", 1.1 MB, structure unknown |
| `.WAV`, `.AVI` | | standard formats |
| `.RTF` | 23 | rich text: battle histories, unit listings (`U*.RTF`), credits; read by `gboh_formats::rtf` |
| `ALEX.HLP` | 1 | WinHelp 3.1 help file; read by `gboh_formats::hlp` |

## MAP: battle maps ✅ structure / 🟡 feature meanings

```text
file  := u16 name_len, name[name_len], u8 width, u8 height,
         u16 hex_id_origin, u16 unk_b, u8 elevation[width*height],
         cell[(width*2+2) * (height*4+1)]
cell  := u16 count, u16 feature[count]       (count <= 12)
```

All 11 battle maps (plus an unused `parait.map`, Paraitacene) parse with no
bytes left over. `EIFFILE.MAP` is a different file (resource index). Maps
saved by the *Collector's Edition* editor have a u32 0 and a 16-byte GUID
in front (see the last sections). The games' maps measure up to 89 hexes
wide (Trasimene) and 85 high (Ilipa).

* Hexes are in columns; **odd columns** are shifted down half a hex.
* `elevation`: 0 = water (sea/river), 1..5 = land levels. (The manual says major
  rivers count as level 1; the loader flags level-0 hexes specially.)
* The feature grid uses doubled coordinates: hex `(x, y)` is at cell
  `(2x + 2, 4y + 2 + 2*(x & 1))` (grid column 0 is unused). This was checked
  by drawing minor streams and matching them pixel for pixel against the
  running game; an earlier reading without the `+2` offsets put every
  feature one hex off. The original computes the column as `id / 50 + 2`
  (`id = 100x + y`, `FUN_004b1c4a`), so from `y = 50` on it is `2x + 3`;
  of Alexander's maps only Paraitacene (65 hexes tall) reaches that, and
  its woods there are only found with this rule (checked in game).
  Hannibal's Ilipa (85) and Caesar's 66-hex maps reach it too. Its six sides are the cells at offsets
  N `(0,-2)`, NE `(1,-1)`, SE `(1,1)`, S `(0,2)`, SW `(-1,1)`, NW `(-1,-1)`.
* Hex ids (used by scenarios) are `hex_id_origin + x*100 + y`; the origin is
  1000, or 3600 for Lyginus and Pelium. `unk_b` is mostly 1.

Feature codes. The map loader converts side codes 20/6/11 to side types
0/1/2. Hex codes 1, 2, 12 and 13 are confirmed by the tile builder
(`Isometric Hex Landscape`, `FUN_0044f689` in the original), which scatters
random "Trees" sprites for code 1 and "Stones" (or "Stones (Green)") for
code 2; code 12 is a road and code 13 river rocks (see Terrain rendering). The rest are inferred from where they occur, against the manual's
terrain chart (Clear, Woods, Rocky, Major River, Rocky Riverbed, Minor
Stream, City Walls):

| code | where | evidence | meaning |
|---|---|---|---|
| 1 | hex | tile builder places trees | Woods ✅ |
| 2 | hex | tile builder places stones | Rocky ✅ |
| 12 | hex | drawn with `Roads` (verified at Pelium) | Road ✅ |
| 13 | hex | random `River Impass` cel (rocks in water); roads join it | Rocky riverbed ✅ |
| 20 | hex | Issus coast, Samarkand, Hydaspes, Jaxartes (river/coast hexes) | ? not drawn: the tile builder ignores it, so it is game-logic only |
| 20 | side | along rivers, coasts, Chaeronea city edge | ? (bit 0) |
| 6 | side | Chaeronea streams, Lyginus | Minor stream ✅ (drawn with `Minor Rivers`) |
| 11 | side | 2 sides, Pelium only | Gate (tentative) |

## Terrain rendering ✅ blocks, streams, cliffs, edges, trees/stones, roads, river rocks (verified in game)

**Screen layout.** The battlefield is drawn rotated 90° from the map file: screen
row = map `x`, screen column = `height - 1 - y`. On screen each hex is a
pointy-top hexagon (apexes at top and bottom, vertical left/right sides), so
screen rows alternate their half-cell shift (from the odd map columns). Checked
against the pre-rendered battlefield pictures in `OVERVIEW.EIF`.

**Zoom levels.** Three views, each with its own art at a fixed scale:

| view | block w×h | top face h | row pitch | level step | blocks file | other art |
|---|---|---|---|---|---|---|
| Zoom In | 130×76 | 64 | 48 | 12 | `LAND.EIF` | `NLAND`/`NLANDG` |
| Normal (default) | 66×38 | 32 | 24 | 6 | `LAND2.EIF` | `ZLAND`/`ZLANDG` |
| Zoom Out | 34×19 | 16 | 12 | 3 | `LAND4.EIF` | `Z2LAND`/`Z2LANDG` |

Row pitch is ¾ of the top face; shifted rows move by half a block; each
elevation level raises a block by `block h - top face h`, which is also the
height of a bank. On screen, even rows sit half a block to the right of odd
rows.

**Blocks.** Each hex is one block. When the `LAND*.EIF` files ("Landscape
Blocks") are present, which they are in the shipped game, the original uses
them for every hex; the `NLAND`/`ZLAND` families ("Landscape Artwork") are then
only used for cliffs, streams, roads and fords. The `…G` ("Green") variants
replace the block and cliff strips and are chosen when the map's `unk_b` is 2.
Verified pixel-exact against the running game (Chaeronea, normal zoom): whole
hexes match exactly wherever the random variant agrees.

* Strips: `River`, `Land1`..`Land9`. Strip index = `offset + elevation`, where
  the offset depends on the battle: 0 if the map's `unk_b` is 2, else 1 for
  battles 0 and 1 of the scenario table (Chaeronea, Lyginus), 3 for battle 5
  (Gaugamela), 4 for battle 8 (Arigaeum), and 0 otherwise. The table order is
  Chaeronea, Lyginus, Pelium, Granicus, Issus, Gaugamela, Samarkand, Jaxartes,
  Arigaeum, Hydaspes, Paraitacene.
* `mask` = which of four neighbours is **exactly one level higher**:
  bit 1 = left, bit 2 = upper-left, bit 4 = upper-right, bit 8 = right (on screen).
* Per-mask cel counts, in strip order: `7,3,1,3,1,1,2,1,3,1,1,1,3,1,1,1` (31 cels,
  the whole `River` strip). Land strips have 2 extra cels in front (33 total).
* Cel choice, consuming `rand()` in this order:
  * if both upper neighbours are lower and `mask == 0`: `rand() % 2` (the two
    leading cels);
  * otherwise `start = 2 (land) or 0 (River) + sum(counts[..mask])`, then
    `k = rand() % counts[mask]`; for `mask == 0`, `k == 0` is replaced by
    `rand() % 3` and any other `k` by `3 + rand() % 4`. Cel = `start + k`.
* **Cliffs.** Drawn before each block, towards its lower-left and lower-right
  neighbours when they are two or more levels down. Strip `Cliff <lower
  elevation>`, cel `drop - 2`; cliff cels are three blocks wide and the hex
  uses the block-wide slice `index % 3` (index = row × columns + column), left
  half towards lower-left, right half towards lower-right. Placed at the
  block's top-left + (0 or w/2, ¾·block h + 2) (the block's full height,
  not its top face: checked against river banks at Issus and Samarkand,
  where the whole cliff face with its foam line shows, 79–90% pixel-exact). If the same-row neighbour on that
  side is exactly one level higher, the drop is +1 and the cliff moves up one
  level. Shifted rows in the last column (no lower-right) use the lower of
  lower-left and the hex two rows down for the right half; likewise column 0
  for the left half.
* **Map edges.** The visible map runs from x = w/2 to the right end of the
  shifted rows. In unshifted rows the column-0 hex has no tile at all; instead,
  when drawing column 1, the original draws the right half of a filler block at
  x − w/2, and after the last column the left half of one at x + w. The filler
  elevation `m` is the lowest of the hex and its two neighbours on that side
  (upper and lower; off-map neighbours count as 0/1/2 for In/Normal/Out), it is
  drawn `(z − m)` levels lower, its mask uses only the two bits facing those
  hexes, and its cel is deterministic: `start + index % count` (no `rand()`).
* **Background.** `LANDBACK.EIF` ("Landscape Background", one 100×100 tile) is
  tiled behind everything, with the tile origin at the top-left of block
  (0, 0) at elevation 0.
* **Streams** (side code 6, `Minor Rivers`). Each hex draws its own half of the
  stream on each of its sides, so a stream between two hexes is two strips.
  Side pieces (cels 0–5: left, upper-left, upper-right, right, lower-right,
  lower-left on screen = map S, SW, NW, N, NE, SE) and corner joins (cels
  6–14, three per corner, chosen by which of the two adjacent sides carry a
  stream) are placed at fixed per-zoom offsets from the block's top-left.
* **Objects** (verified in game at Lyginus, Normal zoom). Trees (hex feature 1)
  and stones (feature 2) are not tile art. They are separate sprites from
  `LANDOVER`/`LANDOVR2`/`LANDOVR4` (strips `Trees`, `Stones`, and `Stones
  (Green)` when `unk_b == 2`), created once per battle as physical objects,
  like units. Offsets are generated in zoom-in pixels with the face size
  `(W, H) = (130, 64)`, relative to a base of `(-W/2, 0)`:
  * Trees: `n = rand()%3 + 6`; for `i = 0..=n`, slot `k = i` (or
    `(rand()%6 + i) % 6` once `i > 5`); `dx = rand()%(W/3) + (k%3)·(W/3)`,
    `dy = rand()%(H/4) + (k/3)·(H/3)`, cel `rand()%7`. Then one more tree,
    cel 0, at `(0, H/3)` with no base offset. That makes 8–10 trees per hex.
  * Stones (only in hexes with elevation > 0; at elevation 0 the feature
    becomes a block overlay instead): `n = rand()%4 + 4`; for `i = 0..=n`:
    `dx = rand()%W + W/4`, `dy = rand()%(H/2)`, cel `rand()%7`.
  * Screen anchor at each zoom = the block's top-left (with elevation) +
    `(block_w/2, block_h/2)` + offset / 1, 2 or 4 (C division). The sprite is
    drawn at anchor − hotspot. Index 255 in these sprites is a shadow, drawn
    through remap table 0.
  * Tree density matches the game (49.5% vs 49.8% coverage over the same
    area), and the vertical coverage profile matches with no offset. The
    "Sparse Trees" preference is sent as `ToggleSparseLandscape`, but nothing
    in the executable handles it, so it appears to have no effect. Depth
    order between objects is not yet confirmed; the renderer sorts by anchor
    y. In blocks mode there are no random bushes.
* **Art groups.** The blocks landscape loads ten picture groups
  (`FUN_0045cd3e`): 0 `Bushes`, 1 `Minor Rivers` cels 0–5, 2 `Minor Rivers`
  cels 6–14, 3 `River Impass`, 4 `Scars`, 5 `Ford Fix`, 6 `Roads`, 7/8 two
  per-mask sets (grid/highlight outlines, hidden by default), 9 `Bushes
  (Green)`. Bushes, River Impass and Scars come from the zoom's object file
  (`LANDOVER`/`LANDOVR2`/`LANDOVR4`); Minor Rivers, Ford Fix and Roads come
  from the artwork file. Ford Fix is never drawn in blocks mode. Each tile
  draws its layer-0 slots in order: 0 bushes, 1 road, 2–7 stream pieces,
  8 river rocks.
* **Roads** (hex code 12; verified at Pelium, pixel-exact once the remap-3
  highlight is accounted for). `Roads` cels are block-sized with hotspot
  (0, 0) and are drawn at the block's top-left. A road enters from the
  upper-left, left or lower-left neighbour (1, 2, 3) and leaves to the
  upper-right, right or lower-right one (4, 7, 10). Cel = in + out − 6:
  0 left→UR, 1 LL→UR, 2 UL→right, 3 left→right, 4 LL→right, 5 UL→LR,
  6 left→LR.
  * In: the first of UL, left, LL with code 12; else the first with
    code 13; else the first missing neighbour among left, UL, LL; else
    left.
  * Out: UR (unless in = UL), then right, then LR (unless in = LL), taking
    the first neighbour with code 12 or 13. Failing that, a missing
    neighbour, checked in the order right, UR, LR, under the same
    exceptions; else right.
  * "Missing" includes the empty first tile of unshifted rows.
* **River rocks.** Hex code 13, and code 2 (stones) in elevation-0 hexes,
  add a random `River Impass` cel (block-sized, hotspot 0) at the block's
  top-left, drawn after the streams.
* **Towns** (verified in game: Chaeronea by eye, Pelium 94.5% of town pixels
  exact). Two battles add a town as physical objects, depth sorted with units
  and trees. Hex ids here are `x * 100 + y` (no map origin). The art is
  looked up by EIF title (`Chaeronea`, `Pelium`, plus the size suffix); its
  first strip is the normal view and the second the reversed view.
  * Chaeronea: one cel (strip 0, cel 0) on hex 16 = (0, 16), offset
    (−65, 79).
  * Pelium: 12 cels (strip 0, cels 0–11) on hex 103 = (1, 3), offsets
    (−119, 142), (−66, 63), (4, 53), (130, 81), (193, 152), (227, 191),
    (−196, 195), (−145, 242), (−66, 237), (64, 237), (−108, 170), (4, 131).
* A brightened hex area seen in game (e.g. around the active leader) is the
  terrain passed through `GAME.PAL` remap table 3; it is not terrain art.

**Superseded:** an earlier reading of the code had `NLAND` as the block art
with `rand() % 4` flat variants and `3 + mask` bank cels. That is the fallback
used only when `LAND*.EIF` is missing.

**Hex-side features.** The map loader turns side codes into a per-side bit:
code 20 → bit 0, code 6 → bit 1, code 11 → bit 2. Side code 6 is drawn with the
`Minor Rivers` art (cels 0–5: one per side; cels 6–14: corner joins), so code 6
is a **minor stream**, not a wall. Overlays (bushes, stones, trees) from
`LANDOVER.EIF` are scattered randomly (see Terrain rendering, Objects).

## SCN: scenarios ✅ structure / 🟡 some field meanings

From the loader (`ALEXLOG3.CPP`) and the per-class `Load` methods. All 11
scenarios (including the unused `parait.scn`) parse to the last byte, every
unit lands on its map, and the leader count matches the number of kind-0 units.
This is Alexander's layout; Hannibal's and Caesar's differ in the unit
records (see *Hannibal and Caesar*), and the *Collector's Edition* editor
writes a layout of its own (see the last sections). `gboh_formats::scn`
tells them apart by their bytes.

```text
file    := u16 n_leaders, leader[n_leaders], u16 unk (skipped by the game),
           str8 title, str8 map_file, str8 unit_file (skipped by the game),
           side home, side visitor,
           u8 n_pairs, (u8, u8)[n_pairs],
           u8 n_home_ai, u8 n_visitor_ai, str8 ai_file[n_home_ai + n_visitor_ai],
           unit* (until end of file)
leader  := u16 id, u16 len, name[len]
side    := u8 nation, u8 retreat, u8 rout_level
str8    := u8 len, char[len]
unit    := u8 kind, base, extra(kind)
base    := str8 name, u8 nation (0..5), u16 unk_a, u16 unk_b, u16 hex_id, u8 facing
```

* `hex_id` = map `hex_id_origin + x*100 + y`.
* `side`: the army's nation, the map direction its routed units flee in
  (1 N, 3 E, 5 S, 7 W) and its rout level, the rout points at which it
  withdraws (from the game's loader). Hydaspes: Macedonians `2 7 80` (flee
  west, withdraw at 80), Indians `3 3 70`.
* `facing` is a direction 1..8 (N, NE, E, SE, S, SW, W, NW) in map terms;
  scenarios only use 2, 3, 4, 6, 7, 8. On screen (the map turned a quarter)
  it becomes facing + 2, wrapping 8 to 1 (6 → NW, 7 → N, 3 → S). That gives
  the six screen facings N, NE, SE, S, SW, NW. (At Chaeronea the Macedonians
  face 6, NW on screen, towards the Greeks, who face 2/8, SE/NE.)
* All decoded unit values agree with the manual's unit table (pp. 70–76):
  `unk_a` = MA, size, TQ, missile weapon and range (the first missile value
  after the weapon; the oxybeles' range 7 is not stored). The last missile
  value (5 for javelins, 10 for bows and slings, 0 for some units) is still
  unknown.
* `extra` by original class. For combat units the first value is the unit's
  **size**, the second its **troop quality** and the byte after them is set
  for units that cover two hexes (phalanx, double-sized hoplites) (`Size:` and `TQ:` in game;
  Hypaspist 1 at Hydaspes, `6 8`, shows `TQ: 8 … Size: 6`):

| kind | class | extra fields |
|---|---|---|
| 0 | `LeaderUnitData` | u16 ×4 |
| 6, 9, 14, 15, 19 | `CombatUnitData` | u16, u16 (troop quality?), u8 |
| 2 | (Combat subclass) | combat + u8 |
| 1, 3, 4, 7, 8, 10, 17, 18 | `MissileCapableUnitData` | combat + u8 missile type (1..4), u16, u16 |
| 5 | `HeavyCavUnitData` | missile + u8 |

**Nation and missile bytes.** The game remaps both on load. Nation: 0 Greek,
1 Persian, 2 Macedonian, 3 Indian, 4 tribal (Danubian), 5 Persian allied.
Missile: 1 bow, 2 sling, 3 javelin, 4 bolt.

**Unit types.** The game turns `kind` + nation (+ missile, + flags) into one of
45 unit type ids. Each type names its sprite set by EIF title; the half and
quarter-size sets append " (Half size)" / " (Quarter size)". The full table is
`gboh_formats::units` (`unit_type`, `ART_TITLES`). Points worth knowing:

* Leaders (kind 0): a Macedonian leader whose name contains "Alexander" uses
  `Alexander`; "Darius III" uses `Darius`; others use the nation's leader set.
* Kind 2 is heavy cavalry; its trailing byte selects the cataphracted sets.
* Kinds 6/14 are hoplites; the combat `c` byte selects "double sized"
  (type 18) over "single sized" (31). Both use `Greek Hoplites` art but
  different formations.
* Kind 7 Persians with `c` != 0 are Cardaces.
* Kind 10 Macedonian javelinists whose name contains "Ag" are Agrianians.
* Kind 15 (hypaspists) uses `Macedonian Heavy Infantry`; kind 19 is the
  phalanx.

All 946 units in the 11 scenarios get a type, and every type used has its
three sprite sizes in `DATA`.

**Unit sprite sets** have a `Standing` strip with one cel per screen facing
(N, NE, E, SE, S, SW, W, NW), plus walking (`N`…`NW`), `Rout*`, `Drop*`,
combat and parts strips.

**Formations** ✅ (verified in game: unit pixels match exactly 90.6% at
Chaeronea, 90.7% at Granicus, 89.2% at Pelium and 91.7% at Hydaspes, where
the elephant line alone is 94.1%. Hydaspes also checks out at the other
zooms: 89.5% at Zoom Out and about 90.5% at Zoom In, leaving out a dialog
covering part of the capture. Most of the rest is shadows falling on
different random grass, the activation flags over units the current leader
can order (`AFLAG*.EIF`, not drawn yet) and the active leader, who carries a
large banner in game). A unit is drawn
as several figures. Each figure is a physical object like a tree, anchored at
its hex centre plus an offset in zoom-in pixels that depends on the class and
the screen facing (scaled by 1/2/4 like trees). Every figure uses the
`Standing` cel for the facing and is drawn at anchor − hotspot. The class
tables are in `gboh_render::units`:

| formation | figures | unit types |
|---|---|---|
| phalanx | 20, on two hexes | phalanx (17), double-sized hoplites (18), Cardaces (43) |
| infantry | 8, two ranks of four | most foot: javelinists, light and medium infantry, archers, slingers, hypaspists, single-sized hoplites |
| cavalry | 6 | all cavalry |
| elephants | 3 | Persian (24) and Indian (40) elephants |
| chariots | 3 | Persian (22) and Indian (23) chariots |
| oxybeles | 1, at (0, 45) facing N, else (0, 20) | oxybeles (38) |
| single | 1, at (0, 11) | leaders |

Infantry and cavalry only define the six screen facings. The phalanx
formation covers two hexes: figures 0–4 and 10–14 stand on the unit's hex and
5–9 and 15–19 on its flank neighbour, a quarter turn clockwise from the
facing on screen (for a unit facing NW, the hex to the NE). Each hex holds a
front and a back rank of five, so the two hexes form two long ranks.
**Activation flags** ✅ placement / 🟡 rule (`AFLAG*.EIF`: strips
`Macedonian Flag`, `Greek Flag`, `Persian Flag`, `Indian Flag` (three
animation cels each), `Flag Shadow`, `Stem`). Every non-leader unit gets a
flag when its sprites are built: the pole's anchor is the hex centre plus
(`5 − rand() % 11`, −12) zoom-in pixels, and the raised pennant hangs at
(+1, −82) from that anchor (both measured at Zoom In). Pennant by nation:
Macedonian, Persian (and allied), Indian, Greek (and tribal). The game shows
flags only on units the active leader can order (see Leaders and command).
The flag's ground shadow is not drawn yet, and the pennant's raising
animation is not modelled.

Infantry and phalanx figures also carry a per-facing draw depth. The renderer
sorts all objects by anchor row, then depth; the original's exact rule is
not confirmed.

## Missile volleys 🟡

A missile unit's presentation (`FUN_0052e6a0`, handler `FUN_0052ee77` on
`MissileFire`) animates every standing figure:

* **Shooting.** Each man plays his shooting strip for the facing (`MN`…`MNW`,
  names from the table at `0x627994`): cel 0, cel 1 for five frames, cel 2,
  cel 3 for six frames, then `Standing`.
* **Missiles.** Each man gets his own missile sprite from the set picked when
  the unit is built (`Arrows`, `Javelins`, `Bolt`, `Stone`, three sizes each,
  loaded by `FUN_004bfcfc` into `0x64fc28`). It shows the 1×1 `Default` cel
  while the man draws, then cel 0 of the strip for the facing (`N`…`NW`,
  table `0x6279b4`) on the way up and cel 1 on the way down (due south keeps
  `S` cel 0), then `Default` again. Its path runs from a per-direction
  launch point by the man, with `10 − rand() % 21` of scatter on each axis,
  over a midpoint lifted by a per-direction amount (e.g. (0, −20) north,
  (−5, −10) north-east), to the target.
* 🟡 The frames are moved by the original's sprite motion engine, whose
  timing is not traced. The rewrite releases the missiles on the release cel
  and flies them at a fixed speed (a fifth of a block per frame) along a
  parabola lifted by a quarter block plus a quarter of the distance, landing
  on random men of the target, and plays the volley after the moves queued
  before it.

## Shock combat animation 🟡

On `CombatAnimate` (`FUN_004e77de`) a unit marks the sides, relative to its
front, where enemies in contact stand (`FUN_004e4b1a` gives the direction to
each), works out each man's exposed side (`FUN_004e6d66`: of those sides, the
one his table entry is lowest for, if under his threshold), picks the men to
fall, and calls each living man's fight animation (`FUN_004eb617`, vtable
slot `0x68`). Only exposed men fight, turned toward their exposed side; the
others stand, unless they are among those who fall:

* **Strips.** The fight strips are `CN`…`CNW` where the art has them (light
  and medium infantry, hoplites, light cavalry), else the walking strips
  (the base set at `+0xf6` by `FUN_004e42d3`). Cels: 0–4 in and 4–0 out for
  the mounted classes (`+0x108` 8–12, `FUN_004eb5d9`), 0–3 and 3–0 for the
  phalanx (class 3), otherwise 1, 2, 3 in and 3, 3, 2, 2, 1, 1 out.
* **Motion.** After a pause of 2 + `rand() % 5` frames each man steps toward
  the enemy by a per-direction amount (zoom-in pixels, N … NW: (0, −10),
  (20, −12), (30, 0), (20, 12), (0, 10), (−20, 12), (−30, 0), (−20, −12);
  tables `0x60da02`/`0x60da12`) with scatter of `10 − rand() % 21`,
  `5 − rand() % 11`, in `640 − rand() % 81` ms, then swings between his place
  and twice the step every 1800 ms.
* **Deaths.** The k-th of n men to fall goes down at `(2k + 1) × 7200 / 2n`
  ms, his sprite turning into a body. A dying cavalryman leaves a riderless
  horse running off in a random direction 15% of the time (`Horse`,
  `Cataphracted Horse`); a dying chariot 25% of the time leaves a runaway
  (`Running Chariot`, or `Chariot Dragging Persian/Indian`).

**Who falls.** The handler keeps its own random generator, seeded with the
figure count every time (`FUN_0057a3be`: `state × 16807 mod 0x47483647` in 32
bits, scaled to 0–99 by `FUN_0057a362`), so the same unit in the same
situation always loses the same men. It visits the living men in turn and
takes one when a roll is under `100 / figures`. In shock (`CombatAnimate`)
it first marks each man exposed if, for a side where an enemy stands
(relative to the unit's front: 1 front, clockwise to 8), his entry in the
formation's table (`0x60d768` phalanx, `0x60d8d0` infantry, `0x60d960`
mounted, `0x60d9cc` elephants and chariots; 9 values per figure, the first
a threshold) is under the threshold, i.e. he stands on that side; it then
rolls twice per man, taking an exposed man on the first roll, or anyone on
the second once a whole round has passed. Other hits (`CohesionHit`) roll
once per man with no exposure test. The rewrite does the same
(`units::pick_fallen`), keeping which men are down per unit; men stand up
again when a unit recovers cohesion (🟡 which rise first is not traced).

The rewrite plays each shock as a fight (`Happened::Shock`, then its
casualties) with these strips, cels, steps and death times, only the exposed
men fighting (single figures turn toward the nearest enemy), the whole fight
in the original's 7.2 s (sped up only while many animations wait).

**Timing.** The original drives everything from one Windows timer
(`SetTimer`, `FUN_0042def7`) whose period is the shortest of up to sixteen
registered callbacks (`TimerCallback[16]`: `FUN_0043ac87` registers one at
`n` per second, period `1000 / n` ms; each slot accumulates the base period
and fires on reaching its own, `FUN_00439c6d`). The sprite layer
(`AlexPhysicalLayer`, callback `FUN_00500310`) runs at 10 per second, so a
sprite cel lasts 100 ms and the animation code's milliseconds (fights' 7200,
600 and 1800 ms legs) are real time; the palette animator runs at 5 per
second. The rewrite uses 100 ms cels for walking, shooting, fighting and
throwing down arms. 🟡 How far a sprite moves per tick (walking and runaway
speed) is not traced.

**Runaways** (`FUN_0050ae12`). A loose horse or chariot sets off in a
random direction (`rand() % 8 + 1`; due north or south veer one side) and
runs until it would leave the map: at each hex it may keep on or veer one
direction either way, never turning away from the nearest map edge (left:
not E; right: not W; top: not SE/SW; bottom: not NE/NW), taking a road (or
rocky riverbed, codes 12/13) if one is among them, else one at random
(`rand() % 4`, skipping ruled-out ones). The rewrite follows these rules
(`runaways::path`), starting each runaway as its man falls and running it at
a fixed pace (🟡 0.12 s a hex; the original's speed is not traced) until it
is off the map; the chariot choice uses both rolls (a dragging chariot is a
quarter of the quarter).

## Routed units and debris 🟡

Unit art carries, besides `Standing` and the walking strips `N`…`NW`, a
routed standing pose `StandingR` (8 cels, one per facing; infantry), running
strips `RoutN`…`RoutNW` (infantry) or `RN`…`RNW` (cavalry, elephants), and
`Drop…`, `Spear…`, `Shield`, `Helmet` pieces, and fallen men: `Shock`
(killed in melee), `Arrow` (by missiles), `Trample` (by elephants) and
`General` (a leader). `RDEBRIS.EIF` ("Generic Rout Debris") holds dropped
bows, shields and spears. A man who falls does not play a death
animation: the original swaps his own sprite for a body where he stands
(`FUN_004e77de`), a random cel of the strip for how he died together with
the `General` strip (`FUN_004e5a75`, kind at `+0x10e`: 1 shock, 2 missile,
3 trampled; a destroyed unit, `UnitEliminated`, uses `General` only). The
rewrite does the same for the men who drop out of the formation (below) and
for every man of a destroyed unit, showing each fall after the animations
queued before it. The `Drop…` strips are not deaths: a unit that routs plays
them (each cel twice, the last once) as its men throw down their arms, then
leaves its arms on the ground: per man a spear (`FUN_004e6128`; missile
units `FUN_004de0f4`) and, on foot, a shield (`FUN_004e6413`), from the
unit's own `Spear…`/`Shield`/`Shield U` strips or else the generic rout
debris (`RDEBRIS*`), each a random cel, where the man stood plus an offset
per facing. The offsets live in point tables built at start-up (code at
`0x4f6d7c`, zeros in the file), per screen facing N, NE, SE, S, SW, NW in
zoom-in pixels: spears for infantry (20,−40) (51,−11) (−35,−15) (−13,−32)
(31,−19) (−42,−15), mounted (16,−43) (48,−18) (−32,−15) (−8,−30) (24,−26)
(−16,−36), phalanx (−9,−63) (57,−16) (−43,−21) (−16,−46) (31,−28) (−67,−44);
shields for unit types 11 and 37 (−17,−22) (−1,−18) (6,−23) (17,−17) (4,−9)
(−13,−11), for 3 and 14 (−21,−4) (−21,−8) (13,9) (17,−9) (0,3) (−28,0), for the
rest (−21,−4) (−19,−6) (5,−8) (17,−4) (2,5) (−23,−5). `Shield U` is used facing
N, NE and SE, `Shield` facing S, SW and NW. The rewrite plays the drop when a
unit first breaks (`Happened::Routed`), before it runs, and leaves the arms
(`units::rout_debris`); facings whose strip is empty (`DropE`, `DropW` in the
art) skip the drop. Archers (four presentation classes, vtables `0x6017b0`,
`0x601850`, `0x6018f0`, `0x601990`, slot `0x5c` = `FUN_004c0da7`) drop a bow
(generic `Bow` strip) instead of a spear, at N (−30,−30), NE (−20,−15),
SE (−13,−29), S (−1,−26), SW (−17,−14), NW (−28,−20) (table `0x60dafe`);
🟡 their second piece (slot `0x60`, `0x4c0ed7`/`0x4c7dd8`) is not traced, so
the rewrite gives them no shield.

**Spent missiles.** The landscape (`PointedAlexLandscape`, bus handler
`FUN_00507937` → `FUN_005079e1`) leaves a spent missile from `Missile
Debris` in the hex a missile falls on, unless it is water: weapon codes 1,
2, 5, 6 use `Arrow`, 3 `Stone`, 4 and 7 `Javelin`, with a random cel, placed
up and left of the hex anchor by `w/5 + rand() % (w/10)` and
`h/4 + rand() % (h/8)` (javelins `h/3 + rand() % (h/6)`), `w`, `h` the block
size. The rewrite leaves one per volley in the target's hex as the volley
ends (🟡 the triggering message is not identified: one per volley is
assumed; bolts use the arrow strip). Routed units run
(running strips, facing the way they go), including off the map. The rewrite draws routed
units in `StandingR`, else the first cel of their running strip for their
facing; the formation is kept (the original's routed layout and the debris
are not drawn yet). In the original, units also lose figures as they take
hits: the presentation code marks figures as dying (`CombatAnimate`,
`CohesionHit`, `UnitEliminated`; the figure state array at `+0x148`, 1
dying, 2 dead) and they become bodies, so a worn unit stands thinner. The
logical side sends `CohesionHit` with `p = hits × 100 / TQ`
(`FUN_0057e4e2`); the presentation keeps `(figures − reserve) × p / 100`
figures down, picking them at random (each living figure with chance
1/figures per pass, favouring an "exposed" list from `FUN_004e6d66`), and
`RestoreCohesion` stands them up again. The reserve (`+0xfe`) is set by the
presentation class: 10 for the 20-figure class (`FUN_004d8bac`), 2 for the
6-figure one (`FUN_004fa403`), 0 for single figures, 3 in the base class
(`FUN_004e42d3`), so elephants and chariots (3 figures) never thin. A
phalanx at 6 hits on TQ 7 keeps 12 of 20, as seen in the original. The
rewrite applies this, removing figures in a fixed random order per unit
(🟡 the exposed-side preference is not reproduced). Also from
`FUN_0057e3d8`: a unit with the flag at `+0xbe` set cannot be brought to
rout by hits (they stop at TQ − 1). `gboh-tools render … save=F` draws a saved game. In the
viewer a moved unit walks along its path (0.2 s per hex, the walking strip
for its facing; 🟡 the original's timing is not measured). The game logs
moves and volleys (`Game::log`), so the computer's moves are walked and
heard too, one after another, faster while several are waiting.

## Leaders and command 🟡

**Leader ratings.** The four leader values in a scenario are, in file order:

| value | meaning | evidence |
|---|---|---|
| 1 | charisma | 0–4; Alexander 4, Tauron 0; named in `CONSTANT.AI`; picks the leader in a shock's casualty roll |
| 2 | command range | traced as a path from the leader (`FUN_00562ce7`, `FUN_00565687`): 1 a hex, 2 for a level up or down, blocked by enemies and by steeper climbs, stopping at empty hexes in enemy zones; reduced by wounds |
| 3 | initiative | shown in game as `I:`; also the starting command points (`Com: n/n`) |
| 4 | personal combat | named in `CONSTANT.AI`; reduced by wounds with the other ratings; added to the leader casualty roll |

At Chaeronea, Alexander (CH) is `3 5 6 7` and the game shows `I: 6 Com: 6/6`;
Alexander III elsewhere is `4 9 7 7` and shows `I: 7 Com: 7/7`. The manual's
leader table (pp. 77–79) confirms values 2 and 3 for every leader checked
(Alexander 7/9, Theagenes 4/9, Demosthenes 3/5, Porus 5/6, Darius III 3/8:
initiative / command range); it does not list values 1 and 4.

**Sides.** A unit fights for its nation's side unless the scenario's pairs
remap it: each pair `(nation, side)` puts `nation` on `side`'s army (at
Granicus `(0, 1)` puts Memnon's Greek mercenaries with the Persians).

**Command** (matches the flags raised at Hydaspes, turn 1). An activated
leader commands the units of his own command group within his command
range. With personal combat he also brings in each other leader of his side
within his range, with that leader's units within the subordinate's own
range. At Hydaspes this flags Alexander's group, Coenus's Companions and
Antigenes's phalanx, but not Craterus's group 16 hexes away across the
river.

**Hex distance.** Map hexes are columns `x` and rows `y`, odd columns half a
hex lower (offset coordinates). Axial `q = x`, `r = y − (x − (x & 1)) / 2`;
distance `max(|dq|, |dr|, |dq + dr|)`.

**Activation order** ✅ (from the game controller). Each leader registers
his name, nation and initiative when the scenario loads; the register order
is the scenario order. Every turn:

1. **Elite initiative.** If the battle has the rule (all but Chaeronea,
   Samarkand and the last table entry) and it has been used fewer than three
   times, a named leader goes first: on turn 1 the first leader whose name
   contains `Alexander` (`Parmenion` at Issus), on later turns `Alexander`
   with a 40% chance (100% at Pelium) per turn. (The game can also ask the
   player.)
2. **Initiative draw.** Otherwise, and for every later activation, one
   leader is drawn among those not yet activated this turn (and not
   eliminated), with probability proportional to initiative: `rand() %
   total`, then the first leader whose cumulative range contains the roll.
   A leader with initiative 0 is never drawn.
3. **Momentum.** A leader who asks for it after his activation goes again
   if `rand() % 100 < (initiative + 1) × 8` on the first attempt, `× 5` on
   the second; at most two successes ("… attempted momentum (tactical
   initiative) and was successful!"). Otherwise the next leader is drawn.

The turn ends when every leader has gone. This matches every capture so far:
Lyginus, Pelium, Granicus and Hydaspes open with Alexander by elite
initiative; at Chaeronea, where the rule is off, Alexander (initiative 6, as
is Philip) came up in the draw.

## Movement ✅ (verified in game: the hexes the game highlights for a selected unit match exactly for six Hydaspes units, including two phalanxes)

**Movement allowance** is the scenario's `unk_a` (leaders 9, cavalry 8, most
foot 5, phalanx 4). It sets both the unit's current and maximum movement
points.

**Movement classes** by unit type (the original's class codes): phalanx (0),
heavy infantry (2: hoplites, hypaspists), medium infantry (3), Macedonian and
Greek light infantry (5), light infantry (6), skirmishers (7: javelins,
archers, slingers, Agrianians), heavy cavalry (8), lancers (11), light
cavalry (12), elephants (13), chariots (14), oxybeles (15). Leaders move by
rules not ported yet.

**Direction.** A unit faces a hex corner and moves only into the two hexes in
front, keeping its facing: facing 2 → sides N and NE, 3 → NE and SE, 4 → SE
and S, 6 → S and SW, 7 → SW and NW, 8 → NW and N (map sides).

**Step cost** into a neighbour: the largest terrain cost of the destination's
hex codes (woods 2; everything else in the shipped maps 1), at least 1; +1
for crossing a minor stream (side code 6); + the elevation difference. Side
code 11 costs 99. In the original's table these are hex terrain types 0–5
and 10 (costs 1, 2, 1, 2, 1, 2, 2) and side types 6, 8, 9 (+1 each) and 11
(99). Side types 8 and 9 never occur, and the loader's water marker for
elevation 0 is overwritten, so water costs only its elevation change.

**Impassable:**

* any side with side code 20;
* climbing more than 2 levels (elevation 0 counts as 1), more than 3 for
  skirmishers, more than 1 for horses, elephants and chariots;
* chariots into woods or rocky hexes (hex codes 1–5);
* horses (heavy and light cavalry, lancers) into a hex next to an elephant;
* a hex holding any unit except a friendly leader (two-hex units hold both
  hexes).

The original also has bans for chariots and elephants on streams and other
sides, but they test side types above 4 directly, which its side test always
answers no, so they never apply.

**Zones of control** (`FUN_004b4287`; each unit's set at `+0xe6`): a
combat unit holds its two front hexes (both hexes' for two-hex units) that
it could step into itself; leaders and routed units hold none. A hex where
a skirmisher stands is never in a zone. A unit may enter a hex in an enemy
zone but not step out of one (`FUN_0056e7d7`), except from where its order
began (with full movement); a two-hex unit whose second hex is in a zone
cannot step at all. Horses may not enter a zone an enemy elephant holds.

**Cohesion for terrain** (`FUN_0056ef04`, unit slot 0x3c, for each step
of a move): hits by class group (infantry and oxybeles / skirmishers /
horse / elephants / chariots) for the hex entered and the side crossed,
from the table at `0x63bae8`:

| terrain | inf | sk | horse | el | ch |
|---|---|---|---|---|---|
| 1 woods | 1 | 0 | 1 | 1 | – |
| 2 rocky | 1 | 0 | 2 | 1 | – |
| 3 marsh | 1 | 1 | 1 | – | – |
| 4 major river | 1 | 1 | 1 | 0 | – |
| 5 major river | 1 | 1 | 2 | – | – |
| 6 minor river (side) | 1 | 1 | 1 | 0 | – |
| 8 steep-banked stream (side) | 2 | 1 | 2 | 1 | – |
| 9 steep bank (side) | 1 | 0 | 1 | 0 | – |
| 10 abatis | 1 | 1 | 1 | – | – |
| up 1 level | 1 | 1 | 1 | 0 | 1 |
| down 1 level | 1 | 0 | 0 | 0 | 1 |
| up 2 | 2 | 2 | – | – | – |
| down 2 | 2 | 1 | – | – | – |
| 3 levels | – | 3 | – | – | – |

(– : 99 in the table, ground the class cannot enter.) Horses stepping next
to an enemy elephant take 2 more ("… moved next to an elephant for 2
hits"). A two-hex unit pivoting pays as for a step to its new main hex
(`FUN_0052843b`; 🟡 from its old main hex, as read).

**Two-hex units** (phalanx, double-sized hoplites) have a second hex at the
flank, map direction facing + 2. Both hexes move together the same way; both
steps must be legal and the cost is the main hex's. The game highlights both
hexes of every position.

**Reachable hexes** are found by a shortest-path search over these steps
with the unit's remaining movement points.

## Orders ✅ turning and command points (verified in game)

**Turning.** A unit turns 60° at a time. Every class pays one movement
point (the manual's two points for elephants, chariots and cataphracts is
not in the code); chariots also take two cohesion hits per turn, as the
manual says. Left is anticlockwise through the
six facings (3 → 2 → 8 → 7 → 6 → 4 → 3) and right the reverse. Skirmishers pay only for their first turn of the turn. A two-hex unit pivots: on a left turn its main hex stays and the
second hex swings round it, on a right turn the second hex stays and the
main hex swings.

**About face** (unit slot `0xf4`, `FUN_005719dc` → `FUN_005253f2`): three
movement points and three turns the same way, reversing the facing; a
two-hex unit's hexes swap, its second hex becoming its main. Like any order
it first takes the ordered-again hit. (🟡 when the button is enabled is not
traced: the rewrite asks for three points left.)

**Undo** (`on_UnitUndo`, `FUN_004b0654`): before a unit's move or turn the
game keeps its state and the active leader's command points on a list
(`FUN_00521fe4`, the list at `0x6502f4`); a different unit's action clears
the list first. Undo takes the last entry back. Missile fire, shock,
recovery and group moves clear the list (`FUN_0056d49b`, `FUN_00570c93`,
`FUN_0057ea82`, `FUN_00564374`); the rewrite also forgets a move that drew
reaction fire.
Verified at Hydaspes: after turning Hypaspist 1 left, the game highlights
exactly the hexes predicted for facing 2 with 4 points left.

**Command points** (`Com: left/max`, starting at the leader's initiative;
verified at Hydaspes: after Alexander had Hypaspist 1 turn and the Agema
Companions, of Coenus's group, move, the game showed `Com: 4/7`, i.e. 1 + 2
points).
Each time a unit finishes its orders the active leader is charged: one point
when he moves on to a different unit, two if that unit is outside his own
command group. A personal combat uses all remaining points. The leader is
finished when he has used them all.

**Every activation** (momentum too, `FUN_0054b4eb` broadcasting
`BeginLeaderOrdersPhase`) gives every unit its movement back and clears
"done" (`FUN_0052b2e2`, `FUN_00536b1a`), so a unit ordered by one leader
may be ordered by a later one the same turn. **Ordered again:** a unit
that spent movement in an earlier order this turn takes one cohesion hit
the first time it moves, turns or fires in an activation ("… takes 1
cohesion hit for being ordered again this turn."); if it breaks, the order
is not carried out.

## Shock combat ✅ tables (match the manual) / 🟡 resolution flow

**Checked against the manual** ("Combat Tables", pp. 81–86 of the GOG PDF).
The game's base-column table, minus 1, equals the manual's "Initial Clash of
Spears" for every defender class, angle and attacker class it lists (0
differences); the superiority table equals the manual's "Shock Superiority"
(0 differences, 1 = AS, 2 = DS); the size adjustment formula reproduces the
manual's "Size Ratio Difference" table; and the results tables match "Shock
Combat Results" in 128 of 130 cells. The two that differ (the game's data
wins for a faithful port; they may be manual typos or a later patch):

| die | column | manual | game |
|---|---|---|---|
| 0 | 3 | 3(2) | 4(2) |
| 1 | 10 | 2(3) | 2(2) |

Class codes map to the manual's abbreviations: PH 0, HI 2, MI 3, LP 5
(peltasts), LI 6, SK 7, HC 8, LN 11, LC 12, EL 13, CH 14.

Rules the manual gives that the game does **not** apply: the terrain
shifts (1L for defenders in river, marsh or woods, 1L/2L uphill or across
steep-banked streams, 1R downhill): the caller passes a terrain shift that
is always 0; skirmisher halving, and ignoring size when elephants or
chariots fight: neither appears anywhere in the combat code.

**Around the table** (from the caller):

* Side sizes are summed, leaving skirmishers out when any other kind of unit
  is on that side (as the manual says for the Agrianians).
* The defending class used is that of the defending unit that gives the
  attacker the lowest frontal base column.
* Each side's hits are shared: `total ÷ units` each, the remainder to the
  leading unit; a unit whose share is at least three times the other side's
  per-unit share (at least 1) takes 2 more; a unit one hit from routing and
  in enemy contact makes a TQ check, taking 1 fewer hit if it costs nothing
  and 1 more otherwise.
* A unit whose hits reach its TQ routs. The manual's "DS–" (no
defender superiority against cavalry) applies to missile units *out of
missiles*; the game's code tests only whether the defender is missile
capable.

The original resolves a shock with one call per combat. Inputs: the leading
attacker's class and the defender's class (movement classes, codes 0–15),
the attack angle (0 front, 1 flank, 2 rear), the attackers' total size and
the defender's size, and a few flags.

**Column** (0–12) = `COLUMN[defender][angle][attacker] − 1`
+ size adjustment (attackers bigger: `attacker ÷ defender − 1`, integer
division; defender bigger: `1 − ceil(defender ÷ attacker)`)
− 3 for a phalanx attacked from the front
+ 2 / − 2 when the leading attacker / the defender is cataphracted heavy
cavalry (the heavy cavalry's trailing scenario byte; the manual mentions only
the defender's 2L)
+ any further shift, clamped to 0–12. The base table is 16 × 3 × 16 values
(in `gboh_game::combat`).

**Row** = `rand() % 10` (the caller passes no modifiers).

**Hits**: the attackers take `ATTACKER_HITS[row][column]` (6 at worst, 1 at
best) shared among them; the defender takes `DEFENDER_HITS[row][column]`
(1 to 6). Higher columns favour the attacker.

**Superiority** doubles the defender's hits (attacker superior) or triples
the attackers' (defender superior):

* flank and rear attacks make the attacker superior, except a flank attack
  on skirmishers and horse attacking elephants;
* otherwise a 16 × 16 class table decides (1 attacker, 2 defender), with
  exceptions removing the defender's superiority when a defending missile
  unit faces certain horse classes, or heavy cavalry with its special flag
  attacks heavy infantry. The manual's "DS–#" and "hDS" marks correspond to
  these exceptions.

**Leader casualties.** After a shock, each side's leader involved (the one
with the highest first rating, probably charisma) rolls `rand() % 10` plus
his fourth rating; the loser by 1–2 is finished for the turn, by 3–4
wounded (killed if already wounded), by 5–6 killed; a tie or a margin of 7
or more does nothing (the code only tests 1–6).

**TQ checks.** A unit checking takes `max(0, rand() % 10 − TQ)` cohesion
hits; if its hits then reach its TQ, it routs (so a TQ 9 unit never takes
hits from a check, as the manual says).

**Before contact.** Defenders already routed are eliminated when attacked
(the code sends `EliminateUnit`). If any defender is still standing, each
attacker marked when the combat was registered takes a TQ check; then, if
any attacker checked, every defender checks too, except phalanx and heavy
infantry defenders when every checking attacker is light infantry, and a
charging chariot adds 1 to the defenders' roll. A unit whose hits reach its
TQ routs and leaves the fight. The marked
attackers are simply those that moved this turn (`FUN_00570883` marks an
attacker when its attack is registered if its movement left is below its
allowance), of any class: the manual's list (heavy and medium infantry,
phalanxes, lancers, elephants, chariots, with exceptions for skirmishers
and light infantry) is not what the code does, and the original's
commentary shows skirmishers taking hits "during the charge". At Hydaspes the Indian chariots (TQ 4, 2 hits) failed
this check by 3 and routed before contact.

**Angle.** An attacker in one of the defender's two front hexes (the two
it moves into) attacks frontally, from the two opposite hexes from the rear,
and from the remaining two on the flank.

**Leading attacker and angle.** Against the primary target (the clicked
defender, else the first), each attacker is scored by its frontal base
column against the target's class. The original let an attacker in the
target's flank or rear lead only if its score was at least the best so
far, so a flanker scoring below an earlier frontal attacker brought no
flank advantage. Fixed as the manual says: the most advantageous angle
any attacker has (rear, flank, front; either hex of a two-hex unit) is
used, and the attacker at it with the best score leads, ties to the later.

**Sequence** of one shock (as implemented in `gboh_game::shock`): eliminate
routed defenders; pre-contact TQ checks; pick the leading attacker and
angle; pick the defending class; sum sizes; roll on the table; share hits and apply the
extra and near-rout rules; units whose hits reach their TQ rout. Not yet:
applying leader casualties, advance after combat, rout movement, and how the
original pairs attackers and defenders (in game every enemy in an
attacker's zone of control must be attacked, and every defender must be in
an attacker's zone).

**Rout points** (the original's per-class method): Alexander his initiative
× 10 (also as "Alexander (CH)" at Chaeronea, where the manual says not),
other leaders × 5, phalanxes 2 × TQ, skirmishers 1 (the Agrianians too; the
manual says 2), elephants and chariots 2, oxybeles 0, everything else TQ.


Seen in game but not ported: reaction fire (when the Agema Companions moved
next to them, "India 3 throws javelins at Agema Companions for 1 cohesion
hit"). Missile units show `Jav(3/3); Range: 1` for India 1, whose scenario
missile values are `3 1 5`: the first is the weapon (javelin), the second
matches the range; the 3/3 supply is not yet matched to a field.

Seen in game (Hydaspes, turn 1): Indian chariots (TQ 4) charged the Agema
Companions; "India 8 took 3 cohesion hits during the charge. India 8 is
routing!" With 2 earlier hits that made 5 ≥ TQ, so it routed before contact,
consistent with the rout rule above. The game's Combat Results dialog shows
only each unit's current and total hits and hits to rout, not the column or
die roll, so the shock tables can only be checked statistically or by
reading the game's memory.

Not ported yet: choosing the combats and attackers, the pre-shock check
(a per-class table indexed with a 0–9 roll), routs and elimination.

## Missile fire ✅ table (ranges agree with the manual) / 🟡 not checked in game beyond reaction fire

**Weapon codes** inside the game: 1 bow, 3 sling, 4 javelin, 6 bow and 7
javelin on light cavalry, 8 bolt (scenario missile byte 1, 2, 3, 4,
remapped; light cavalry's bows and javelins are re-coded on load).
**Supply** (manual): javelins 3, bows and slings 10, bolts 99; replenished
at a leader's activation by a random amount up to half the original supply
(not ported).

**Volley.** Hit if `rand() % 10 + modifier ≤ HIT_ON[weapon][range]`:

| weapon | range 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|
| bow | 5 | 3 | 3 | 1 | | |
| sling | 3 | 2 | | | | |
| javelin | 5 | | | | | |
| mounted bow | 5 | 3 | 1 | | | |
| mounted javelin | 3 | | | | | |
| bolt | 7 | 5 | 5 | 3 | 2 | 1 |

A bolt at range 7 read past the original's table into the next byte (85),
so it always hit; fixed: it hits on 0. Modifiers: +1 against phalanx or heavy infantry, +3 more when
that target is shot from its front at range 1 by anything but javelins
(shields); +1 if the shooter has moved this turn (not javelins); against
cataphracts +2 for bows, +1 for slings and javelins. A hit is 1 cohesion hit,
2 against elephants and chariots.

**Reaction fire** at a unit entering a missile unit's zone of control: a
volley at range 1 with no front or movement modifiers. Seen at Hydaspes:
India 3's javelins hit the Agema Companions for 1 (hits on 0–5, 60%).

Leaders can be hit by volleys (15% per the manual); not ported.

## Routs ✅ path rule (from the code) / 🟡 not checked in game

A routed unit first turns to face its retreat direction the short way round
(two-hex units that cannot turn are eliminated), then flees: two hexes at
once, its full movement allowance in the rout phase at the end of the turn
(manual). Each step tries map directions in order: retreat N: N, NW, NE; E:
NE, SE, N, S; S: S, SW, SE; W: NW, SW, N, S. Fleeing east or west, odd steps
swap the first two, so the unit zig-zags straight across. A step must stay
on the map, be passable for the unit, be outside enemy zones of control and
be empty; failing that, outside the zones with friendly units; failing
that, with friendly units in a zone (`FUN_0057ae96`). Reaching the map
edge, finding no step, or coming back to a hex already passed takes the
unit out of the battle (terminal rout or elimination), and its rout points
count against its army. It runs one hex further if it ends on an occupied
hex, and one more if next to an enemy leader. Friendly combat units it
routs through take a cohesion hit each (which may break them); routed ones
are destroyed. Foot archers, slingers and javelinists drop their missiles
when they rout (`FUN_0056bcb0`).

**Rally** (from the code; the manual adds: once per turn per unit, by a
leader within command range, two orders outside his group, and the unit
cannot be ordered again that turn). The first roll fails if `rand() % 10 >
leader initiative + 1`. Otherwise a second roll, halved, and the unit's TQ
band give the hits removed:

| roll | TQ < 4 | TQ 4–5 | TQ ≥ 6 |
|---|---|---|---|
| 0–1 | 1 | 1 | 1 |
| 2–3 | 2 | 2 | 2 |
| 4–5 | 2 | 2 | 2 |
| 6–7 | fails | 3 | 3 |
| 8–9 | fails | 4 | 4 |

A failed rally sends the unit into terminal rout towards its map edge.

## Turn sequence 🟡 (`gboh_game::game`; the manual's order with the ported rules)

1. **Begin turn:** elite initiative or the initiative draw picks the first
   leader; he gets command points equal to his initiative.
2. **Orders:** he orders units in his command (move, turn, volley; one or
   two points each) and designates shocks.
3. **Finish leader:** his shocks are resolved; routed units flee two hexes;
   then momentum or the next draw. Repeat until every leader has gone.
4. **End turn:** routed units flee their full movement allowance; movement
   points and turning reset.
5. **Victory:** an army whose lost rout points (units out of the battle)
   reach its rout level withdraws and loses; if both do, the one less far
   past its level wins, the Macedonians on a tie (manual).

Also in the loop (🟡 from the manual unless noted):

* **Missiles:** units start with their supply; a volley order needs the
  target in the unit's arc (within range by forward steps from its front),
  once per activation. At each activation units in command that are out of
  missiles get `rand() % (ceil(supply ÷ 2) + 1)` and cannot be ordered.
* **Reaction fire** during movement, hex by hex: each enemy missile unit
  with missiles and the entered hex in its arc fires at range 1; a unit
  routed by it stops.
* **Shock designation:** the target must be in a front hex; every other
  enemy in the attacker's front hexes is attacked too, unless already
  attacked this phase; a second attacker on the same target joins that
  shock. Skirmishers cannot shock except the Agrianians, or against
  chariots and routed units.
* **Advance:** if a shock clears the primary defender's hex and it is
  empty and next to no other enemy, the attacker with the highest TQ moves
  in (single-hex units).
* **Leader casualties:** in a shock the higher total is hurt (`0x4a9140`).
  Finished leaders skip the rest of the turn; wounded ones
  (`FUN_00566251`) lose 2 (1 when hurt stepping away) from all four
  ratings; a second wound kills. A killed leader's rout points count and a
  soldier from the ranks replaces him (range 4, initiative 2, finished for
  the turn; `FUN_00565b3d`); Alexander and Darius are not replaced.

* **Orderly withdrawal** (automatic; `FUN_00577e3c`, `FUN_00578783`,
  `FUN_00524ce4`, `FUN_005778f8`): when a unit moves next to them, light
  and heavy cavalry withdraw from anything, lancers from all but horse,
  skirmishers from heavy infantry and phalanxes, unless another enemy holds
  them in its zone. Cavalry an elephant approaches first takes hits; a unit
  with TQ − 2 hits or more stays; then a TQ check (`rand() % 10 ≤ TQ`). It
  moves with 2 points away from the approacher (the three sides facing
  away), through empty hexes, zones ignored, to the hex farthest from the
  nearest enemy; then one hit if approached from flank or rear
  (skirmishers: rear) and one more for a failed TQ check. The approacher is
  not stopped. A leader alone steps away at a risk of a casualty (90 in 100
  from cavalry, 60 from others, `random() % 100`).
* **Missile replenishment** (`FUN_005672ba`): at a leader's activation,
  each unit he may order that is out of missiles gets `random() % 5`
  (javelins `% 2`) and is done for the turn; more than two cost him a
  command point.

* **Group move** (`FUN_00564572`; conditions from the manual): before
  any other order, if every unit of the leader's group faces the same way,
  is in command and is more than two hexes from any enemy. `rand() % 100`
  below `(initiative − 2) × 20 + 30` grants a full group move, below
  `(initiative − 2) × 20 + 50` a one-hex one (capped at 100); either way it
  takes all his orders and rules out momentum. The group then steps
  together through the two front sides of its facing, as far as the
  slowest unit allows, never into an enemy zone of control. Every attempt
  marks the leader; at his next activation the mark extends a streak
  (`FUN_00565fa0`; reset if he did not try, or was in personal combat). With
  a streak of one the roll is 30 lower; with two or more it is 0, a sure
  pass (the manual's "Alexander is always able to complete a one-hex group
  move with the correct conditions").

## AI scripts (`*.AI`) ✅ language / 🟡 built-ins

The computer opponent is scripted: plain-text files in `DATA`, 45 of them
(about 7,800 lines). Each scenario names its plans per army (`home_ai`,
`visitor_ai`; Hydaspes: `MHYDAS1-3` and `IHYDAS1-3`). `BASICMAN.AI` holds the
shared tactics; `SPECIAL.AI` is a library the Macedonian Hydaspes and Pelium
plans import after it; the rest are plans.

**Language.** Names are case-sensitive: `BASICMAN.AI`'s `Rally` has a local
`routing` beside the constant `ROUTING`, and treating them as one makes
every unit look routed. Keywords appear in fixed case (`if`, `else`,
`for … to`, `while`, `return`, `var`, `const`, `procedure`, `import`,
`write`, `watch`, `NOT`, `AND`, `OR`, `Conditions`, `Orders for`). A file
holds imports, constants, variables and procedures (forward declarations end
in `;`), and a plan `Name { … }` with a `Conditions` block and `Orders for
<leader> { <condition> <call>; … }`. The `;` ending a simple statement may
be missing before `else` or `}` (`BASICMAN.AI` and `SPECIAL.AI` rely on
this). Values are integers; lists are handles. With these rules every file
parses, and every call in every plan names a script procedure or a game
built-in with the right number of arguments.

**Running.** The game asks the active AI leader's plan for one order at a
time (`GiveAIOrder`): the conditions are evaluated, the first `Orders for`
entry whose condition holds is called, and that repeats after each order
until the script calls `FinishLeader`.

**Built-ins.** The game registers 23 constants (`TQ 1`, `COHESION_HIT 2`,
`SIZE 3`, `MOVEMENT_POINTS 4`, `MOVEMENT_REMAINING 5`, `MISSLE_TYPE 6`,
`MISSLE_RANGE 7`, `MISSLES_REMAINING 8`, `UNIT_TYPE 9`, directions `NORTH 1`
… `NORTHWEST 8`, `TRUE`, `FALSE`, `CLOCKWISE 1`, `COUNTERCLOCKWISE 2`) and
50 functions with their argument counts (`MoveToward(hex, unit)`,
`Query(attribute, unit)`, `AssignList(list, kind, unit)`, …). `CONSTANT.AI`
adds names for the attributes, the classes and the AI's own codes.

Script values: units are ids, hexes are `x * 100 + y` with **no** map origin
(Porus's first goal at Hydaspes, `1016`, is map hex (10, 16), in front of
the Macedonian line), directions 1 N … 8 NW. Facings are hex vertices (NE,
E, SE, SW, W, NW); adjacent-hex directions are sides (N, NE, SE, S, SW,
NW); `GetFrontalFacings` in `BASICMAN.AI` spells this out.

`Query` attributes (from the code): leaders 14 command range, 15
initiative, 16 charisma, 17 personal combat, 18 orders given, 19 orders
left; combat units 1 TQ, 2 cohesion hits, 3 size, 9 class, 21 routing;
missile units 6 type, 7 range, 8 missiles left; every unit 4 movement
allowance, 5 movement left, 10 hex, 11 second hex, 12 facing, 13 nation, 20
missile capable; anything else returns −1. Nations as the scripts see them
(the game remaps on load): file 0 Greek → 4, 1 Persian → 2, 2 Macedonian →
1, 3 Indian → 3, 4 Danubian → 5, 5 Scythian → 6, matching `CONSTANT.AI`.
`AssignList` kinds: 0 front, 1 flank, 2 rear, 3 own hexes, 5 all around, 6
the units the leader can order. `Random(n)` is `rand() % n`. The group list
is the units the active leader can order.

**Traced built-ins** (per-class unit methods; all combat classes share
them):

* **Order count:** the AI's own budget is initiative − orders given; each
  new unit ordered costs one, wherever it stands.
* **`Direction(a, b)`:** hexes become grid points `(2x + 2, 4y + 2 + 2·(x
  odd))` (the same `FUN_004b1c4a`, with its extra column from `y = 50` on); the ratio of `|dy|` to `|dx|` (0, equal, under, 3×, under 3×,
  over) and the quadrant index a table of angles in 15° steps clockwise from
  north (the six neighbours sit at 0, 4, 8, 12, 16, 20). The angle ÷ 4 gives
  NE, E, SE, SW, W, NW, so due north counts as NE and due south as SW.
* **`IsInGeneralDir(unit, hex)`:** the angle lies in the facing's five-step
  cone (NE 0–4, E 4–8, SE 8–12, SW 12–16, W 16–20, NW 20–0), from either hex
  of a two-hex unit.
* **`Rotate(unit, dir)`:** N becomes NE and S becomes SW; it turns clockwise
  if that is fewer steps, otherwise counter-clockwise.
* **`TurnToAttack(unit)`:** tries facings 0–3 turns away, clockwise first
  at each distance, and turns to the first with an enemy in front.
* **`AnyoneToCombat(unit)`:** among enemies the unit can reach, routed ones
  first, then those within 3 hits of routing, then those its class is
  attacker superior to (superiority table), then reachable flank and rear
  hexes of the others, then enemies it attacks at a frontal base column
  above 4; the nearest in the first non-empty group.
* **`MoveToward(hex, unit)`:** the target if reachable, else the reachable
  hex (its own included) nearest the target.
* **`FindClosestEnemyToAttack(unit)`:** searches rings outward; routed
  enemies count.
* **`CanMove(unit)`:** whether any empty hex is reachable.
* **`Recover(unit)`:** removes 0/1/2/3 hits with 5/40/50/5%; the leader's
  initiative moves `(initiative − 4) × 4` points from "none" to "three" (a
  negative "none" comes off "one"); the roll `rand() % 100` is 40 lower in
  an enemy zone of control.
* **`AttemptToRally(unit)`:** counts the order, then the rally rule.

* **Who may take orders** (`FUN_005235b8`, at every leader's activation,
  for the player too): a unit of his side in command range that has not
  spent movement points this turn and is not done. The list is fixed for the
  activation. A shock order (`Attack`) or a recovery takes the unit off it
  and marks it done for the turn; an AI volley takes it off. So a unit moved
  by one leader cannot be ordered by the next.
* **Group list and `CanBeOrdered`:** the group list is the leader's whole
  group, in command range or not (`GetUnitsOutOfRange` filters it with
  `InCommandRange`); `CanBeOrdered` is true for units in his command and for
  the leader himself until he has moved (`MoveLeader` asks it first).
* **Moving a unit** (`FUN_00538e7c`): a unit flagged "ordered again"
  takes 1 cohesion hit when it moves, once per turn (see momentum below).

* **`Attack(unit, hex)`:** attacks everyone in the unit's zone of control,
  the unit on `hex` as primary target; always returns 1; the unit is done.
* **`Fire(unit, hex)`:** fires if the hex is among the unit's targets.
* **`FindClosestEnemyToFire(unit)`:** the nearest of those targets.
* **`EnemyWithinRange(unit, r)`:** any enemy, leaders included, within `r`.
* **`FindHurtingUnit()`:** over every friendly unit on the map, the nearest
  routed one if the leader's initiative is over 3, else the nearest with
  cohesion hits.

* **Leader movement** (`FUN_00561d02`, step cost `FUN_005655da`): any of
  the six directions within his remaining movement; a step costs 1 + the
  height difference and is impossible across an impassable side or two or
  more levels; terrain does not matter. He may not enter a hex holding an
  enemy or next to one, and may end only in an empty hex. `MoveToward` sends
  him to the target if he can end there, else to the nearest hex he can.

**AI against AI**, each side's first plan, up to 30 turns:

| battle | result |
|---|---|
| Granicus | Macedonians win, turn 7 |
| Hydaspes | Indians win, turn 6 |
| Chaeronea | Macedonians win, turn 9 |
| Gaugamela | Persians win, turn 8 |
| Lyginus | Danubians win, turn 8 |
| Pelium | Danubians win, turn 7 |
| Samarkand | Persians win, turn 9 |
| Issus, Arigaeum, Jaxartes | no verdict in 30 turns |

**Checked against the original** (Hydaspes, Wine). The original allows
only one computer army (the Leader Control dialog flips the other to
Player), so the comparison is the Macedonian AI against Indians that give
no orders (`gboh-tools ai … HYDASPES.SCN 2 home names`), read from the
original's Running Commentary. The original played the first plan in the
scenario's list (`MHYDAS3`). Over two turns the same leaders move the same
units: Alexander's Sogdia runs into India 3's javelins; Craterus moves
Prmsida, Arachos and Companions (Craterus); Coenus's Companions (Demetrius)
and Agema Companions draw javelins; Antigenes moves the Cleitus, Gorgias,
Attalus and Meleager phalanxes; Tauron's Macedonia 1 archers advance on
India 1. Activation order and dice differ (the original's random seed is
not reproduced).

**Longer run, same setup (7 turns).** Rout points lost at each turn end,
Macedonians / Indians:

| turn | original | rewrite |
|---|---|---|
| 3 | 0 / 9 | 0 / 0 |
| 4 | 15 / 20 | 0 / 2 |
| 5 | 57 / 20 | 6 / 4 |
| 6 | 57 / 22 | 7 / 4 |
| 7 | 66 / 22 | 7 / 4 |

(Game 1.) A second game of the same setup went very differently (game 2:
0 / 6 at turn 4, 14 / 47 at turn 7), so the original's results vary widely
with the dice; distributions, not single games, have to be compared.

In game 1 the original is far bloodier. On its turn 4 the Antigenes phalanxes charge
the elephant line and Meleager, Cleitus and Coenus rout at once; most of
the Macedonian loss then comes from failed rallies (terminal routs of
phalanxes, 14 rout points each). In the rewrite the same attack happens
but costs the phalanxes almost nothing, so the melee against elephants (or
the check before contact) differs and needs checking from an identical
position. Routed elephants rampage in the original (now implemented, see
`Game::rampage`).

**From identical positions.** Starting from game 2's autosaves (the turn
is resumed with the leaders the save's commentary has not yet shown active)
and playing to the next save, 40 rewrite runs each:

| from | original lost (Mac / Ind) | rewrite median (Mac / Ind) |
|---|---|---|
| turn 4 | 0 / 15 | 0 / 14 |
| turn 5 | 14 / 16 | 0 / 22 (Mac max 6) |
| turn 6 | 0 / 10 | 1 / 12 |

Two of three turns agree closely; on the third the original loses a
phalanx (14 rout points) and the rewrite never loses more than 6. Loading
that turn-5 save into the original and letting it play the turn again gave
+1 / +19 (rewrite median +0 / +22), so the phalanx loss was a dice swing of
the original's, not a rule the rewrite misses. The original's Combat
Results for that replay (hits each unit took per melee) fit the results
table's ranges: attackers 0–3, defenders 1–5 (e.g. the Meleager phalanx
charging an elephant: 3 / 2). Over
whole battles (40 runs from the start) the rewrite's Macedonian losses at
turn 7 have median 2 and maximum 43, against 66 and 14 in the two original
games, which with the replay above looks like the original's spread
rather than a difference in rules.
`gboh-tools ai … save=F seed=N` runs these replays.

A leader may try for momentum only if he gave at least one order in that
activation, did not group move and is on the map (`CanAttemptMomentum`,
sent at the end of his orders, `FUN_00564374`). Every unit restores its
movement and clears "done" on `BeginLeaderOrdersPhase` (`FUN_0052b2e2`);
how that combines with the orderable rule and the "ordered again" flag in
the original is not fully traced.

The commentary also showed that **AI leaders always try for momentum**, and
that after a successful momentum the leader's units move again, those that
had already moved taking the "ordered again" cohesion hit (Craterus's
Prmsida, Arachos and Companions; Antigenes's four phalanxes). The rewrite
now does both: every activation refreshes every unit (see "Orders"). Two
script behaviours that look like bugs are the original's: `AttackInZOC`
attacks at the first occupied hex in front even if the unit there is a
friend, and units next to the enemy they were sent at report "could not
move".

## Saved games (`*.alx`) 🟡 units only

The original autosaves during play (`DATA/Autosave.alx`). The file starts
with a title (`Autosaved Game: Hydaspes; Turn #8; Spitaces`) and the
scenario file name, then the Running Commentary text, then the sprites'
state (art titles), then the logical units, then activation state and the
AI plan files in use (`mhydas3.ai`). `gboh_game::save` reads the units:

* **Order:** combat units of the home army in scenario order, then the
  visitor's, then the leaders in plain scenario order (checked by drawing
  game 2's turn-5 save beside the original's own view of it: grouping the
  leaders by army put Indian leaders where Macedonian ones stand).
* **Base record** (`FUN_00529e9f`), 2-byte values unless noted: movement
  left, unknown, done, ordered again, unknown, a flag adding one trailing
  value, hex (`x * 100 + y`), second hex, facing (4 bytes), out of the
  battle.
* **Combat units** (`FUN_00536c6a`) add hits, routed and two unknown
  values (with the flag above: one more value and a counted list); missile
  units (`FUN_0056c1ee`) add missiles left and two unknown values. Chariots
  save the missile values or not depending on their class (not decoded;
  the reader takes whichever form lets the next record parse).
* **Leaders** (`FUN_005622d4`): a flag with 20 bytes of activation state,
  a flag with the four ratings, a chain of named entries, then the base
  record. A killed leader keeps a stale record with no valid hex.

The section is found by its shape (the first offset where every record
parses). Checked: the turn-8 autosave of the Hydaspes comparison gives 66
and 22 rout points lost, as the original's Game Overview showed.
`gboh-tools ai … save=F` starts a match from a save.

## The rewrite's saved games (`*.sav`)

The rewrite saves its own games rather than the original's `*.alx` (of
which only the units are decoded): a text file with the scenario, who the
computer plays, the rout levels and the player's orders in turn. Loading
starts the scenario afresh and replays the orders, play being
deterministic. The format is described in `crates/gboh-viewer/src/saves.rs`;
its first line carries a version, as a change to the rules makes old orders
play out differently. The save and load dialog is the original's
`LOADSAVEDLG` (description, file name, the saves found, OK, Cancel, Delete);
the start screen's Load Saved Game opens it, and a game autosaves as
`Autosave.sav` at the start of each turn. Saves go to the platform's
application data directory (`GBOH_SAVES` overrides it), never the game's.

## Menus, preferences and help

**Menus.** The battle window's menu bar is the executable's `CHILDMENU`
(`RT_MENU`: a header word pair, then items of flags `u16` (0x10 popup,
0x80 last), a command id `u16` unless a popup, and a UTF-16 text; `&` marks
the mnemonic and a tab the accelerator). The rewrite draws it in the
palette's Windows colours (light grey 7, dark grey 248, white 255, navy 4).
Commands of the multiple-window interface (Open Map, Tile, Cascade, Arrange
Icons) and the network chat are greyed: the rewrite has one window.

**Preferences** (registry `HKCU\Software\I-Magic\Alexander\Preferences`,
read in `FUN_004816e2`): `Movement Animation`, `Combat Animation`,
`Sound`, `Ambient Sound`, `Music`, `Finish Leader`, `Overall Combat
Results`, `Show Intro Video`, `Auto Save` (1 when first created),
`Volume` and `Music Volume` (5000 of 10000). The `PREFERENCES` dialog
(142 × 209 units; its debug controls lie past its right edge and are not
shown) has check boxes 101, 102, 104, 15, 103, 17, 20, 18, 19 in that order
and two `Slider` controls the dialog makes in code (`FUN_00599944`,
`FUN_00427990`, range 10000; art `SLIDER.EIF` strip `Horizontal`: thumb,
track left, middle, right). 🟡 The sliders' places are inferred from the
gaps above the "Sound Volume" and "Music Volume" labels.

**Music** (tables at `0x61b058`): `start`, then `main` and `alex4` in turn
during a battle, and `victory` or `defeat` at its end. The river sounds
(`river`, `river1-2`, `FUN_005127b8`) murmur on battlefields with water (🟡
how often is not traced).

**Help.** About This Battle and Victory Conditions (`on_HelpAboutThisBattle`,
`on_HelpVictoryConditions`) opened `ALEX.HLP` (the GOG build has the
WinHelp calls patched out); for the custom battle they show string 83
instead. The rewrite reads the help file itself (`gboh_formats::hlp`, its
layout described there): About This Battle shows the battle's topic
("Hydaspes - 326 BC"), Victory Conditions the battle summary's from its
"STANDARD VICTORY CONDITIONS" ("Hydaspes  (326 BC)"), Help a list of the
topics. About shows `ABOUTDLG`'s texts and `CREDITS.RTF`.

## Cursors

The executable's cursor groups (`RT_GROUP_CURSOR`; images `RT_CURSOR`:
hot spot words, a 1-bit `BITMAPINFOHEADER` of doubled height, the XOR then
the AND bits, rows bottom-up): `BUSYCURSOR`, `SWORD`, `SLING`, `ARROW`,
`BOLT`, `JAVELIN` and `MOVENORTH` … `MOVEWEST` (eight). The game changes
them by name (`on_ChangeCursor`): the `Move…` arrows while the mouse
within 25 pixels of the map window's sides, or 35 of its top and bottom,
scrolls it (the margins at `0x5ec7c4`, set in code), the sword over a unit the selected one may shock,
and with a volley armed a missile over a unit it may shoot (bows `ARROW`,
`SLING`, javelins `JAVELIN`, oxybeles `BOLT`); otherwise the arrow.

## Start screen

`STARTGAME`'s buttons: 106 Start Battle (the scenario map), 101 Load, 102
Start Campaign, 103 Multiplayer, 104 View Tutorial, 105 Historical Video,
2 Exit. The GOG build cuts the help calls: View Tutorial, and the scenario
map's Units… and Details… (bar Paraitacene's, which tells its two
description strings), show a pointer to the manual (the rewrite: string
83, and the help's tutorial topic for View Tutorial). More… lists the
scenario files in `DATA` other than the battles' (`VARIANTDLG`) and plays
the one chosen as battle 11, "Custom". The intro (`INTRO.EXE`, run by the
launcher before the game) and the historical video (`HISTORY.EXE`, run by
button 105, the game not waiting) are Macromedia Director 5 projectors
with their own sound (`INTRO.WAV`, `HISTORY.WAV`). The rewrite plays the
movies inside them itself (`gboh_formats::director`, whose comment
describes the container, cast, bitmaps, palettes, sounds, score and
scripts; the player is `crates/gboh-viewer/src/slides.rs`): 15 frames a
second, negative tempos holding a frame that many seconds (🟡), wipes and
dissolves over their durations, the score's sound channels and the frame
scripts' `sound playFile` (the WAV files), `sound fadeOut` and `quit`; a
key or click ends them. The intro plays at start while Show Intro Video is
ticked. The title
screen plays the `start` music.

opengboh adds four buttons of its own, top left: Content Manager (import
the games), Settings (Select Game, the preferences, the rule options), All
Battles (every imported game's battles and campaigns, and the editor's
scenarios, in one list) and Scenario Editor (see the last sections).

## Campaign

From the campaign object (`AlexCampaignGame`, `CAMPAIGN.CPP`); the rules
are `gboh_game::campaign` (their tables are there), the screens
`crates/gboh-viewer/src/campaign.rs`. Ten rounds; the army's flag moves to
a neighbouring province each round and an unconquered province's battles
are fought in order (Macedon: Lyginus, Pelium; Anatolia: Granicus;
Levant: Issus; Mesopotamia: Gaugamela; Bactria: Samarkand, Jaxartes;
Indus: Arigaeum, Hydaspes; Egypt and Persia fall by marching in). The
player leads the Macedonians; a battle lost ends the campaign, except
Samarkand. Conquest reveals neighbours. At each map, conquered provinces
roll to stay subjugated (`rand() % 100` against `95 − garrison / d`, `d`
by level, more for coasts the fleet holds), and reinforcements gather in
Macedon (12 infantry, 3 cavalry, less 4 and 2 for each province not wholly
subjugated). Nothing of a battle carries over (the coded garrison and
Gaugamela effects never take hold in the original). The results rate the
campaign by the round Indus fell plus the provinces not wholly subjugated
(strings 38–46); taking Bactria is victory, after which the original plays
`320ALEX.AVI` (Indeo 3.2, 320 × 200, 15 fps) in `VIDEODLG`, at (32, 10);
the rewrite has no Indeo decoder and asks `ffmpeg` for the frames and
sound when it is installed (`GBOH_FFMPEG` names another), else goes
straight to the results. 🟡 The opening's dissolve of changed provinces is not drawn.

**Map** (`CAMPAIGNMAP`, on the `Campaign Game` bitmap): the `Campaign Map`
art of `CAMPAIGN.EIF` through a 529 × 357 window at (35, 17), scrolled by
dragging; hidden provinces blacked out, free ones shaded twice and partly
subjugated ones once; each garrison's total in `Numerals` and its icons
(`Units` for infantry, `Dark Units` for cavalry, in 50, 10, 5 and 1); the
army's flag; battle labels in revealed provinces (queued ones framed); the
fleet; the log below. Dragging the flag to a province the army may enter
queues its battles (Battle), else Subjugate takes the province; garrison
icons drag between conquered provinces. A double-click on an icon merges
five Ones into a Five, or splits a larger icon into the next smaller (a
Five into five Ones, a Ten into two Fives, a Fifty into five Tens) when
there are none of those; a province's icons regroup, largest first,
whenever its garrison changes (Hannibal's `FUN_005951c3`).

## Network play

The original's multiplayer used DirectPlay and proxied its message bus;
the rewrite's (`crates/gboh-viewer/src/net.rs`) links two copies of
itself over TCP and sends the players' orders (play being deterministic),
with the original's `STARTSERVER` and `STARTCLIENT` dialogs and its
messages (strings 109, 201, 204, 205, 216, 219). The host chooses the
battle and, on the Leader Control screen, which armies each player leads
(Player: the host's; Computer: the other's); Window → Show Chat sends a
line to the other player.

## Victory 🟡 (`FUN_0054d45b`, `FUN_0054c265`, `FUN_0054d85c`)

Checked at the end of each turn, the Macedonian army being the original's
first side:

1. **Rout levels:** an army whose lost rout points reach its level loses;
   if both have, the one less far past its level wins, and a tie goes to
   the Macedonians, as the manual says (the original gave it to the enemy:
   fixed).
2. **Leaders:** if neither has broken, a killed Alexander loses the battle
   for the Macedonians and a killed Darius for the Persians (`FUN_0054c265`
   matches leader names).
3. **Turn limits:** the Macedonians lose if no army has broken by the end
   of turn 10 at Chaeronea, Lyginus, Pelium, Granicus, the Jaxartes,
   Arigaeum and Hydaspes (the manual says Hydaspes has none), turn 5 at
   Samarkand and turn 7 at Paraitacene; Issus and Gaugamela have none.
4. **Pelium:** unrouted Macedonian units on the west edge (column 0, rows
   5–20) leave across the Apsos, scoring their TQ; once 50 points have
   gone, a Macedonian leader leaving there scores 1000, an immediate
   Macedonian win. (Checked here as each leader finishes; the original
   checks per unit.)

The manual's Jaxartes condition (the Scythians win if no unrouted
Macedonian unit is north of the river by the end of turn 4) is not in the
code: the only battle-specific turn tests are the limits above.

AI against AI with these rules (each side's first plan): every battle but
Issus ends within 10 turns; Arigaeum and the Jaxartes end on the turn limit
(Indian and Scythian wins).

## Sounds 🟡

Plain 16-bit PCM WAV files in `DATA` (22 kHz mono effects, 44 kHz stereo
music). The original names them in tables at `0x61ad34`–`0x61b068`:

* **Movement**, groups of four, picked as `group × 4 + rand() % 4`
  (`FUN_005138f0`): `lilup2-5`, `milup1-2`, `hilup1-4` (twice),
  `horslup2-4`, `eleflup1-3`, `darius` (Darius' chariot), `charlup1-3`,
  `cavlup1-4`, `hcav1-3` (four times), `eleflup`, `oxymove`, `lilup`.
  Which unit uses which group is set in the presentation code (not traced;
  the rewrite maps by name: light, medium and heavy infantry, light and
  heavy cavalry, elephants, chariots, oxybeles).
* **Splashes** `splash`, `splash1-4`, `splasha`, `splashb` (water).
* **Missiles:** bows `bowshot`, `bowshot1` with hits `arohitsa-c`;
  javelins `bowshot` with `javhits1-3`; slings `sling` with `rokhit1-3`;
  oxybeles `oxybeles` with `oxyhit`, `oxyhit1-2`.
* **Shock:** `shoklup1-5`, in 13 rows of five (`param × 5 + rand() % 5`);
  `blows`; elephants `elef1-5`, `elefblow`, hits `elefhit`, `elefhit2-3`;
  horses `horsbat`, `horsbat1-2`, `hors1-4`.
* **Ambient:** `river`, `river1-2`, `waves`.
* **Music:** `start`, `main`, `alex4`, `victory`, `defeat`.

The viewer plays selection, movement, volley (shot and hit), shock (with
elephants), battle start and the end.

## Hannibal and Caesar 🟡 (from Hannibal's executable; Caesar's files only where noted)

The three games share the engine. What differs in their files:

- **Palette** `HANNIBAL.PAL` / `CAESAR.PAL` (same format as `GAME.PAL`). The
  executable is `HANNIBAL.EXE` / `CAESAR.EXE` (Caesar's folder also holds a
  scenario editor). Fonts: no `MS8TEXT.EIF`; the small font is `MSSANS2.EIF`
  ("MS Sans Serif 2", 94 glyphs from `!`). `WALKANIM.EIF` is Alexander's,
  byte for byte. The `.ANI` files are Windows animated cursors.
- **Intro**: `INTRO.AVI` beside `DATA` (Indeo 3.2, 512 × 384, 15 frames a
  second), no Director projector; Hannibal has no history video (its button
  sits off the dialog, at x = −100).
- **Scenarios** (`gboh_formats::scn`, layout `Hannibal`, which Caesar's
  share): a side's rout level is a u16; every unit's common part ends with a
  u16 **arrival turn** (non-zero: the unit enters at its hex on that turn and
  doesn't count toward the rout totals before); leaders add a u8 (elite
  initiative) and an i16 (link group, −1 none) to the four ratings; kind 21
  (Hannibal's legions) and 25 (Caesar's) are missile records with an i16
  legion type (1 hastati, 2 principes, 3 cohort); kind 22 (Roman cavalry)
  reads as heavy cavalry (a cataphract byte); Caesar adds 23 (missile) and
  24 (combat). The layout isn't marked in the file; the parser takes the
  one that reads it through. Nation bytes 6 Roman, 7 Carthaginian, 8
  Seleucid follow Alexander's six. One of Hannibal's files (an early draft
  the game hides) has leaders without the elite byte.
- **Unit types** (`gboh_formats::units::HANNIBAL_TYPES`): the 54 sprite sets
  Hannibal registers, in its order (type ids from `HANNIBAL` = 45 on), each
  with its class; a unit's type follows from kind, nation, missile byte,
  cataphract byte, legion type and parts of its name (for instance
  Carthaginian medium infantry named for Celts are drawn as one of three
  Celtic sets).
- **Missiles**: Hannibal adds "Mounted Javelins" (light cavalry, elephants),
  "Light Javelins" (light infantry) and "Pilum Javelins" (legions) to
  Alexander's four.
- **Water**: the same four frames of colours, on palette indices 41..=45.
- **Music**: Hannibal plays its CD (GOG: `MUSIC/TrackNN.ogg` beside `DATA`):
  the start screen track 2, battles tracks 3, 4, 5 in turn, a defeat 6, a
  victory 7; without it the WAVs (`START`, `TRACK1`..`3`, `VICTORY`,
  `DEFEAT`).

### Hannibal's terrain (`gboh_render::hanland`)

No "Landscape Artwork" files: the block files have the cliffs, the overlay
files the streams.

- **Blocks** as Alexander's (masks, counts, cels), chosen with the 64-bit
  `random()`; the strip offset is 3 at Bagradas and Zama, 2 at Ilipa. Trebbia
  is in snow: `SLAND*.EIF` ("Snow Landscape Blocks": `River` at elevation 0,
  `All Levels` above). Before the blocks, every tile above water draws one
  number for its (hidden) battle scar.
- **Edges** as Alexander's, but an off-map neighbour counts as elevation 9.
- **Cliffs**: where the lower-right (lower-left) neighbour is two or more
  levels down, a half-block `SE` (`SW`) `Cliffs 1-3` (hex at elevation 3) or
  `Cliffs 3-5` cel, cel = tile index mod 4, at (w/2 or 0, 3 − zoom + 3h/4)
  from the tile's top-left (zoom 0, 1, 2), under the block; a same-row
  neighbour one level lower carries on the face one level down. A missing
  lower neighbour (the map's sides) is replaced by the hex two rows down.
- **Streams** (side code 6): each side with a stream draws `Minor River
  Edges` cel 0..6 (N, NE, SE, S, SW, NW); a side without one draws `Minor
  River Corners` where its neighbours' streams meet it. A neighbour exactly
  one level higher moves a piece up a level or swaps it for one of edges cels
  6..16 (at Trebbia too, from the plain set). Cliffs hide the pieces on their
  sides. Offsets per zoom are in the code.
- **Objects**: trees 5 or 6 on a 2 × 2 grid of the hex (from (−65, −32)) and
  one in the centre (snow trees at Trebbia); a stone hex has a single stone at
  (−49, −22). Baecula has towers on six hexes and seven camp wall pieces
  (each with a reversed-view set).

### Hannibal's scenario screen

A map (the add-on's "Scenario Map" where Dertosa's scenario is installed)
with the battles' names painted on it; a click on a name chooses that
battle, which is framed, and the box below shows its overview picture (the
battle's strip in `OVERVIEW.EIF`, its half-size cel) and its description.
The rout dialog adds two checkboxes (turn limits, default deployments). The
difficulty favours the player's army (the home one unless the computer
plays it), in Alexander as well.

### Hannibal's rules (`gboh_game::game::Rules::Hannibal`)

Most of the rules code is Alexander's (the shock, missile, movement,
terrain and rout tables are the same). What Hannibal's executable does
differently, as ported:

- **Classes**: 1 legion (hastati, principes, cohorts: one pilum, never
  resupplied, not armoured against missiles) and 9 Roman cavalry (heavy
  cavalry wherever a rule names horse). The shock tables already have their
  rows.
- **Superiority** is consulted always and last, so attacker and defender can
  both be superior. Elephants: others' attacking Roman elephants are
  superior; elephants other than the Carthaginians' get +1 column against
  other troops; they shake Carthaginian elephants' pre-shock checks. Celts
  attacking (by name) shake the defenders' checks as chariots do.
- **Pursuit**: a defender that breaks (not an elephant) is pursued by the
  first attacking horse unit that held, on `rand() % 10` (+2 against foot) of
  4 (heavy cavalry), 6 (Roman cavalry, lancers) or 8 (light cavalry). It
  rides the original's hex line (a bearing in 15° steps picks a vertex
  direction; each step takes the hex before it nearer the straight line) to
  the routed unit, stopping before an occupied hex and after one in an
  enemy's zone, its facing kept; it catches it on `rand() % 10` ≤ its TQ if
  the unit is then in its front with no other foe there. The caught unit
  checks at TQ 1 (fails on 2..9): foot are cut down; horse, chariots and
  elephants flee again with the pursuer after them (riding off the map's
  edge, it becomes a leaderless unit off that edge). Not caught, or after a chase, enemy leaders beside the pursuer step
  away. Failing the roll, it advances into the hex the unit held, besides
  the shock's one advance.
- **Orderly withdrawal**: horse withdraw only before foot.
- **Group orders** (move, attack, retreat, restore cohesion): roll `random()
  % 100` against `(initiative − 2) × 20 + 30` (one hex: + 50), easier after
  failures; **Roman leaders never fail**; refused only with an enemy beside a
  unit; success takes the whole activation.
- **Manipular line extension**: the whole activation, no roll. In the
  group's order, a maniple (not routed) whose right-front hex holds one of
  the same nation, group and line steps into its left-front hex (by the
  leader's facing) if empty and not in an enemy's zone (the original tested
  neither: fixed, as the manual says): a rear maniple behind-left of a front one comes up on
  its left, so the line extends to the left.
- **Elite initiative**: any leader with the scenario's elite flag, three
  times a battle at most; on the first turn always (his chain of superiors
  followed up), later 40 % for the computer's leaders, while a player's
  is asked (Yes/No; a save records the answer). In the draw a leader's superior goes first if he
  hasn't yet.
- **Victory**: one rout rule; Hannibal's death loses his army the battle;
  turn limits only when the set-up's option is on (at the limit the
  original's result 1 goes to the visitor, 0x10001 to the home army: the
  Romans hold out at Bagradas and Trasimene, the home army wins elsewhere,
  as the manual's victory conditions read).
- Restore cohesion in an enemy's zone: −60 (Alexander −40). Reaction fire
  from a tower (hex code 14) all round. Rampaging elephants trample routed
  units out. Resupply skips legions and isn't an order. Units arrive at the
  start of their turn (the counter from 0) if their hex is empty and no
  enemy leader stands beside it; else they try again each turn.
- Battles: wind at Cannae (70 % a turn; facing into it, 6 or 7, a hex off
  the move's search and the missiles' range, leaders not; stepping back,
  by the unit's own facing, where the original tested the reversed one:
  fixed);
  Trasimene's Romans idle on the first turn; Metaurus' Ligurians and Gauls
  need a Roman within 7 hexes and a TQ check the first time.
- **Backward movement**: a unit steps back through its rear, two movement
  points at most, its facing kept.
- **Deployment phase** (when the set-up's default deployments are off and
  the armies are more than a hex apart): the line halfway between the
  armies across their retreat edges' axis is fixed; the player puts his
  units on his side of it, on passable empty hexes, then Finish begins the
  battle. A save records that the battle began with one.
- **Options** (Hannibal's Preferences): the Triarii tactical doctrine
  (Roman Triarii take no orders while a legion unit stands within 6 hexes,
  unless an enemy other than elephants is within 3), sparse trees (only each
  wooded hex's centre tree shows).
- **Free units** (no leader, group 0xffff): the draw's first entry, of
  initiative 3, in every battle, once a turn; drawn, each closes on its
  nearest enemy and attacks. Those off the map (pursuers that rode off)
  come back on 40 %: at a random hex of their edge that is empty with no
  enemy leader beside it, facing about.
- Group commands ask first (they use all the leader's orders), unless the
  player ticked "don't show this again" (Preferences).

### Hannibal's campaign (`gboh_game::campaign::Kind::Hannibal`)

Alexander's campaign game on other tables (`HannibalCampaignGame`):

- Ten provinces in a ring (0 Carthage … 9 Sicily) with Cisalpine Gaul also
  next to Roma; one table for the routes and what a conquest reveals; the
  army starts in Gaul; Carthage, Numidia, Lusitania and Hispania held
  wholly; the fleet off Gaul, Magna Graecia and Sicily. Staying put isn't a
  march; Roma is never conquered nor subjugated.
- Battles are queued when the map closes with Battle: Numidia's Bagradas,
  Cisalpine Gaul's Trebbia then Trasimene, Magna Graecia's Cannae. Battle is
  offered only after the flag is moved to an unconquered province with
  battles. Three queues: Cannae won brings Baecula and Metaurus the round
  after next, then Ilipa; Metaurus lost (the only loss that doesn't end the
  campaign) brings the Great Plains and Zama.
- Won with Zama, or Cisalpine Gaul, Samnium, Magna Graecia and Sicily wholly
  subjugated (checked every round).
- The opening: no changes in round 1; coasts Gaul, Magna Graecia, Sicily;
  the province of a battle just lost slips for sure; reinforcements in
  Carthage of `max(0, 12s − 4n)` infantry and `max(0, 3s − n)` cavalry, `n`
  the unsubjugated among Numidia, Lusitania, Hispania (not the army's),
  `s` 2 with Magna Graecia wholly held.
- The screen: `CAMPAIGN.EIF` only (its own background); the map's window at
  (35, 8), the map 11 pixels down in it; every province shaded by its
  subjugation (hidden ones too: Area Blackout cels only tell where the
  mouse is; the shading cels are RLE effect runs); no names strip (the names
  painted on the map; for the log the executable's, from `0x655500`), no
  numerals, no battle labels; 2 × 2 dissolve cells. Results: the Victory or
  Defeat background with the Carthaginian's picture at (174, 89), rounds,
  provinces conquered and wholly subjugated, and a rating named by how the
  campaign ended. No video.

### Caesar 🟡 (from Caesar's executable)

Caesar's engine is Hannibal's one version on. Where it differs:

- **Scenarios**: Hannibal's layout. Told apart by their nations (9
  Caesarian, 10 Pompeian, 11 barbarians: Gauls, Germans, Helvetii, Nervii,
  Cimbri; 0 is Mithridates' Pontic army, 6 Roman) or by kinds 23 (barbarian
  foot, a missile record), 24 (barbarian horse, a combat record) and 25
  (cohorts, a missile record whose i16 is 1 veteran, 2 recruit, 3
  conscript). The header's nation pairs also choose the art: a unit whose
  nation is a pair's first takes the second's art (Sulla's and Marius'
  armies are drawn as Romans, the Romans as Pompeians). The missile byte
  adds 5, a thrown axe.
- **Unit types** (`gboh_formats::units::CAESAR_TYPES`, ids from `CAESAR`
  on): Caesar's 59 sprite sets in its order, each with its class (three
  new: barbarian infantry and cavalry, which fight only hand to hand, and
  artillery: scorpions and ballistas) and the missile its men throw. A
  unit's set follows from kind, nation, missile byte, cohort type and parts
  of its name; leaders' names are matched whole (`J. Caesar`, `Pompey`,
  `Beorix` have their own figures). Where the game picks one of three sets
  (A, B, C) at random, opengboh picks by the name.
- **Missiles**: "Axes" joins the table at index 3, so Hannibal's later
  entries move one up.
- **Terrain** (`gboh_render::hanland`, the overlay file has `Minor Rivers`):
  as Hannibal's, except that
  - a stream is drawn as one piece per hex corner whose two sides both have
    it (three variants each), and an end cap where it stops (which a
    neighbour's corner piece clears);
  - hex code 12 is a road: each road hex joins up to three pieces toward
    its road neighbours (or the map's edge); on elevation 0 the piece is a
    bridge, and a `Road Fix` cel covers a step up to an upper neighbour;
  - hex code 3 is marsh (a tile overlay); side code 10 is a ditch, or a
    berm on hexes with code 4 and two gaps in Caesar's line at
    Dyrrhachium (one piece per corner, as streams);
  - side codes 9 and 11 are walls, objects drawn on each hex's south,
    south-west and north-west sides; towers stand on hexes the game lists
    for Dyrrhachium, Pharsalus, Thapsus, Munda and Bibracte;
  - no snow; Ruspina's strips start four on; the tiles are drawn a layer at
    a time (cliffs, blocks, overlays, ditches), each overlay placed by its
    cel's hotspot.
- **AI scripts** (all three games' language): procedures may take `var`
  parameters, passed by reference, and overload on how many they take
  (`BASICMAN.AI` has two `FigurePriority`). Some of Caesar's plans use
  leader constants (`TRIBUNE_VI`, `J_CAESAR`) that nothing defines, so the
  original can't compile them, and Hannibal's `CONSTANT.AI` lists its
  leaders under numbers no scenario's groups have, so its plans address
  the wrong groups. Fixed: opengboh names each group after its leader
  (words in capitals joined by `_`, its group id the value), replacing
  those. A
  plan's `Orders for` names a leader by the letters and digits of his name
  (`TribuneIIICaesarian`). `FindHurtingUnit` takes the hits to exceed.
- **Rules** (`gboh_game::game::Rules::Caesar`): Hannibal's engine without
  its nations' and battles' rules (no wind, Triarii, unreliable troops,
  Celts' or elephants' extras; superiority in Alexander's order; no
  leader's death ends a battle). Then:
  - classes: barbarian infantry (4) and cavalry (10, a horse to every rule);
    cohorts move as legions but fight by type: veterans as legionaries,
    recruits as heavy infantry, conscripts as light; scorpions and
    ballistas as oxybeles;
  - missiles: thrown axes (range 1, two volleys, hit on 7); ballistas by
    their own range table (9, 7, 6, 4, 3, 2, 1); a cohort's single pilum;
    barbarian bows −1; a cataphract denies any defender superiority;
  - rallying marks a unit for good (but first and tenth cohorts): a point
    worse on TQ checks, later rallies and shooting, and a
    column worse attacking (better defending; the original tests the
    attacker twice, so its shifts cancel: fixed);
  - terrain: road to road a flat point; ditches a point more (another onto
    a berm), walls impassable, elevation 0 counted as 1; chariots and
    elephants keep off the lowest ground and marsh, chariots off woods,
    rough ground and across ditches and streams; horses never step next to
    an enemy elephant; its own cohesion-hit rows; at Dyrrhachium a berm
    shifts the shock column −2 (attacker on it) or −3 (defender);
  - command: Romans, Caesarians and Pompeians never fail a group order and
    extend the line (cohorts of the same type); staff leaders (no units of
    their own) pay no more to order another's group;
  - battles: Sabis' Caesarians sit out the first turn; turn limits
    (Pharsalus 7, Thapsus 5, Munda 11, Vercellae, Chaeronea, Bibracte and
    Sabis 5, Rhine 4) go to the visitor (the Caesarians must win by then);
    at Ruspina the Caesarians win once none of theirs is left on the field
    (they broke out).
- **Interface**: its battle table (ten battles, Rhine only where installed;
  each description two strings joined), the scenario screen's info box at
  (41, 200, 382, 145), its own side names, Group Attack in the toolbar's
  place of the line extension, CD tracks 2 start, 3 and 4 the battle, 5
  defeat, 6 victory. Its start screen's buttons are bare captions on the
  painting (`Big Stone Pushbutton`'s `Up` cel is an empty placeholder), the
  stone showing only while one is pressed (the rewrite draws them with the
  `Big Pushbutton` its data also has, to be readable); the scenario screen opens on the
  first battle, the description running on under the picture at full
  width (as Hannibal's).
- **Variants** (Hannibal's and Caesar's): OK on a battle that has `*.var`
  files asks which to play (`VARIANTDLG`: "Standard", string 47, then the
  variants' descriptions); a variant names its battle by the original's
  full table.

### Caesar's campaign (`gboh_game::campaign::Kind::Caesar`)

The civil war on thirteen provinces (0 Hispania … 3 Roma … 12 Mauretania;
names from the executable). Caesar starts in Samnium; Pompeian armies in
Graecia, Numidia, Hispania and Egypt, Pompey in Roma. Each round Caesar's
flag moves to a neighbour by land or sea, then Finish; entering an army's
province fights its battle (Pharsalus, Thapsus, Munda, Ruspina). A victory
destroys the armies there; in Roma it drives Pompey out (or rioting costs
Caesar's troops a point of TQ in later battles), in Egypt hunger costs a
point and Cleopatra may keep him a round; Pompey flees to one of his
armies. The Pompeians march: an army at home sets out one round in four (in
two with Pompey), Graecia's and Numidia's once by sea, else by land by set
routes ending in Roma; Hispania's by Massilia; Egypt's stays. All four
destroyed within ten rounds wins; a battle lost, or an army (or Pompey after
round 1) in Roma, loses. The rating is named by the armies destroyed. The
map has no shading or garrisons: the flags (Caesar's, the Pompeians',
Pompey's) and the two fleets.

## Rule options (`gboh_game::options`)

The games as they shipped, their plain bugs fixed (above), are the
default. Where a fix or a later game's rule would change the balance, it is
an option instead (Settings → Rules in the launcher; a save records the
options its battle was fought by, and a network game's host sends its own
to the other player):

- **Manual turning:** elephants and chariots pay 2 movement points per
  turn, and also Alexander's cataphracts and Caesar's heavy cavalry (the
  games charge every class 1).
- **Manual shock:** the manuals' terrain adjustment (Alexander's and
  Hannibal's the best for the defender: a column left if all defenders
  stand in a river, marsh or woods, or any stands higher than the attackers,
  or any attacker attacks across a steep-banked stream; two if all do; else
  a column right if all stand lower and none in a river or marsh; Caesar's
  cumulative, with infantry in rough ground and attacks across rivers);
  with a skirmisher on either side the others' hits halve; no size ratio
  where elephants or chariots fight, and the ratio rounded in the attacker's
  favour when one moved; a phalanx's front −3 only against a moving attack
  not all of phalanxes; defending heavy cavalry 2L (Caesar: cataphracts)
  and no +2 for a cataphract attacker; the charge only for the classes the
  manuals list (heavy and medium infantry, phalanxes, lancers, elephants;
  Alexander's chariots, Caesar's cohorts), none attacking skirmishers,
  phalanxes and heavy infantry steady only against a frontal light
  infantry charge, a charging elephant shaking defenders in the later
  games; and no leader casualties in shock.
- **Manual missiles:** shooting into woods a point harder; replenishment up
  to half the supply, rounded up; a leader beside the target hit 15 times
  in 100 (the games 16); Caesar's routed units cut down by a missile hit.
- **Manual movement:** a unit that began its order in an enemy's zone
  leaves it only outpacing every enemy holding it there (Alexander's also
  through a free front hex; his light cavalry against cataphracts
  regardless); Caesar's skirmishers move through zones; skirmishers with
  missiles left hold their flank hexes too.
- **Manual command:** group moves at most 90 % on a first try and 20 %
  easier after a failure; every leader's group move succeeds in Caesar; a
  leader stepping away from an enemy is hurt 5 times in 100 (the games 90
  from cavalry, 60 from others).
- **Manual victory:** Alexander's Agrianians 2 rout points, "Alexander
  (CH)" at Chaeronea a leader's 5 × initiative, and no turn limit at
  Hydaspes; Hannibal × 10 and his death no end of the battle;
  Cynoscephalae's limit turn 10; Caesar's artillery its TQ; Vercellae's
  heat (the Gauls, but their horse, a point worse on TQ checks from turn 3,
  two from turn 5).
- **From Hannibal to Alexander:** cavalry pursuit after a shock; a
  deployment phase before battle; backward movement; group retreat,
  attack and restore cohesion (the rewrite adds these to Alexander's
  menus).
- **From Caesar to Alexander and Hannibal:** road to road a flat movement
  point; rallied units stay marked; staff leaders (no units of their own)
  pay nothing extra to order another's group; and for Hannibal,
  superiority in Alexander's order (no stacking).

## The Collector's Edition and its scenario editor

The *Great Battles Collector's Edition* (Caesar's GOG folder) adds
`gbce.exe`, Caesar's engine with every unit of the three games, which plays
scenarios made with `DATA/scenario editor.exe` (the editor keeps its unit
catalogue in `DATA/Scenario Editor/`). opengboh reads and plays these
scenarios by Caesar's rules, and has its own editor after the original.

- **Scenarios** (`gboh_formats::scn::Layout::Editor`): a u32 0 and a u32
  version 2 mark them, then a GUID, title, author, description, the map,
  both sides (nation, retreat edge, u16 rout level), alliance pairs, the AI
  plans, a turn limit (on, turns − 1, the side that must win by then), the
  groups (id, the name the plans know it by, twenty timed orders: type 1
  defend, 2 move, 3 attack, turns, hex; a behaviour, 1 defend or 3 attack)
  and the units. A unit is a u32 kind (0 leader … 17 cohort), name, facing,
  movement, nation, hex, a −1, its **artwork** (an index into the editor's
  table of 120 sprite sets), its catalogue id and its group, then the
  kind's fields (leaders: initiative, command range, charisma, elite;
  combat units: TQ, size, two-hex, a cataphract byte for heavy cavalry;
  missile units also the missile, range and supply; legionaries and
  cohorts their type). The nation only decides the side: the art comes
  from the artwork, so armies mix the games' units.
- **Maps**: the games' layout behind a u32 0 and a GUID.
- **The unit catalogue** (`MainUnits.unt`): a u32 count and as many unit
  records, 2,632 units of all twelve nations.
- **Packages** (`.psf`, `gboh_formats::psf`): the editor's way of passing
  a scenario around. A u8 1 and a u8 0, an i32 index of the last file,
  each file's name (u8 length and bytes) and u32 size, then the files'
  bytes in that order: map, scenario, home plan, visitor plan. The game
  doesn't read them.
- **AI plans**: written by the editor on saving, one per side
  (`<name>_H1.AI`, `<name>_V1.AI`), from its groups' orders: each group
  carries out its orders in turn, each for its turns (a move until the hex
  is reached), then attacks or defends as its behaviour says.

The editor (the start screen's **Scenario Editor**, on Caesar's data):
**Map** paints levels, water, woods, rough ground and roads as a brush
(as the button goes down, and on along a drag). No two neighbours may be
more than a level apart: where the original refuses such a level, the land
around is raised or lowered to slope down to it (water is never changed
so; a level too high beside water is refused). Streams and walls go on the
side of the hex nearest the click; its **Properties** brush shows a hex's level and
ground and sets its six sides, **Clear hex** clears them, and the map
resizes (keeping its terrain, refused if units would fall off; the width
odd as in the original, but up to 89 hexes a side, the games' largest,
where the original stops at 50);
**Units** places units from the catalogue, every unit fighting for one of
the two sides and joining a leader's group (a leader forms one), facing
the enemy from its side's retreat edge, a two-hex unit needing its second
hex free; **Groups** gives each group its orders and behaviour;
**Scenario** sets the title, sides, retreat edges, rout levels and turn
limit; **File** starts on a map (or a blank one), opens a scenario (a
game's own is taken over, its units keeping their art and stats) or a
package (unpacked into the scenarios folder, asking before replacing a
different file), saves it with its map and plans to opengboh's
`scenarios` folder (never the game's), packs it, and plays it.

## opengboh's own folder

opengboh keeps everything it writes in one folder of the platform's
application data (`~/Library/Application Support/opengboh` on macOS,
`%APPDATA%\opengboh` on Windows, `$XDG_DATA_HOME/opengboh` or
`~/.local/share/opengboh` elsewhere), the parent of the saves folder (so
`GBOH_SAVES` moves it all). Nothing is written to the games' folders.

```text
games/alexander/ALEX.EXE, games/alexander/DATA/...   imported copies
games/hannibal/..., games/caesar/...
settings.txt      "game <key>" (the game selected; earlier "theme <key>")
                  and "options <key> ..." (the rule options on)
preferences.txt   the game preferences, "<name> <value>" a line
saves/            saved games (*.sav), Autosave.sav
scenarios/        the editor's scenarios, maps, AI plans and packages
```

The game keys are `alexander`, `hannibal` and `caesar`; the option keys
are those of `gboh_game::options::ALL` (see *Rule options*).
