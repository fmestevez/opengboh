//! Hannibal's terrain (its `HanLandscape`), which reuses Alexander's block
//! landscape with these differences (see `docs/formats.md`, "Hannibal's
//! terrain"):
//!
//! * no "Landscape Artwork": cliffs are half-block `SE/SW Cliffs` cels of
//!   the block files, attached where a lower neighbour is two or more levels
//!   down; streams are built from `Minor River Edges` and `Minor River
//!   Corners` of the overlay file, piece by piece, and nudged where a
//!   neighbour is one level up; there are no roads or river rocks;
//! * Trebbia is drawn in snow (`Snow Landscape Blocks`, two strips: `River`
//!   and `All Levels`; snow trees and streams);
//! * off-map neighbours count as elevation 9 when the map's edges are
//!   squared off;
//! * the random numbers come from the 64-bit `random()` rather than
//!   `rand()`;
//! * trees stand on a 2 × 2 grid of the hex, five or six and a centre one;
//!   stones are a single object; Baecula has towers and camp walls.
//!
//! Caesar's (`JCLandscape`) is the same with: streams drawn a piece per hex
//! corner where both its sides have one (`Minor Rivers`), with end caps
//! (`Minor River Ends`) that a neighbour's piece clears; roads (hex code
//! 12) and bridges; marsh (code 3); ditches and berms (side code 10) as
//! tile pieces, walls (9, 11) and towers as objects; no snow; the tiles
//! drawn a layer at a time; overlays placed by their cels' hotspots.

use crate::objects::{ObjectArt, Placement};
use crate::terrain::{Layout, Zoom, block_mask};
use crate::{BorlandRand, Canvas, Sprite, strip};
use gboh_formats::eif::Eif;
use gboh_formats::map::{Map, Side};

/// Cels per neighbour mask in a block strip (as Alexander's), and the two
/// leading cels of land strips used when both upper neighbours are lower.
const MASK_COUNTS: [usize; 16] = [7, 3, 1, 3, 1, 1, 2, 1, 3, 1, 1, 1, 3, 1, 1, 1];
const BOTH_LOWER_CELS: usize = 2;

/// Map codes: a minor stream (side), trees and stones (hex).
const STREAM: u16 = 6;
const TREES: u16 = 1;
const STONES: u16 = 2;

/// Elevation assumed for neighbours off the map's edges.
const OFF_MAP: i32 = 9;

/// Zoom-in hex face the scatter offsets are generated in.
const FACE: (i32, i32) = (130, 64);

/// What a battle needs of the terrain, by its map file: the block strip
/// offset, whether it's in snow, and whether it's Baecula (towers, walls).
#[derive(Debug, Clone, Copy, Default)]
pub struct Battle {
    pub strip_offset: usize,
    pub snow: bool,
    pub baecula: bool,
    /// Caesar's towers (hex ids `x * 100 + y`).
    pub towers: &'static [u16],
}

impl Battle {
    /// Hannibal's battle of map file `map_file`.
    pub fn of(map_file: &str) -> Self {
        let stem = map_file.split('.').next().unwrap_or("").to_ascii_uppercase();
        Battle {
            strip_offset: match stem.as_str() {
                "BAGRADAS" | "ZAMA" => 3,
                "ILIPA" => 2,
                _ => 0,
            },
            snow: stem == "TREBBIA",
            baecula: stem == "BAECULA",
            towers: &[],
        }
    }

    /// Caesar's battle of map file `map_file` (`FUN_004936d4`,
    /// `FUN_004937a5`): Ruspina's strip offset, the forts' towers.
    pub fn of_caesar(map_file: &str) -> Self {
        let stem = map_file.split('.').next().unwrap_or("").to_ascii_uppercase();
        Battle {
            strip_offset: if stem == "RUSPINA" { 4 } else { 0 },
            snow: false,
            baecula: false,
            towers: match stem.as_str() {
                "DYRRHACH" => &[2804, 2815, 2921, 3139, 2805, 2816, 2920, 3140, 2108, 1908, 1910, 2011, 2211, 1507, 1510, 1512, 1714, 1914],
                "PHARSAL" => &[102, 302],
                "THAPSUS" => &[2522, 2622, 3122, 2527, 2628, 3127],
                "MUNDA" => &[33, 334, 436, 337, 39],
                "BIBRAC" => &[2917, 2920, 2924, 2927],
                _ => &[],
            },
        }
    }
}

