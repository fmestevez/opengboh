//! Isometric terrain blocks (see `docs/formats.md`, "Terrain rendering").
//!
//! The screen is the map rotated 90°: screen row `r` = map `x`, screen column
//! `c` = `height - 1 - y`. Even rows sit half a block to the right of odd rows.
//!
//! The original has two block art families. When the `LAND*.EIF` files exist
//! (they ship with the game) it uses those "Landscape Blocks"; the older
//! `NLAND`/`ZLAND` "Landscape Artwork" sets are then only used for cliffs,
//! streams and roads. Only the blocks path is implemented here.

use crate::{BorlandRand, Canvas, Sprite, strip};
use gboh_formats::eif::Eif;
use gboh_formats::map::{Map, Side};

/// The three landscape zoom levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zoom {
    /// "Zoom In".
    In,
    /// The default view.
    Normal,
    /// "Zoom Out".
    Out,
}

impl Zoom {
    /// `(block width, block height, top face height)`, from the original's
    /// static tables.
    pub fn block(self) -> (i32, i32, i32) {
        match self {
            Zoom::In => (130, 76, 64),
            Zoom::Normal => (66, 38, 32),
            Zoom::Out => (34, 19, 16),
        }
    }

    pub fn row_pitch(self) -> i32 {
        self.block().2 * 3 / 4
    }

    /// Height of one elevation level (and of a bank).
    pub fn level_step(self) -> i32 {
        let (_, h, face) = self.block();
        h - face
    }

    /// The "Landscape Artwork" file with streams, cliffs, roads and fords.
    pub fn artwork_file(self) -> &'static str {
        match self {
            Zoom::In => "NLAND.EIF",
            Zoom::Normal => "ZLAND.EIF",
            Zoom::Out => "Z2LAND.EIF",
        }
    }

    /// Offsets of the six `Minor Rivers` side pieces (cels 0..6) from the
    /// block's top-left corner: left, upper-left, upper-right, right,
    /// lower-right, lower-left.
    fn stream_sides(self) -> [(i32, i32); 6] {
        match self {
            Zoom::In => [(0, 27), (3, 12), (65, 11), (119, 29), (65, 57), (3, 57)],
            Zoom::Normal => [(0, 13), (1, 6), (33, 5), (60, 14), (33, 28), (2, 28)],
            Zoom::Out => [(-1, 7), (1, 3), (16, 3), (30, 7), (16, 14), (1, 14)],
        }
    }

    /// Offsets of the nine corner joins (cels 6..15).
    fn stream_joins(self) -> [(i32, i32); 9] {
        match self {
            Zoom::In => [(-9, 28), (-9, 55), (-9, 28), (55, 8), (0, 21), (0, 9), (120, 23), (65, 10), (65, 10)],
            Zoom::Normal => [(-5, 14), (-5, 27), (-5, 14), (28, 4), (-1, 10), (0, 4), (61, 11), (32, 5), (32, 5)],
            Zoom::Out => [(-3, 7), (-3, 14), (-3, 7), (14, 2), (-1, 5), (0, 2), (31, 6), (16, 3), (16, 3)],
        }
    }

    /// Green variant of [`artwork_file`](Self::artwork_file), used for
    /// cliffs when the map's `unk_b` is 2.
    pub fn green_artwork_file(self) -> &'static str {
        match self {
            Zoom::In => "NLANDG.EIF",
            Zoom::Normal => "ZLANDG.EIF",
            Zoom::Out => "Z2LANDG.EIF",
        }
    }

    /// The block art file ("Landscape Blocks").
    pub fn blocks_file(self) -> &'static str {
        match self {
            Zoom::In => "LAND.EIF",
            Zoom::Normal => "LAND2.EIF",
            Zoom::Out => "LAND4.EIF",
        }
    }
}

