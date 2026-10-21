# Data formats

Notes on the file formats used by Erudite's *Great Battles of History* engine.
Verified against *The Great Battles of Alexander* (1997, patch 1.3, GOG);
Hannibal and Caesar not yet checked. All values little-endian. Status: ✅ understood,
🟡 partial, ❓ not yet looked at.

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
per-unit-type movement data (names such as `Foot`, `Mounted`, `Phalanx` and
float factors). ❓

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
game even though the `River` cels have ripple texture. The frame rate is not
yet known.

The five remap tables map a palette index to another index:

| table | effect (mean luminance ratio) | likely use |
|---|---|---|
| 0 | ×0.74, darker | shadow (index 255 in kind 3 sprites, confirmed; probably also RLE ops 3/4) |
| 1 | ×0.86, darker | lighter shadow / translucency |
| 2 | ×1.39, brighter | highlight |
| 3 | ×1.19, brighter | highlight |
| 4 | ×1.31, brighter | highlight |

## Other files ❓

| ext | count | notes |
|---|---|---|
| `.MAP` | 12 | battle maps, see below 🟡 |
| `.SCN` | 11 | scenarios, see below 🟡 |
| `.AI` | 45 | plain-text AI plans (`'` comments) |
| `EIFFILE.HED` | 1 | "Activation Unit Flags", 1.1 MB, structure unknown |
| `.WAV`, `.AVI`, `.RTF`, `.HLP` | | standard formats |

## MAP: battle maps ✅ structure / 🟡 feature meanings

```text
file  := u16 name_len, name[name_len], u8 width, u8 height,
         u16 hex_id_origin, u16 unk_b, u8 elevation[width*height],
         cell[(width*2+2) * (height*4+1)]
cell  := u16 count, u16 feature[count]       (count <= 12)
```

All 11 battle maps (plus an unused `parait.map`, Paraitacene) parse with no
bytes left over. `EIFFILE.MAP` is a different file (resource index).

* Hexes are in columns; **odd columns** are shifted down half a hex.
* `elevation`: 0 = water (sea/river), 1..5 = land levels. (The manual says major
  rivers count as level 1; the loader flags level-0 hexes specially.)
* The feature grid uses doubled coordinates: hex `(x, y)` is at cell
  `(2x + 2, 4y + 2 + 2*(x & 1))` (grid column 0 is unused). This was checked
  by drawing minor streams and matching them pixel for pixel against the
  running game; an earlier reading without the `+2` offsets put every
  feature one hex off. Its six sides are the cells at offsets
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
  block's top-left + (0 or w/2, ¾·face + 2). If the same-row neighbour on that
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

## Leaders and command 🟡

**Leader ratings.** The four leader values in a scenario are, in file order:

| value | meaning | evidence |
|---|---|---|
| 1 | charisma | 0–4; Alexander 4, Tauron 0; named in `CONSTANT.AI`; picks the leader in a shock's casualty roll |
| 2 | command range | passed with the leader's hex to range searches; reduced by wounds |
| 3 | initiative | shown in game as `I:`; also the starting command points (`Com: n/n`) |
| 4 | personal combat | named in `CONSTANT.AI`; reduced by wounds with the command range; added to the leader casualty roll |

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

**Zones of control.** A unit may enter a hex next to an enemy combat unit
but not move on from it (unless it started there); skirmishers ignore this.
The original also asks each enemy whether it exerts the zone; that test is
not ported.

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
main hex swings. An about-face (three steps) swaps a two-hex unit's hexes.
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

**Ordered again.** Units are marked as ordered when a leader activates them.
A unit ordered by a second leader in the same turn takes one cohesion hit
("… takes 1 cohesion hit for being ordered again this turn.").

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
column against the target's class. An attacker in the target's flank or
rear (either hex of a two-hex unit) takes the lead if its score is at least
the best so far, and sets the angle to flank or rear; a frontal attacker
takes the lead only while no flanker has, with the same test; ties go to
the later unit. So a flanker scoring below an earlier frontal attacker
brings no flank advantage. (The manual says the most advantageous angle is
used.)

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

A bolt at range 7 reads past the table into the next byte (85), so it
always hits. Modifiers: +1 against phalanx or heavy infantry, +3 more when
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
be empty; failing that, a hex with friendly units is used (routing through
them; per the manual they take a cohesion hit). Reaching the map edge or
finding no step takes the unit out of the battle (terminal rout or
elimination), and its rout points count against its army.

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
* **Leader casualties:** finished leaders skip the rest of the turn;
  wounded ones lose 2 command range and 2 from the fourth rating (the
  original's wound message carries a 2); a second wound, or "killed",
  removes them.

* **Orderly withdrawal:** when a unit moves next to cavalry (it not being
  cavalry), a leader, or skirmishers (it being heavy infantry or phalanx),
  they make a TQ check (`rand() % 10 ≤ TQ − hits`) and on success take one
  hit if approached from flank or rear (skirmishers: rear only) and back
  away up to two hexes, keeping facing, avoiding enemy zones, rough terrain
  and occupied hexes.
* **Routing through friends:** friendly units on a routed unit's path take
  one cohesion hit.

Not yet: rampaging elephants and a computer opponent.

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
  odd))`; the ratio of `|dy|` to `|dx|` (0, equal, under, 3×, under 3×,
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

The commentary also showed that **AI leaders always try for momentum**, and
that after a successful momentum the leader's units move again, those that
had already moved taking the "ordered again" cohesion hit (Craterus's
Prmsida, Arachos and Companions; Antigenes's four phalanxes). The rewrite
now does both (`Game::momentum_refresh`; 🟡 the original's
`FUN_0052b2e2` restores movement and clears "done", but the message that
triggers it is not traced). Two
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
  visitor's, then the leaders the same way.
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