impl Zoom {
    /// The snow block file ("Snow Landscape Blocks").
    pub fn snow_blocks_file(self) -> &'static str {
        match self {
            Zoom::In => "SLAND.EIF",
            Zoom::Normal => "SLAND2.EIF",
            Zoom::Out => "SLAND4.EIF",
        }
    }

    /// The zoom's index (0 in, 1 normal, 2 out), which the original's
    /// offsets use.
    fn index(self) -> i32 {
        match self {
            Zoom::In => 0,
            Zoom::Normal => 1,
            Zoom::Out => 2,
        }
    }

    /// Where the six `Minor River Edges` pieces go from the block's
    /// top-left: N, NE, SE, S, SW, NW sides (cels 0..6).
    fn edge_offsets(self) -> [(i32, i32); 6] {
        match self {
            Zoom::In => [(119, 17), (65, 45), (0, 45), (0, 15), (3, 1), (65, 0)],
            Zoom::Normal => [(61, 8), (33, 22), (0, 22), (0, 7), (1, 0), (33, 0)],
            Zoom::Out => [(31, 4), (16, 11), (0, 11), (-1, 4), (1, 0), (18, 0)],
        }
    }

    /// Where the six `Minor River Corners` pieces go (cels 0..6).
    fn corner_offsets(self) -> [(i32, i32); 6] {
        match self {
            Zoom::In => [(-9, 28), (-9, 55), (55, 8), (0, 21), (120, 23), (65, 10)],
            Zoom::Normal => [(-5, 14), (-5, 27), (28, 4), (-1, 10), (61, 11), (32, 5)],
            Zoom::Out => [(-3, 7), (-3, 14), (14, 2), (-1, 5), (31, 6), (16, 3)],
        }
    }
}

/// Decoded terrain art for one zoom.
pub struct HanArt {
    /// Block strips of the set in use: `River`, `Land1`.. (or the snow
    /// set's `River`, `All Levels`).
    pub strips: Vec<Vec<Sprite>>,
    pub snow: bool,
    /// `SE Cliffs 1-3`, `SE Cliffs 3-5`, `SW Cliffs 1-3`, `SW Cliffs 3-5`
    /// (always from the plain block file).
    pub cliffs: [Vec<Sprite>; 4],
    /// The stream pieces in use (snow ones at Trebbia), and the plain edges,
    /// which the original swaps in for raised banks even in snow.
    pub edges: Vec<Sprite>,
    pub corners: Vec<Sprite>,
    pub plain_edges: Vec<Sprite>,
    pub background: Option<Sprite>,
    /// Caesar's overlays, if it's Caesar's terrain.
    pub caesar: Option<CaesarArt>,
}

/// A sprite with its hotspot.
pub type Cel = (Sprite, (i32, i32));

/// Caesar's terrain overlays (the zoom's overlay file), with hotspots.
pub struct CaesarArt {
    pub rivers: Vec<Cel>,
    pub river_ends: Vec<Cel>,
    pub roads: Vec<Cel>,
    pub bridges: Vec<Cel>,
    pub road_fix: Vec<Cel>,
    pub marsh: Vec<Cel>,
    pub berm: Vec<Cel>,
    pub ditch: Vec<Cel>,
}

/// Strip `name` of `eif` with each cel's hotspot.
fn cels_hot(eif: &Eif, name: &str) -> Vec<Cel> {
    let Some(s) = eif.strips.iter().find(|s| s.name == name) else { return Vec::new() };
    strip(eif, name).unwrap_or_default().into_iter().zip(s.cels.iter().map(|c| (c.hot_x as i32, c.hot_y as i32))).collect()
}

impl CaesarArt {
    pub fn load(overlay: &Eif) -> Option<Self> {
        overlay.strips.iter().any(|s| s.name == "Minor Rivers").then(|| Self {
            rivers: cels_hot(overlay, "Minor Rivers"),
            river_ends: cels_hot(overlay, "Minor River Ends"),
            roads: cels_hot(overlay, "Roads"),
            bridges: cels_hot(overlay, "Bridges"),
            road_fix: cels_hot(overlay, "Road Fix"),
            marsh: cels_hot(overlay, "Marsh"),
            berm: cels_hot(overlay, "Birm"),
            ditch: cels_hot(overlay, "Ditch"),
        })
    }
}

impl HanArt {
    /// `blocks` is the zoom's plain block file, `snow` its snow one (for a
    /// battle in snow), `overlay` its overlay file.
    pub fn load(blocks: &Eif, snow: Option<&Eif>, overlay: &Eif) -> Option<Self> {
        let set = snow.unwrap_or(blocks);
        let mut strips = Vec::new();
        let river = set.strips.iter().position(|s| s.name == "River")?;
        for s in &set.strips[river..] {
            if s.cels.is_empty() || s.name.contains("Cliffs") || s.name.contains("Fix") {
                break;
            }
            strips.push(strip(set, &s.name)?);
        }
        let cliffs = ["SE Cliffs 1-3", "SE Cliffs 3-5", "SW Cliffs 1-3", "SW Cliffs 3-5"].map(|n| strip(blocks, n).unwrap_or_default());
        let prefix = if snow.is_some() { "Snow " } else { "" };
        Some(Self {
            strips,
            snow: snow.is_some(),
            cliffs,
            edges: strip(overlay, &format!("{prefix}Minor River Edges")).unwrap_or_default(),
            corners: strip(overlay, &format!("{prefix}Minor River Corners")).unwrap_or_default(),
            plain_edges: strip(overlay, "Minor River Edges").unwrap_or_default(),
            background: None,
            caesar: CaesarArt::load(overlay),
        })
    }
}