/// Cels per neighbour mask in a block strip, in strip order. Land strips
/// start with [`BOTH_LOWER_CELS`] extra cels; the `River` strip does not.
const MASK_COUNTS: [usize; 16] = [7, 3, 1, 3, 1, 1, 2, 1, 3, 1, 1, 1, 3, 1, 1, 1];
/// Leading cels of land strips used when both upper neighbours are lower.
const BOTH_LOWER_CELS: usize = 2;
/// Flat blocks (mask 0) pick one of the first `FLAT_RARE` cels 1 time in 7.
const FLAT_RARE: usize = 3;

/// Map side code for a minor stream.
const STREAM: u16 = 6;
/// Map hex codes: a road, and impassable river rocks (which roads also
/// join up with).
const ROAD: u16 = 12;
const RIVER_ROCKS: u16 = 13;
/// Map hex code for stones; in elevation-0 hexes they are drawn as river
/// rocks.
const STONES: u16 = 2;

/// Decoded terrain art for one zoom level.
pub struct TerrainArt {
    /// Block strips: `River`, `Land1`, `Land2`, ...
    pub strips: Vec<Vec<Sprite>>,
    /// `Minor Rivers`: six side pieces, then nine corner joins.
    pub streams: Vec<Sprite>,
    /// `Cliff 0`..`Cliff 3`, indexed by the lower elevation; cel = drop - 2.
    pub cliffs: Vec<Vec<Sprite>>,
    /// `Roads` from the artwork file: seven block-sized pieces.
    pub roads: Vec<Sprite>,
    /// `River Impass` from the zoom's object file (see
    /// [`Zoom::objects_file`]): block-sized rocks-in-water overlays.
    pub river_rocks: Vec<Sprite>,
    /// "Landscape Background" (`LANDBACK.EIF`), tiled behind the map.
    pub background: Option<Sprite>,
}

impl TerrainArt {
    /// `blocks` is the zoom's "Landscape Blocks" file, `artwork` its
    /// "Landscape Artwork" file (see [`Zoom::artwork_file`]) and `cliffs`
    /// the same or its green variant.
    pub fn load(blocks: &Eif, artwork: &Eif, cliffs: &Eif) -> Option<Self> {
        let mut strips = vec![strip(blocks, "River")?];
        while let Some(s) = strip(blocks, &format!("Land{}", strips.len())) {
            strips.push(s);
        }
        let streams = strip(artwork, "Minor Rivers")?;
        let cliffs = (0..4).map(|n| strip(cliffs, &format!("Cliff {n}"))).collect::<Option<_>>()?;
        let roads = strip(artwork, "Roads")?;
        (streams.len() >= 15 && roads.len() >= 7).then_some(Self {
            strips,
            streams,
            cliffs,
            roads,
            river_rocks: Vec::new(),
            background: None,
        })
    }
}

/// Screen layout of a map.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub rows: usize,
    pub cols: usize,
    pub zoom: Zoom,
    /// The reversed view: the map turned half round.
    pub reversed: bool,
}

impl Layout {
    pub fn new(map: &Map, zoom: Zoom) -> Self {
        Self { rows: map.width, cols: map.height, zoom, reversed: false }
    }

    /// The layout of the reversed view.
    pub fn reversed(map: &Map, zoom: Zoom) -> Self {
        Self { reversed: true, ..Self::new(map, zoom) }
    }

    /// The map hex of block `(r, c)`. Normally row `r` is map column `x` and
    /// the columns run up the map; reversed (`FUN_0045cc35`), rows run back
    /// from the last map column and columns down the map, a map column's
    /// odd-ness pushing it one block right. Either way the first block of
    /// an unshifted row has no hex.
    pub fn to_map(&self, r: usize, c: usize) -> (usize, usize) {
        if self.reversed {
            let x = self.rows - 1 - r;
            (x, c.saturating_sub(x & 1))
        } else {
            (r, self.cols - 1 - c)
        }
    }

    /// The block `(r, c)` of map hex `(x, y)`.
    pub fn to_tile(&self, x: usize, y: usize) -> (usize, usize) {
        if self.reversed { (self.rows - 1 - x, y + (x & 1)) } else { (x, self.cols - 1 - y) }
    }

    /// The map side shown as screen side `side` (turned half round when
    /// reversed).
    pub fn side(&self, side: Side) -> Side {
        if !self.reversed {
            return side;
        }
        let i = Side::ALL.iter().position(|&s| s == side).unwrap_or(0);
        Side::ALL[(i + 3) % 6]
    }

    /// The facing to draw for scenario facing `facing`: turned four screen
    /// steps when reversed (the original's `FUN_0044ea4d(f, 4)`).
    pub fn facing(&self, facing: u8) -> u8 {
        if self.reversed { (facing + 4) % 8 } else { facing }
    }

    pub fn shifted(r: usize) -> bool {
        r.is_multiple_of(2)
    }

    /// Space above row 0: five elevation levels plus room for the tallest
    /// terrain objects standing on the top row.
    fn top_margin(&self) -> i32 {
        let (_, h, _) = self.zoom.block();
        5 * self.zoom.level_step() + 2 * h
    }

    /// Top-left pixel of the block at `(r, c)` at elevation 0, relative to
    /// the top of the canvas returned by [`render`].
    pub fn block_origin(&self, r: usize, c: usize) -> (i32, i32) {
        let (w, _, _) = self.zoom.block();
        let x = c as i32 * w + if Self::shifted(r) { w / 2 } else { 0 };
        let y = r as i32 * self.zoom.row_pitch() + self.top_margin();
        (x, y)
    }

    pub fn canvas_size(&self) -> (usize, usize) {
        let (w, h, _) = self.zoom.block();
        // one extra block so the background shows past the right edge
        let width = self.cols as i32 * w + w / 2 + w;
        let height = (self.rows as i32 - 1) * self.zoom.row_pitch() + h + self.top_margin();
        (width as usize, height as usize)
    }

    /// Screen neighbours `(lower-left, lower-right)`.
    pub fn lower(&self, r: usize, c: usize) -> [Option<(usize, usize)>; 2] {
        let at = |r: usize, c: isize| (r < self.rows && c >= 0 && (c as usize) < self.cols).then_some((r, c as usize));
        let c = c as isize;
        let (ll, lr) = if Self::shifted(r) { (c, c + 1) } else { (c - 1, c) };
        [at(r + 1, ll), at(r + 1, lr)]
    }

    /// Screen neighbours `(left, upper-left, upper-right, right)`.
    pub fn neighbours(&self, r: usize, c: usize) -> [Option<(usize, usize)>; 4] {
        let at = |r: isize, c: isize| {
            (r >= 0 && c >= 0 && (r as usize) < self.rows && (c as usize) < self.cols)
                .then_some((r as usize, c as usize))
        };
        let (r, c) = (r as isize, c as isize);
        let (ul, ur) = if Self::shifted(r as usize) { (c, c + 1) } else { (c - 1, c) };
        [at(r, c - 1), at(r - 1, ul), at(r - 1, ur), at(r, c + 1)]
    }
}

/// Neighbour mask of a block: bits 1/2/4/8 = left/upper-left/upper-right/right
/// neighbour is exactly one level higher. Returns `None` when both upper
/// neighbours are lower and nothing else applies (the original's mask 0x30).
pub fn block_mask(map: &Map, layout: &Layout, r: usize, c: usize) -> Option<usize> {
    let elev = |(r, c): (usize, usize)| {
        let (x, y) = layout.to_map(r, c);
        map.elevation_at(x, y) as i32
    };
    let z = elev((r, c));
    let [left, ul, ur, right] = layout.neighbours(r, c).map(|n| n.map(elev));
    let mut mask = 0;
    let mut lower = 0;
    if left == Some(z + 1) {
        mask |= 1;
    }
    match ul {
        Some(e) if e == z + 1 => mask |= 2,
        Some(e) if e < z => lower |= 1,
        _ => {}
    }
    match ur {
        Some(e) if e == z + 1 => mask |= 4,
        Some(e) if e < z => lower |= 2,
        _ => {}
    }
    if right == Some(z + 1) {
        mask |= 8;
    }
    if mask == 0 && lower == 3 { None } else { Some(mask) }
}