/// Which cliff strip of [`HanArt::cliffs`]: SE or SW, by the hex's own
/// elevation (3: "1-3", else "3-5"); the drop doesn't matter.
fn cliff_strip(sw: bool, z: i32) -> usize {
    (sw as usize) * 2 + (z != 3) as usize
}

/// A stream piece: the slot it's attached in, edge or corner, cel, offset.
struct Piece {
    slot: usize,
    corner: bool,
    cel: usize,
}

/// The stream pieces of a hex, by slot (`FUN_0044de87`): each side with a
/// stream an edge; a side without one, corner pieces where its neighbours'
/// streams meet it.
fn stream_pieces(has: impl Fn(Side) -> bool) -> Vec<Piece> {
    let (s, sw, nw, n, ne, se) = (has(Side::S), has(Side::SW), has(Side::NW), has(Side::N), has(Side::NE), has(Side::SE));
    let mut out = Vec::new();
    let mut put = |on: bool, slot, corner, cel| {
        if on {
            out.push(Piece { slot, corner, cel });
        }
    };
    put(s, 2, false, 3);
    put(!s && se, 8, true, 1);
    put(!s && sw, 9, true, 0);
    put(sw, 3, false, 4);
    put(!sw && s, 10, true, 3);
    put(!sw && nw, 11, true, 2);
    put(nw, 4, false, 5);
    put(!nw && sw, 12, true, 5);
    put(!nw && n, 13, true, 4);
    put(n, 5, false, 0);
    put(ne, 6, false, 1);
    put(se, 7, false, 2);
    out.sort_by_key(|p| p.slot);
    out
}

/// A raised neighbour's effect on a stream slot (mask bits 1 left, 2
/// upper-left, 4 upper-right, 8 right one level higher): moved up a level,
/// or swapped for one of the plain `Minor River Edges` cels 6..16.
enum Nudge {
    Up,
    Cel(usize),
}

fn nudge(slot: usize, m: usize) -> Option<Nudge> {
    let b = |bit: usize| m & bit != 0;
    Some(match slot {
        2 if b(1) => Nudge::Up,
        2 if b(2) => Nudge::Cel(6),
        3 if b(2) => Nudge::Up,
        3 if b(1) && b(4) => Nudge::Cel(7),
        3 if b(1) => Nudge::Cel(8),
        3 if b(4) => Nudge::Cel(9),
        4 if b(4) => Nudge::Up,
        4 if b(8) && b(2) => Nudge::Cel(10),
        4 if b(8) => Nudge::Cel(11),
        4 if b(2) => Nudge::Cel(12),
        5 if b(8) => Nudge::Up,
        5 if b(4) => Nudge::Cel(13),
        6 if b(8) => Nudge::Cel(14),
        7 if b(1) => Nudge::Cel(15),
        8 if b(1) => Nudge::Up,
        9 | 10 if b(1) || b(2) => Nudge::Up,
        11 | 12 if b(2) || b(4) => Nudge::Up,
        13 if b(8) || b(4) => Nudge::Up,
        _ => return None,
    })
}

/// A cliff cel attached to a tile: cliff strip, cel, offset from the tile's
/// top-left.
type Cliff = (usize, usize, (i32, i32));

struct Painter<'a> {
    map: &'a Map,
    art: &'a HanArt,
    layout: Layout,
    strip_offset: usize,
    canvas: Canvas,
}