/// Pick the cel within a block strip the way the original does
/// (`FUN_0045d7e9`), consuming `rand()` calls in the same order.
pub fn block_cel(mask: Option<usize>, elevation: u8, rng: &mut BorlandRand) -> usize {
    let Some(mask) = mask else {
        return rng.next() as usize % BOTH_LOWER_CELS;
    };
    let base = if elevation == 0 { 0 } else { BOTH_LOWER_CELS };
    let start: usize = MASK_COUNTS[..mask].iter().sum();
    let count = MASK_COUNTS[mask];
    let mut pick = rng.next() as usize % count;
    if mask == 0 {
        pick = if pick == 0 {
            rng.next() as usize % FLAT_RARE
        } else {
            rng.next() as usize % (count - FLAT_RARE) + FLAT_RARE
        };
    }
    base + start + pick
}

/// Stream pieces for one hex as `(cel, offset index)` pairs, in the order
/// the original attaches them (`FUN_0044f689`). Side pieces use cels 0..6 and
/// [`Zoom::stream_sides`]; joins use cels 6..15 and [`Zoom::stream_joins`].
fn stream_pieces(map: &Map, layout: &Layout, x: usize, y: usize) -> Vec<(usize, bool)> {
    let has = |side| map.side_features(x, y, layout.side(side)).contains(&STREAM);
    let (mut a, mut b, mut c) = (0, 0, 0);
    if has(Side::SE) {
        a += 1;
    }
    if has(Side::S) {
        a += 4;
        b += 1;
    }
    if has(Side::SW) {
        a += 2;
        b += 4;
        c += 1;
    }
    if has(Side::NW) {
        b += 2;
        c += 4;
    }
    if has(Side::N) {
        c += 2;
    }
    let mut out = Vec::new();
    // Each corner: a side piece if its own side has a stream, else a join
    // chosen by which of the two adjacent sides do.
    for (corner, n) in [a, b, c].into_iter().enumerate() {
        match n {
            4.. => out.push((corner, true)),
            2 => out.push((6 + corner * 3, false)),
            1 => out.push((6 + corner * 3 + 1, false)),
            3 => out.push((6 + corner * 3 + 2, false)),
            _ => {}
        }
    }
    for (piece, side) in [(3, Side::N), (4, Side::NE), (5, Side::SE)] {
        if has(side) {
            out.push((piece, true));
        }
    }
    out
}

/// Elevation the original assumes for neighbours off the map edge (it
/// reuses the zoom flags: 0, 1 or 2).
fn off_map_elevation(zoom: Zoom) -> i32 {
    match zoom {
        Zoom::In => 0,
        Zoom::Normal => 1,
        Zoom::Out => 2,
    }
}

struct Painter<'a> {
    map: &'a Map,
    art: &'a TerrainArt,
    layout: Layout,
    strip_offset: usize,
    canvas: Canvas,
    rng: BorlandRand,
    /// River rocks cel per block, chosen up front.
    river_rocks: std::collections::HashMap<(usize, usize), usize>,
}