impl Painter<'_> {
    fn elev(&self, (r, c): (usize, usize)) -> i32 {
        let (x, y) = self.layout.to_map(r, c);
        self.map.elevation_at(x, y) as i32
    }

    /// The original has no tile in the first hex of unshifted rows.
    fn tile(&self, n: Option<(usize, usize)>) -> Option<(usize, usize)> {
        n.filter(|&(r, c)| c != 0 || Layout::shifted(r))
    }

    fn origin(&self, r: usize, c: usize) -> (i32, i32) {
        let (x, y) = self.layout.block_origin(r, c);
        (x, y - self.elev((r, c)) * self.layout.zoom.level_step())
    }

    /// The block strip for elevation `e` (snow: `River` at 0, `All Levels`
    /// above).
    fn strip_for(&self, e: i32) -> &[Sprite] {
        let e = if self.art.snow { e.min(1) } else { e } as usize;
        &self.art.strips[(self.strip_offset + e).min(self.art.strips.len() - 1)]
    }

    /// The cliffs each tile carries and the stream slots they hide
    /// (`FUN_0045ba47`, first pass).
    fn cliffs(&self) -> (Vec<Vec<Cliff>>, Vec<u16>) {
        let (rows, cols) = (self.layout.rows, self.layout.cols);
        let zoom = self.layout.zoom;
        let (w, h, _) = zoom.block();
        let level = zoom.level_step();
        let y = 3 - zoom.index() + 3 * h / 4;
        let mut cliffs = vec![Vec::new(); rows * cols];
        let mut hidden = vec![0u16; rows * cols];
        for r in 0..rows {
            for c in 0..cols {
                if self.tile(Some((r, c))).is_none() {
                    continue;
                }
                let i = r * cols + c;
                let z = self.elev((r, c));
                let [left, ul, ur, right] = self.layout.neighbours(r, c).map(|n| self.tile(n));
                let [ll, lr] = self.layout.lower(r, c).map(|n| self.tile(n));
                let drop = |n: Option<(usize, usize)>| n.is_some_and(|n| z - self.elev(n) >= 2);
                let one_lower = |n: Option<(usize, usize)>| n.filter(|&n| self.elev(n) == z - 1);
                let two_down = self.tile((r + 2 < rows).then_some((r + 2, c)));
                let mut hide = |slots: &[usize]| slots.iter().for_each(|&s| hidden[i] |= 1 << s);
                if drop(lr) {
                    let s = cliff_strip(false, z);
                    cliffs[i].push((s, i % 4, (w / 2, y)));
                    if let Some((rr, rc)) = one_lower(right) {
                        cliffs[rr * cols + rc].push((s + 2, i % 4, (0, y + level)));
                    }
                    hide(&[6]);
                }
                if drop(ll) {
                    let s = cliff_strip(true, z);
                    cliffs[i].push((s, i % 4, (0, y)));
                    if let Some((lr_, lc)) = one_lower(left) {
                        cliffs[lr_ * cols + lc].push((s - 2, i % 4, (w / 2, y + level)));
                    }
                    hide(&[7, 8]);
                }
                if lr.is_none() && ll.is_some() && drop(two_down) {
                    cliffs[i].push((cliff_strip(false, z), i % 4, (w / 2, y)));
                    hide(&[6]);
                }
                if ll.is_none() && lr.is_some() && drop(two_down) {
                    cliffs[i].push((cliff_strip(true, z), i % 4, (0, y)));
                    hide(&[7, 8]);
                }
                for (n, slots) in [(left, &[2, 10][..]), (ul, &[3, 9, 12][..]), (ur, &[4, 11][..]), (right, &[5, 13][..])] {
                    if drop(n) {
                        hide(slots);
                    }
                }
            }
        }
        (cliffs, hidden)
    }

    /// Half blocks squaring off the map's left and right edges (as
    /// Alexander's, with off-map neighbours at elevation 9).
    fn edge_fill(&mut self, r: usize, c: usize) {
        let zoom = self.layout.zoom;
        let (w, _, _) = zoom.block();
        let step = zoom.level_step();
        let z = self.elev((r, c));
        let [_, ul, ur, _] = self.layout.neighbours(r, c);
        let [ll, lr] = self.layout.lower(r, c);
        let index = r * self.layout.cols + c;
        let (px, py) = self.origin(r, c);
        let fill = if Layout::shifted(r) {
            None
        } else if c == 1 {
            Some((ul, ll, 4, 8, false, px - w / 2))
        } else if c == self.layout.cols - 1 {
            Some((ur, lr, 2, 1, true, px + w))
        } else {
            None
        };
        let Some((upper, lower, upper_bit, own_bit, left_half, x)) = fill else { return };
        let elev_or_edge = |n: Option<(usize, usize)>| n.map_or(OFF_MAP, |n| self.elev(n));
        let (up, low) = (elev_or_edge(upper), elev_or_edge(lower));
        let m = z.min(up).min(low);
        let mut mask = 0;
        if up - m == 1 {
            mask |= upper_bit;
        }
        if z - m == 1 {
            mask |= own_bit;
        }
        let base = if (if self.art.snow { m.min(1) } else { m }) == 0 { 0 } else { BOTH_LOWER_CELS };
        let start: usize = MASK_COUNTS[..mask].iter().sum();
        let cels = self.strip_for(m);
        let cel = cels[(base + start + index % MASK_COUNTS[mask]).min(cels.len() - 1)].clone();
        let y = py + (z - m) * step;
        let half = (w / 2) as usize;
        if left_half {
            self.canvas.blit_columns(&cel, x, y, 0, half);
        } else {
            self.canvas.blit_columns(&cel, x - half as i32, y, half, cel.width);
        }
    }
}

/// A block cel by neighbour mask, with the 64-bit generator (`FUN_0045ba47`).
fn block_cel(mask: Option<usize>, elevation: i32, rng: &mut BorlandRand) -> usize {
    let Some(mask) = mask else {
        return rng.random() as usize % BOTH_LOWER_CELS;
    };
    let base = if elevation == 0 { 0 } else { BOTH_LOWER_CELS };
    let start: usize = MASK_COUNTS[..mask].iter().sum();
    let mut pick = rng.random() as usize % MASK_COUNTS[mask];
    if mask == 0 {
        pick = if pick == 0 { rng.random() as usize % 3 } else { 3 + rng.random() as usize % 4 };
    }
    base + start + pick
}

/// Draw Hannibal's terrain in `layout`'s view: the background, then per
/// tile row by row the edge fill, the cliffs, the block and its streams.
pub fn render_view(map: &Map, art: &HanArt, layout: Layout, strip_offset: usize, seed: u32) -> Canvas {
    if let Some(ca) = &art.caesar {
        return caesar_view(map, art, ca, layout, strip_offset, seed);
    }
    let (w, h) = layout.canvas_size();
    let mut p = Painter { map, art, layout, strip_offset, canvas: Canvas::new(w, h, 0) };
    let mut rng = BorlandRand::new(seed);
    let (rows, cols) = (layout.rows, layout.cols);
    // the tiles are built first, each hex above water drawing a (hidden)
    // battle scar; then the blocks are chosen in tile order
    let tiles: Vec<(usize, usize)> = (0..rows).flat_map(|r| (0..cols).map(move |c| (r, c))).filter(|&t| p.tile(Some(t)).is_some()).collect();
    for &t in &tiles {
        if p.elev(t) >= 1 {
            rng.random();
        }
    }
    let mut cels = vec![0; rows * cols];
    for &(r, c) in &tiles {
        let z = p.elev((r, c));
        cels[r * cols + c] = block_cel(block_mask(map, &layout, r, c), if art.snow { z.min(1) } else { z }, &mut rng);
    }
    let (cliffs, hidden) = p.cliffs();
    if let Some(bg) = &art.background {
        let (x0, y0) = layout.block_origin(0, 0);
        p.canvas.tile(bg, x0, y0);
    }
    let zoom = layout.zoom;
    let level = zoom.level_step();
    for &(r, c) in &tiles {
        let i = r * cols + c;
        p.edge_fill(r, c);
        let (px, py) = p.origin(r, c);
        for &(s, cel, (dx, dy)) in &cliffs[i] {
            if let Some(sprite) = art.cliffs[s].get(cel % art.cliffs[s].len().max(1)) {
                p.canvas.blit(sprite, px + dx, py + dy);
            }
        }
        let z = p.elev((r, c));
        let cels_of = p.strip_for(z);
        let block = cels_of[cels[i].min(cels_of.len() - 1)].clone();
        p.canvas.blit(&block, px, py);
        let (x, y) = layout.to_map(r, c);
        let has = |side| map.side_features(x, y, layout.side(side)).contains(&STREAM);
        let mask = block_mask(map, &layout, r, c).unwrap_or(0);
        for piece in stream_pieces(has) {
            if hidden[i] & (1 << piece.slot) != 0 {
                continue;
            }
            let (dx, mut dy) = if piece.corner { zoom.corner_offsets()[piece.cel] } else { zoom.edge_offsets()[piece.cel] };
            let mut sprite = if piece.corner { art.corners.get(piece.cel) } else { art.edges.get(piece.cel) };
            match nudge(piece.slot, mask) {
                Some(Nudge::Up) => dy -= level,
                Some(Nudge::Cel(n)) => sprite = art.plain_edges.get(n),
                None => {}
            }
            if let Some(s) = sprite {
                p.canvas.blit(s, px + dx, py + dy);
            }
        }
    }
    p.canvas
}

/// Map side codes of Caesar's: a wall (two kinds), a ditch or berm.
const WALL: [u16; 2] = [9, 11];
const DITCH: u16 = 10;
/// Map hex codes of Caesar's: marsh, a berm hex, a road.
const MARSH: u16 = 3;
const BERM: u16 = 4;
const ROAD: u16 = 12;

/// A tile's piece on layer 2 or 3 (stream corners and caps, roads, marsh,
/// ditches) by slot: which art and cel.
#[derive(Clone, Copy)]
enum Piece2 {
    Marsh(usize),
    River(usize),
    End(usize),
    Road(usize),
    Bridge(usize),
    Fix(usize),
    Berm(usize),
    Ditch(usize),
}