impl Painter<'_> {
    fn elev(&self, (r, c): (usize, usize)) -> i32 {
        let (x, y) = self.layout.to_map(r, c);
        self.map.elevation_at(x, y) as i32
    }

    fn elev_or_edge(&self, n: Option<(usize, usize)>) -> i32 {
        n.map_or(off_map_elevation(self.layout.zoom), |n| self.elev(n))
    }

    /// Top-left of the block at `(r, c)` including its elevation.
    fn origin(&self, r: usize, c: usize) -> (i32, i32) {
        let (x, y) = self.layout.block_origin(r, c);
        (x, y - self.elev((r, c)) * self.layout.zoom.level_step())
    }

    /// Whether the original has a tile here: it leaves the first hex of
    /// every unshifted row empty, and neighbour rules treat it as off-map.
    fn tile(&self, n: Option<(usize, usize)>) -> Option<(usize, usize)> {
        n.filter(|&(r, c)| c != 0 || Layout::shifted(r))
    }

    /// The hex's road attribute: the last road or river-rocks code in its
    /// feature list.
    fn road_attr(&self, n: Option<(usize, usize)>) -> Option<u16> {
        let (x, y) = self.layout.to_map(n?.0, n?.1);
        self.map.hex_features(x, y).iter().rev().copied().find(|&f| f == ROAD || f == RIVER_ROCKS)
    }

    /// Road piece for a road hex (`FUN_0045d7e9`). A road enters from the
    /// upper-left, left or lower-left neighbour (1, 2, 3) and leaves to the
    /// upper-right, right or lower-right one (4, 7, 10), preferring
    /// neighbours with a road, then off-map sides; the cel is the sum - 6.
    fn road_cel(&self, r: usize, c: usize) -> Option<usize> {
        if self.road_attr(Some((r, c))) != Some(ROAD) {
            return None;
        }
        let [left, ul, ur, right] = self.layout.neighbours(r, c).map(|n| self.tile(n));
        let [ll, lr] = self.layout.lower(r, c).map(|n| self.tile(n));
        let is = |n, code| self.road_attr(n) == Some(code);
        let joins = |n| self.road_attr(n).is_some();
        let mut v = [ROAD, RIVER_ROCKS]
            .into_iter()
            .find_map(|code| [(ul, 1), (left, 2), (ll, 3)].into_iter().find(|&(n, _)| is(n, code)))
            .map_or(0, |(_, v)| v);
        if v == 0 {
            v = if left.is_none() {
                2
            } else if ul.is_none() {
                1
            } else if ll.is_none() {
                3
            } else {
                2
            };
        }
        // a road may not turn back on itself (1 + 4, 3 + 10)
        if v != 1 && joins(ur) {
            v += 4;
        } else if joins(right) {
            v += 7;
        } else if v != 3 && joins(lr) {
            v += 10;
        } else if right.is_none() {
            v += 7;
        } else if ur.is_none() && v != 1 {
            v += 4;
        } else if lr.is_none() && v != 3 {
            v += 10;
        } else {
            v += 7;
        }
        Some(v - 6)
    }

    fn block_strip(&self, elevation: i32) -> &[Sprite] {
        &self.art.strips[(self.strip_offset + elevation as usize).min(self.art.strips.len() - 1)]
    }

    /// Cliff faces towards the lower neighbours when they are two or more
    /// levels down (`FUN_0045e26a`).
    fn cliffs(&mut self, r: usize, c: usize) {
        let zoom = self.layout.zoom;
        let (w, h, _) = zoom.block();
        let step = zoom.level_step();
        let z = self.elev((r, c));
        let [left, _, _, right] = self.layout.neighbours(r, c);
        let [ll, lr] = self.layout.lower(r, c);
        let (px, py) = self.origin(r, c);
        let index = r * self.layout.cols + c;
        let slice = (index % 3) as i32 * w;
        // ¾ of the whole block's height (not of its top face) below the origin
        let y = py + h * 3 / 4 + 2;
        let two_down = (r + 2 < self.layout.rows).then_some((r + 2, c));
        let raised = |n: Option<(usize, usize)>| n.is_some_and(|n| self.elev(n) == z + 1);
        // (lower elevation, drop, dy) for the left and right halves
        let mut halves: [Option<(i32, i32, i32)>; 2] = [None, None];
        if let Some(ll) = ll {
            let low = self.elev(ll);
            halves[0] = Some(if raised(left) { (low, z - low + 1, -step) } else { (low, z - low, 0) });
            // shifted rows have no lower-right neighbour in the last column
            if Layout::shifted(r)
                && c == self.layout.cols - 1
                && let Some(t) = two_down
            {
                let low = low.min(self.elev(t));
                halves[1] = Some((low, z - low, 0));
            }
        }
        if let Some(lr) = lr {
            let low = self.elev(lr);
            halves[1] = Some(if raised(right) { (low, z - low + 1, -step) } else { (low, z - low, 0) });
            if Layout::shifted(r)
                && c == 0
                && let Some(t) = two_down
            {
                let low = low.min(self.elev(t));
                halves[0] = Some((low, z - low, 0));
            }
        }
        // the original draws the right half first
        for (right_half, half) in [(true, halves[1]), (false, halves[0])] {
            let Some((low, drop, dy)) = half else { continue };
            if drop < 2 || !(0..4).contains(&low) {
                continue;
            }
            let cels = &self.art.cliffs[low as usize];
            let cel = &cels[(drop as usize - 2).min(cels.len() - 1)];
            let from = (slice + if right_half { w / 2 } else { 0 }) as usize;
            let dx = if right_half { w / 2 } else { 0 };
            self.canvas.blit_columns(cel, px + dx - from as i32, y + dy, from, from + (w / 2) as usize);
        }
    }

    /// Half blocks that square off the map's left and right edges, where
    /// alternate rows leave a half-block gap.
    fn edge_fill(&mut self, r: usize, c: usize) {
        let zoom = self.layout.zoom;
        let (w, _, _) = zoom.block();
        let step = zoom.level_step();
        let z = self.elev((r, c));
        let [_, ul, ur, _] = self.layout.neighbours(r, c);
        let [ll, lr] = self.layout.lower(r, c);
        let index = r * self.layout.cols + c;
        let (px, py) = self.origin(r, c);
        // Only unshifted rows get fills: the right gap is filled from the last
        // hex, and the first hex is replaced by the right half of a block
        // derived from the second (the original's `col == 1` case).
        let fill = if Layout::shifted(r) {
            None
        } else if c == 1 {
            // bits 4 = upper-right, 8 = right, as seen from the filler
            Some((ul, ll, 4, 8, false, px - w / 2))
        } else if c == self.layout.cols - 1 {
            // bits 2 = upper-left, 1 = left
            Some((ur, lr, 2, 1, true, px + w))
        } else {
            None
        };
        let Some((upper, lower, upper_bit, own_bit, left_half, x)) = fill else { return };
        let up = self.elev_or_edge(upper);
        let low = self.elev_or_edge(lower);
        let m = z.min(up).min(low);
        let mut mask = 0;
        if up - m == 1 {
            mask |= upper_bit;
        }
        if z - m == 1 {
            mask |= own_bit;
        }
        let base = if m == 0 { 0 } else { BOTH_LOWER_CELS };
        let start: usize = MASK_COUNTS[..mask].iter().sum();
        let cel_index = base + start + index % MASK_COUNTS[mask];
        let cels = self.block_strip(m);
        let cel = cels[cel_index.min(cels.len() - 1)].clone();
        let cel = &cel;
        let y = py + (z - m) * step;
        let half = (w / 2) as usize;
        if left_half {
            self.canvas.blit_columns(cel, x, y, 0, half);
        } else {
            self.canvas.blit_columns(cel, x - half as i32, y, half, cel.width);
        }
    }

    fn block(&mut self, r: usize, c: usize) {
        let zoom = self.layout.zoom;
        let (x, y) = self.layout.to_map(r, c);
        let z = self.elev((r, c));
        let mask = block_mask(self.map, &self.layout, r, c);
        let cel = block_cel(mask, z as u8, &mut self.rng);
        let (px, py) = self.origin(r, c);
        let cels = self.block_strip(z);
        let sprite = cels[cel.min(cels.len() - 1)].clone();
        self.canvas.blit(&sprite, px, py);
        if let Some(cel) = self.road_cel(r, c) {
            self.canvas.blit(&self.art.roads[cel], px, py);
        }
        for (cel, side) in stream_pieces(self.map, &self.layout, x, y) {
            let (dx, dy) = if side { zoom.stream_sides()[cel] } else { zoom.stream_joins()[cel - 6] };
            self.canvas.blit(&self.art.streams[cel], px + dx, py + dy);
        }
        if let Some(rocks) = self.river_rocks.get(&(r, c)).and_then(|&cel| self.art.river_rocks.get(cel)) {
            self.canvas.blit(rocks, px, py);
        }
    }
}

/// Draw the terrain, top row first, left to right. Per hex: cliffs, edge
/// fill, then the block with its streams, as the original does.
///
/// `strip_offset` is added to the elevation to choose the block strip (0 =
/// `River`, 1 = `Land1`, ...); the original sets it per battle.
pub fn render(map: &Map, art: &TerrainArt, zoom: Zoom, strip_offset: usize, seed: u32) -> Canvas {
    render_view(map, art, Layout::new(map, zoom), strip_offset, seed)
}

/// As [`render`], in `layout`'s view.
pub fn render_view(map: &Map, art: &TerrainArt, layout: Layout, strip_offset: usize, seed: u32) -> Canvas {
    let (w, h) = layout.canvas_size();
    let mut p = Painter {
        map,
        art,
        layout,
        strip_offset,
        canvas: Canvas::new(w, h, 0),
        rng: BorlandRand::new(seed),
        river_rocks: Default::default(),
    };
    // River rocks are picked when the hexes are built, before blocks.
    let count = art.river_rocks.len() as u32;
    for x in 0..map.width {
        for y in 0..map.height {
            let rocks = map.hex_features(x, y).iter().any(|&f| {
                f == RIVER_ROCKS || (f == STONES && map.elevation_at(x, y) == 0)
            });
            if rocks && count > 0 {
                let cel = (p.rng.next() % count) as usize;
                p.river_rocks.insert(layout.to_tile(x, y), cel);
            }
        }
    }
    if let Some(bg) = &art.background {
        // tiled from the map origin (the top-left of block (0, 0) at elevation 0)
        let (x0, y0) = layout.block_origin(0, 0);
        p.canvas.tile(bg, x0, y0);
    }
    for r in 0..layout.rows {
        for c in 0..layout.cols {
            if c == 0 && !Layout::shifted(r) {
                // hidden: covered by the edge fill drawn with column 1
                continue;
            }
            p.cliffs(r, c);
            p.edge_fill(r, c);
            p.block(r, c);
        }
    }
    p.canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reversed view puts hex `(x, y)` where the original's
    /// `FUN_0045cc35` does (tile index `i` of a map `w` wide and `h` tall:
    /// column `w - 1 - i / h`, row `i % h - (x & 1)`), and maps back.
    #[test]
    fn reversed_blocks() {
        let map = Map { name: String::new(), width: 33, height: 45, hex_id_origin: 1000, unk_b: 0, elevation: vec![1; 33 * 45], cells: Vec::new() };
        let normal = Layout::new(&map, Zoom::Normal);
        let reversed = Layout::reversed(&map, Zoom::Normal);
        for r in 0..reversed.rows {
            for c in 0..reversed.cols {
                if c == 0 && !Layout::shifted(r) {
                    continue;
                }
                let i = r * map.height + c;
                let x = map.width - 1 - i / map.height;
                let y = i % map.height - (x & 1);
                assert_eq!(reversed.to_map(r, c), (x, y));
                assert_eq!(reversed.to_tile(x, y), (r, c));
                assert_eq!(normal.to_tile(normal.to_map(r, c).0, normal.to_map(r, c).1), (r, c));
            }
        }
        // turned half round: the first column's top hex is now bottom right
        assert_eq!(reversed.to_tile(0, 0), (32, 0));
        assert_eq!(reversed.side(Side::N), Side::S);
        assert_eq!(reversed.facing(1), 5);
    }
}