/// Draw Caesar's terrain in `layout`'s view (`FUN_0045aab9`): the
/// background, every edge fill, then the tiles a layer at a time: cliffs,
/// blocks, overlays (marsh, streams, roads), ditches and berms.
fn caesar_view(map: &Map, art: &HanArt, ca: &CaesarArt, layout: Layout, strip_offset: usize, seed: u32) -> Canvas {
    let (w, h) = layout.canvas_size();
    let mut p = Painter { map, art, layout, strip_offset, canvas: Canvas::new(w, h, 0) };
    let mut rng = BorlandRand::new(seed);
    let (rows, cols) = (layout.rows, layout.cols);
    let tiles: Vec<(usize, usize)> = (0..rows).flat_map(|r| (0..cols).map(move |c| (r, c))).filter(|&t| p.tile(Some(t)).is_some()).collect();
    let n = rows * cols;
    // layer 2 slots 0..0x20 and layer 3 slots 0..8 per tile
    let mut layer2: Vec<[Option<Piece2>; 32]> = vec![[None; 32]; n];
    let mut layer3: Vec<[Option<Piece2>; 8]> = vec![[None; 8]; n];
    let side_has = |x: usize, y: usize, side: Side, codes: &[u16]| map.side_features(x, y, layout.side(side)).iter().any(|c| codes.contains(c));
    // the corners in their order: the two sides (screen: top, upper-right,
    // lower-right, bottom, lower-left, upper-left), and the sides whose lack
    // caps each end
    const CORNERS: [(Side, Side, Side, Side); 6] = [
        (Side::SW, Side::NW, Side::S, Side::N),
        (Side::NW, Side::N, Side::SW, Side::NE),
        (Side::N, Side::NE, Side::NW, Side::SE),
        (Side::NE, Side::SE, Side::N, Side::S),
        (Side::SE, Side::S, Side::NE, Side::SW),
        (Side::S, Side::SW, Side::SE, Side::NW),
    ];
    const END_CELS: [(usize, usize); 6] = [(11, 2), (1, 4), (3, 6), (5, 8), (7, 10), (9, 0)];
    // the tile builder (`FUN_0044c04b`), tile by tile
    for &(r, c) in &tiles {
        let i = r * cols + c;
        let (x, y) = layout.to_map(r, c);
        let codes = map.hex_features(x, y);
        if codes.contains(&MARSH) {
            layer2[i][1] = Some(Piece2::Marsh(rng.random() as usize % ca.marsh.len().max(1)));
        }
        for (k, &(a, b, cap_a, cap_b)) in CORNERS.iter().enumerate() {
            if side_has(x, y, a, &[STREAM]) && side_has(x, y, b, &[STREAM]) {
                layer2[i][2 + k] = Some(Piece2::River(3 * k + rng.random() as usize % 3));
                if !side_has(x, y, cap_a, &[STREAM]) {
                    layer2[i][8 + k] = Some(Piece2::End(END_CELS[k].0));
                }
                if !side_has(x, y, cap_b, &[STREAM]) {
                    layer2[i][14 + k] = Some(Piece2::End(END_CELS[k].1));
                }
            }
        }
        let berm = codes.contains(&BERM) || [2836, 2838].contains(&(x * 100 + y));
        for (k, &(a, b, _, _)) in CORNERS.iter().enumerate() {
            if side_has(x, y, a, &[DITCH]) && side_has(x, y, b, &[DITCH]) {
                layer3[i][1 + k] = Some(if berm { Piece2::Berm(k) } else { Piece2::Ditch(k) });
            }
        }
        if p.elev((r, c)) >= 1 {
            rng.random();
        }
    }
    // a corner piece clears the caps its neighbours put at its ends
    let road = |t: Option<(usize, usize)>| t.is_some_and(|(r, c)| {
        let (x, y) = layout.to_map(r, c);
        map.hex_features(x, y).contains(&ROAD)
    });
    let mut pieces = vec![[0u16; 3]; n];
    for &(r, c) in &tiles {
        let i = r * cols + c;
        let [left, ul, ur, right] = layout.neighbours(r, c).map(|t| p.tile(t));
        let [ll, lr] = layout.lower(r, c).map(|t| p.tile(t));
        const CLEAR: [[(usize, usize); 2]; 6] = [[(1, 11), (2, 17)], [(2, 12), (3, 18)], [(3, 13), (5, 19)], [(5, 8), (4, 14)], [(4, 9), (0, 15)], [(0, 10), (1, 16)]];
        let around = [left, ul, ur, right, ll, lr];
        for (k, clear) in CLEAR.iter().enumerate() {
            if layer2[i][2 + k].is_some() {
                for &(nb, slot) in clear {
                    if let Some((nr, nc)) = around[nb] {
                        layer2[nr * cols + nc][slot] = None;
                    }
                }
            }
        }
        // roads: up to three pieces from the road neighbours' weights
        if road(Some((r, c))) {
            let mut piece = [0u16; 3];
            let mut count = [0u8; 3];
            let order = [(left, 2u16), (right, 16), (ul, 1), (ur, 32), (ll, 4), (lr, 8)];
            for pass in 0..2 {
                for &(nb, weight) in &order {
                    let ok = if pass == 0 { road(nb) } else { nb.is_none() };
                    if !ok {
                        continue;
                    }
                    let (mut added, mut first) = (0, 0);
                    for q in 0..if pass == 0 { 3 } else { 1 } {
                        if count[q] >= 2 {
                            continue;
                        }
                        if first == 0 {
                            first = piece[q];
                        } else if piece[q] == first {
                            continue;
                        }
                        piece[q] += weight;
                        count[q] += 1;
                        added += 1;
                        if added > 1 {
                            break;
                        }
                    }
                }
            }
            pieces[i] = piece;
            let z = p.elev((r, c));
            if z != 0 {
                if road(ul) && ul.is_some_and(|t| p.elev(t) == z + 1) {
                    layer2[i][0x17] = Some(Piece2::Fix(0));
                }
                if road(ur) && ur.is_some_and(|t| p.elev(t) == z + 1) {
                    layer2[i][0x18] = Some(Piece2::Fix(1));
                }
            }
        }
    }
    // the blocks (and each road piece's cel) in tile order
    let mut cels = vec![0; n];
    for &(r, c) in &tiles {
        let i = r * cols + c;
        let z = p.elev((r, c));
        cels[i] = block_cel(block_mask(map, &layout, r, c), z, &mut rng);
        if road(Some((r, c))) {
            for (q, &v) in pieces[i].iter().enumerate() {
                let idx = (0..=3).find_map(|s| match v >> s {
                    5 => Some(3 * s),
                    9 => Some(3 * s + 1),
                    17 => Some(3 * s + 2),
                    _ => None,
                });
                let Some(idx) = idx.filter(|&k| k <= 9) else { continue };
                layer2[i][0x14 + q] = Some(if z == 0 { Piece2::Bridge(idx) } else { Piece2::Road(idx * 3 + rng.random() as usize % 3) });
            }
        }
    }
    let (cliffs, _) = p.cliffs();
    if let Some(bg) = &art.background {
        let (x0, y0) = layout.block_origin(0, 0);
        p.canvas.tile(bg, x0, y0);
    }
    for &(r, c) in &tiles {
        p.edge_fill(r, c);
    }
    let draw = |canvas: &mut Canvas, piece: Piece2, (px, py): (i32, i32)| {
        let (set, k) = match piece {
            Piece2::Marsh(k) => (&ca.marsh, k),
            Piece2::River(k) => (&ca.rivers, k),
            Piece2::End(k) => (&ca.river_ends, k),
            Piece2::Road(k) => (&ca.roads, k),
            Piece2::Bridge(k) => (&ca.bridges, k),
            Piece2::Fix(k) => (&ca.road_fix, k),
            Piece2::Berm(k) => (&ca.berm, k),
            Piece2::Ditch(k) => (&ca.ditch, k),
        };
        if let Some((s, (hx, hy))) = set.get(k) {
            canvas.blit(s, px - hx, py - hy);
        }
    };
    for layer in 0..4 {
        for &(r, c) in &tiles {
            let i = r * cols + c;
            let o = p.origin(r, c);
            match layer {
                0 => {
                    for &(s, cel, (dx, dy)) in &cliffs[i] {
                        if let Some(sprite) = art.cliffs[s].get(cel % art.cliffs[s].len().max(1)) {
                            p.canvas.blit(sprite, o.0 + dx, o.1 + dy);
                        }
                    }
                }
                1 => {
                    let z = p.elev((r, c));
                    let set = p.strip_for(z);
                    let block = set[cels[i].min(set.len() - 1)].clone();
                    p.canvas.blit(&block, o.0, o.1);
                }
                2 => {
                    for piece in layer2[i].iter().flatten() {
                        draw(&mut p.canvas, *piece, o);
                    }
                }
                _ => {
                    for piece in layer3[i].iter().flatten() {
                        draw(&mut p.canvas, *piece, o);
                    }
                }
            }
        }
    }
    p.canvas
}

/// Caesar's walls (`FUN_0051b915`): each hex its own south, south-west and
/// north-west sides (the opposite ones in the reversed view), offsets in
/// zoom-in pixels; a slanting piece meets the other slant if both are walls.
fn walls(map: &Map, reversed: bool) -> Vec<Placement> {
    let (w, h) = FACE;
    let mut out = Vec::new();
    let (s, sw, nw) = if reversed { (Side::N, Side::NE, Side::SE) } else { (Side::S, Side::SW, Side::NW) };
    for x in 0..map.width {
        for y in 0..map.height {
            let wall = |side| map.side_features(x, y, side).iter().any(|c| WALL.contains(c));
            let hex = (x, y);
            if wall(s) {
                out.push(Placement { hex, offset: (-w / 2, 4), art: ObjectArt::Walls, cel: 0 });
            }
            if wall(sw) {
                out.push(Placement { hex, offset: (-w / 4 - 1, -h / 2 + 12), art: ObjectArt::Walls, cel: if wall(nw) { 1 } else { 2 } });
            }
            if wall(nw) {
                out.push(Placement { hex, offset: (w / 4, -h / 2 + 12), art: ObjectArt::Walls, cel: if wall(sw) { 1 } else { 2 } });
            }
        }
    }
    out
}

/// Baecula's towers (map hex ids `x * 100 + y`) and camp wall pieces (hex,
/// zoom-in offset per cel), in the normal and reversed views.
const TOWERS: [u16; 6] = [308, 318, 709, 712, 715, 718];
const WALLS: [(u16, (i32, i32)); 7] = [(19, (6, 23)), (419, (7, -71)), (717, (60, 20)), (711, (68, 24)), (709, (124, -172)), (308, (66, -32)), (8, (-3, 23))];
const WALLS_REVERSED: [(u16, (i32, i32)); 7] =
    [(19, (0, 11)), (419, (0, 122)), (711, (-65, -12)), (717, (-63, -9)), (709, (-130, 179)), (308, (-60, 73)), (8, (0, -14))];

/// Scatter Hannibal's terrain objects (`FUN_0044de87`) with the 64-bit
/// generator: trees (`tree_cels` in the strip in use), stones (three cels),
/// and Baecula's landmarks.
pub fn scatter(map: &Map, battle: &Battle, reversed: bool, tree_cels: usize, caesar: bool, rng: &mut BorlandRand) -> Vec<Placement> {
    let (w, h) = FACE;
    let below = |rng: &mut BorlandRand, n: i32| if n <= 0 { 0 } else { (rng.random() as i32) % n };
    let trees = if battle.snow { ObjectArt::SnowTrees } else { ObjectArt::Trees };
    let mut out = Vec::new();
    for x in 0..map.width {
        for y in 0..map.height {
            let hex = (x, y);
            for &feature in map.hex_features(x, y) {
                match feature {
                    TREES => {
                        let n = rng.random() as i32 % 2 + 4;
                        for i in 0..=n {
                            let k = if i <= 3 { i } else { (rng.random() as i32 % 4 + i) % 4 };
                            let dx = below(rng, w / 4) + (k % 2) * (w / 2);
                            let dy = below(rng, h / 4) + (k / 2) * (h / 2);
                            let cel = rng.random() as usize % tree_cels.max(1);
                            out.push(Placement { hex, offset: (-w / 2 + dx, -h / 2 + dy), art: trees, cel });
                        }
                        out.push(Placement { hex, offset: (0, 0), art: trees, cel: 0 });
                    }
                    STONES => {
                        let cel = rng.random() as usize % 3;
                        out.push(Placement { hex, offset: (-w / 2 + w / 8, -h / 2 + h / 6), art: ObjectArt::Stones, cel });
                    }
                    _ => {}
                }
            }
        }
    }
    if caesar {
        out.extend(walls(map, reversed));
    }
    let tower = if reversed { ObjectArt::TowerReverse } else { ObjectArt::Tower };
    for &id in battle.towers {
        let hex = ((id / 100) as usize, (id % 100) as usize);
        if hex.0 < map.width && hex.1 < map.height {
            out.push(Placement { hex, offset: (0, 0), art: tower, cel: 0 });
        }
    }
    if battle.baecula {
        let at = |id: u16| ((id / 100) as usize, (id % 100) as usize);
        let in_map = |(x, y): (usize, usize)| x < map.width && y < map.height;
        let tower = if reversed { ObjectArt::TowerReverse } else { ObjectArt::Tower };
        out.extend(TOWERS.iter().map(|&id| at(id)).filter(|&h| in_map(h)).map(|hex| Placement { hex, offset: (0, 0), art: tower, cel: 0 }));
        let (walls, art) = if reversed { (&WALLS_REVERSED, ObjectArt::BaeculaReverse) } else { (&WALLS, ObjectArt::Baecula) };
        for (cel, &(id, offset)) in walls.iter().enumerate() {
            if in_map(at(id)) {
                out.push(Placement { hex: at(id), offset, art, cel });
            }
        }
    }
    out
}
