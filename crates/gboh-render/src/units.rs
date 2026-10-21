//! Units on the battlefield: each unit is drawn as a formation of figures
//! (see `docs/formats.md`, "Unit sprite sets").
//!
//! Like trees, every figure is a physical object on the unit's hex, offset
//! from the hex centre in zoom-in pixels. The offsets depend on the unit's
//! class and its facing on screen.

use crate::objects::{Drawable, anchor_at};
use crate::terrain::Layout;
use crate::{Sprite, strip};
use gboh_formats::eif::Eif;
use gboh_formats::map::Map;
use gboh_formats::scn::Unit;

/// One figure of a formation: offset and draw depth per screen facing
/// (index 0 = N … 7 = NW).
pub struct Figure {
    pub offset: [(i32, i32); 8],
    pub depth: [i32; 8],
}

/// Formations by unit class. Classes that only face the six hex sides
/// leave E and W (index 2 and 6) unused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Formation {
    /// 20 figures: phalanx, double-sized hoplites, Cardaces.
    Phalanx,
    /// 8 figures in two ranks of four: most foot.
    Infantry,
    /// 6 figures: cavalry.
    Cavalry,
    /// Three elephants.
    Elephants,
    /// Three chariots (Persian and Indian).
    Chariots,
    /// The oxybeles (bolt thrower): one figure.
    Oxybeles,
    /// One figure: leaders.
    Single,
}

impl Formation {
    /// The formation the original builds for unit type `ty` (see
    /// [`gboh_formats::units::unit_type`]).
    pub fn of_type(ty: usize) -> Self {
        match ty {
            17 | 18 | 43 => Formation::Phalanx,
            2 | 3 | 10 | 11 | 13 | 14 | 15 | 20 | 21 | 29 | 30 | 31 | 32 | 33 | 36 | 37 | 42 => Formation::Infantry,
            8 | 9 | 12 | 16 | 19 | 25 | 26 | 27 | 28 | 34 | 39 | 41 | 44 => Formation::Cavalry,
            24 | 40 => Formation::Elephants,
            22 | 23 => Formation::Chariots,
            38 => Formation::Oxybeles,
            _ => Formation::Single,
        }
    }

    pub fn figures(self) -> &'static [Figure] {
        match self {
            Formation::Phalanx => &PHALANX,
            Formation::Infantry => &INFANTRY,
            Formation::Cavalry => &CAVALRY,
            Formation::Elephants => &ELEPHANTS,
            Formation::Chariots => &CHARIOTS,
            Formation::Oxybeles => &OXYBELES,
            Formation::Single => &SINGLE,
        }
    }
}

/// Screen facing (1 N … 8 NW) for a scenario facing: the screen shows the
/// map turned a quarter, so map directions turn two steps clockwise.
pub fn screen_facing(facing: u8) -> usize {
    (facing as usize + 1) % 8 + 1
}

/// The `Standing` cels of a unit sprite set, one per screen facing, with
/// hotspots.
pub struct UnitSprites {
    standing: Vec<(Sprite, (i32, i32))>,
}

impl UnitSprites {
    pub fn load(eif: &Eif) -> Option<Self> {
        let s = eif.strips.iter().find(|s| s.name == "Standing")?;
        let sprites = strip(eif, "Standing")?;
        let hot = s.cels.iter().map(|c| (c.hot_x as i32, c.hot_y as i32));
        Some(Self { standing: sprites.into_iter().zip(hot).collect() })
    }
}

/// The screen neighbour of map hex `hex` in screen direction `dir` (1 N …
/// 8 NW; N and S have no neighbour on the pointy-topped screen grid).
fn screen_neighbour(layout: &Layout, hex: (usize, usize), dir: usize) -> Option<(usize, usize)> {
    let (r, c) = (hex.0, layout.cols - 1 - hex.1);
    let [left, ul, ur, right] = layout.neighbours(r, c);
    let [ll, lr] = layout.lower(r, c);
    let n = match dir {
        2 => ur,
        3 => right,
        4 => lr,
        6 => ll,
        7 => left,
        8 => ul,
        _ => None,
    }?;
    Some(layout.to_map(n.0, n.1))
}

/// A two-hex unit's second hex is its flank neighbour, a quarter turn
/// (two of the eight directions) clockwise from the facing on screen.
const FLANK_TURN: usize = 2;

/// What the renderer needs to know about a unit on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitView {
    /// Map hex `(x, y)`.
    pub hex: (usize, usize),
    /// Scenario facing (map directions).
    pub facing: u8,
    /// Unit type id (see `gboh_formats::units`).
    pub type_id: usize,
    /// Scenario nation byte.
    pub nation: u8,
    /// Index in the scenario (the order the original builds sprites in).
    pub id: usize,
}

impl UnitView {
    /// The view of a scenario unit, if it is on the map and has a type.
    pub fn of_scenario(map: &Map, id: usize, unit: &Unit) -> Option<Self> {
        Some(Self {
            hex: map.hex_from_id(unit.hex)?,
            facing: unit.facing,
            type_id: gboh_formats::units::unit_type(unit)?,
            nation: unit.nation,
            id,
        })
    }
}

/// Drawables for `unit` standing on its hex.
pub fn drawables<'a>(map: &Map, layout: &Layout, unit: &UnitView, art: &'a UnitSprites) -> Vec<Drawable<'a>> {
    let (hex, ty) = (unit.hex, unit.type_id);
    let facing = screen_facing(unit.facing);
    let Some((sprite, hot)) = art.standing.get(facing - 1) else { return Vec::new() };
    let formation = Formation::of_type(ty);
    let second = (facing + FLANK_TURN - 1) % 8 + 1;
    let second = screen_neighbour(layout, hex, second);
    formation
        .figures()
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            // two-hex formations put every other group of five on the second hex
            let on = if formation == Formation::Phalanx && (i / 5) % 2 == 1 { second? } else { hex };
            Some(Drawable { anchor: anchor_at(map, layout, on, f.offset[facing - 1]), depth: f.depth[facing - 1], sprite, hot: *hot })
        })
        .collect()
}

/// Activation flag art (`Activation Unit Flags`): one pennant animation
/// per nation and the pole.
pub struct FlagSprites {
    pennants: Vec<(Sprite, (i32, i32))>,
    stem: (Sprite, (i32, i32)),
}

impl FlagSprites {
    pub fn load(eif: &Eif) -> Option<Self> {
        let first = |name: &str| -> Option<(Sprite, (i32, i32))> {
            let s = eif.strips.iter().find(|s| s.name == name)?;
            let c = s.cels.first()?;
            Some((Sprite::from(c.decode().ok()?), (c.hot_x as i32, c.hot_y as i32)))
        };
        let pennants = ["Macedonian Flag", "Persian Flag", "Indian Flag", "Greek Flag"]
            .into_iter()
            .map(first)
            .collect::<Option<_>>()?;
        Some(Self { pennants, stem: first("Stem")? })
    }
}

/// Where a flag stands, in zoom-in pixels from the hex centre (before the
/// sideways jitter), and where its pennant hangs from the pole's anchor.
/// Measured in game at Zoom In.
const FLAG_BASE: (i32, i32) = (0, -12);
const PENNANT: (i32, i32) = (1, -82);

/// The pole and raised pennant (first animation frame) of `unit`'s flag.
/// `jitter` is the unit's sideways offset, `5 - rand() % 11` in the
/// original. The flag's ground shadow is not drawn yet.
pub fn flag_drawables<'a>(map: &Map, layout: &Layout, unit: &UnitView, jitter: i32, art: &'a FlagSprites) -> Vec<Drawable<'a>> {
    let hex = unit.hex;
    // pennant by nation: Macedonian, Persian (and allied), Indian, Greek
    // (and tribal)
    let pennant = match unit.nation {
        2 => 0,
        1 | 5 => 1,
        3 => 2,
        _ => 3,
    };
    let base = (FLAG_BASE.0 + jitter, FLAG_BASE.1);
    let (stem, stem_hot) = &art.stem;
    let (flag, flag_hot) = &art.pennants[pennant];
    let pole = anchor_at(map, layout, hex, base);
    let top = anchor_at(map, layout, hex, (base.0 + PENNANT.0, base.1 + PENNANT.1));
    // the pennant sorts with its pole, so it shares the pole's anchor
    let hot = (flag_hot.0 - (top.0 - pole.0), flag_hot.1 - (top.1 - pole.1));
    vec![
        Drawable { anchor: pole, depth: 0, sprite: stem, hot: *stem_hot },
        Drawable { anchor: pole, depth: 1, sprite: flag, hot },
    ]
}

const PHALANX: [Figure; 20] = [
    Figure { offset: [(-43, -2), (-43, -7), (10, -11), (52, 3), (39, 17), (18, 27), (-10, 11), (-27, 17)], depth: [10, 1, 2, 2, 11, 20, 18, 19] },
    Figure { offset: [(-20, -2), (-28, 1), (10, -11), (37, 11), (16, 17), (3, 19), (-10, 11), (-12, 9)], depth: [9, 3, 2, 4, 12, 18, 18, 17] },
    Figure { offset: [(3, -2), (-13, 9), (10, -4), (22, 19), (-7, 17), (-12, 11), (-10, 4), (3, 1)], depth: [8, 5, 4, 6, 13, 16, 16, 15] },
    Figure { offset: [(26, -2), (2, 17), (10, 3), (7, 27), (-30, 17), (-27, 3), (-10, -3), (18, -7)], depth: [7, 7, 6, 8, 14, 14, 14, 13] },
    Figure { offset: [(49, -2), (17, 25), (10, 10), (-8, 35), (-53, 17), (-42, -5), (-10, -10), (33, -15)], depth: [6, 9, 8, 10, 15, 12, 12, 11] },
    Figure { offset: [(-58, -2), (-33, -15), (10, -17), (42, -5), (54, 17), (8, 35), (-10, 17), (-17, 25)], depth: [5, 11, 10, 12, 16, 10, 10, 9] },
    Figure { offset: [(-35, -2), (-18, -7), (10, -10), (27, 3), (31, 17), (-7, 27), (-10, 10), (-2, 17)], depth: [4, 13, 12, 14, 17, 8, 8, 7] },
    Figure { offset: [(-12, -2), (-3, 1), (10, -3), (12, 11), (8, 17), (-22, 19), (-10, 3), (13, 9)], depth: [3, 15, 14, 16, 18, 6, 6, 5] },
    Figure { offset: [(11, -2), (12, 9), (10, 4), (-3, 19), (-15, 17), (-37, 11), (-10, -4), (28, 1)], depth: [2, 17, 16, 18, 19, 4, 4, 3] },
    Figure { offset: [(34, -2), (27, 17), (10, 11), (-18, 27), (-38, 17), (-52, 3), (-10, -11), (43, -7)], depth: [1, 19, 18, 20, 20, 2, 2, 1] },
    Figure { offset: [(-38, 19), (-52, 3), (-10, -15), (43, -7), (34, -4), (27, 17), (10, 8), (-18, 27)], depth: [20, 2, 1, 1, 1, 19, 17, 20] },
    Figure { offset: [(-15, 19), (-37, 11), (-10, -15), (28, 1), (11, -4), (12, 9), (10, 8), (-3, 19)], depth: [19, 4, 1, 3, 2, 17, 17, 18] },
    Figure { offset: [(8, 19), (-22, 19), (-10, -8), (13, 9), (-12, -4), (-3, 1), (10, 1), (12, 11)], depth: [18, 6, 3, 5, 3, 15, 15, 16] },
    Figure { offset: [(31, 19), (-7, 27), (-10, -1), (-2, 17), (-35, -4), (-18, -7), (10, -6), (27, 3)], depth: [17, 8, 5, 7, 4, 13, 13, 14] },
    Figure { offset: [(54, 19), (8, 35), (-10, 6), (-17, 25), (-58, -4), (-33, -15), (10, -13), (42, -5)], depth: [16, 10, 7, 9, 5, 11, 11, 12] },
    Figure { offset: [(-53, 19), (-42, -5), (-10, -21), (33, -15), (49, -4), (17, 25), (10, 14), (-8, 35)], depth: [15, 12, 9, 11, 6, 9, 9, 10] },
    Figure { offset: [(-30, 19), (-27, 3), (-10, -14), (18, -7), (27, -4), (2, 17), (10, 7), (7, 27)], depth: [14, 14, 11, 13, 7, 7, 7, 8] },
    Figure { offset: [(-7, 19), (-12, 11), (-10, -7), (3, 1), (4, -4), (-13, 9), (10, 0), (22, 19)], depth: [13, 16, 13, 15, 8, 5, 5, 6] },
    Figure { offset: [(16, 19), (3, 19), (-10, 0), (-12, 9), (-19, -4), (-28, 1), (10, -7), (37, 11)], depth: [12, 18, 15, 17, 9, 3, 3, 4] },
    Figure { offset: [(39, 19), (18, 27), (-10, 7), (-27, 17), (-42, -4), (-43, -7), (10, -14), (52, 3)], depth: [11, 20, 17, 19, 10, 1, 1, 2] },
];
const INFANTRY: [Figure; 8] = [
    Figure { offset: [(-32, -5), (-22, -6), (0, 0), (49, 2), (33, 16), (8, 32), (0, 0), (-40, 27)], depth: [4, 1, 0, 2, 5, 8, 0, 7] },
    Figure { offset: [(-9, -5), (-3, 4), (0, 0), (30, 12), (10, 16), (-11, 22), (0, 0), (-21, 17)], depth: [3, 3, 0, 4, 6, 6, 0, 5] },
    Figure { offset: [(14, -5), (16, 14), (0, 0), (11, 22), (-13, 16), (-30, 12), (0, 0), (-2, 7)], depth: [2, 5, 0, 6, 7, 4, 0, 3] },
    Figure { offset: [(37, -5), (35, 24), (0, 0), (-8, 32), (-36, 16), (-49, 2), (0, 0), (17, -3)], depth: [1, 7, 0, 8, 8, 2, 0, 1] },
    Figure { offset: [(-36, 16), (-49, 2), (0, 0), (22, -6), (37, -5), (35, 24), (0, 0), (-13, 35)], depth: [8, 2, 0, 1, 1, 7, 0, 8] },
    Figure { offset: [(-13, 16), (-30, 12), (0, 0), (3, 4), (14, -5), (16, 14), (0, 0), (6, 25)], depth: [7, 4, 0, 3, 2, 5, 0, 6] },
    Figure { offset: [(10, 16), (-11, 22), (0, 0), (-16, 14), (-9, -5), (-3, 4), (0, 0), (25, 15)], depth: [6, 6, 0, 5, 3, 3, 0, 4] },
    Figure { offset: [(33, 16), (8, 32), (0, 0), (-35, 24), (-32, -5), (-22, -6), (0, 0), (44, 5)], depth: [5, 8, 0, 7, 4, 1, 0, 2] },
];
const CAVALRY: [Figure; 6] = [
    Figure { offset: [(-30, -1), (4, -12), (0, 0), (44, 9), (33, 24), (0, 29), (0, 0), (-38, 3)], depth: [0, 0, 0, 0, 0, 0, 0, 0] },
    Figure { offset: [(0, -1), (23, -1), (0, 0), (26, 19), (4, 24), (-19, 13), (0, 0), (-15, -6)], depth: [0, 0, 0, 0, 0, 0, 0, 0] },
    Figure { offset: [(29, -1), (41, 10), (0, 0), (8, 28), (-26, 24), (-37, 7), (0, 0), (8, -14)], depth: [0, 0, 0, 0, 0, 0, 0, 0] },
    Figure { offset: [(-25, 24), (-37, 2), (0, 0), (13, -4), (30, -1), (41, 15), (0, 0), (3, 18)], depth: [0, 0, 0, 0, 0, 0, 0, 0] },
    Figure { offset: [(5, 24), (-19, 13), (0, 0), (-10, 5), (0, -1), (23, 4), (0, 0), (21, 9)], depth: [0, 0, 0, 0, 0, 0, 0, 0] },
    Figure { offset: [(34, 24), (0, 24), (0, 0), (-33, 13), (-29, -1), (4, -7), (0, 0), (39, -1)], depth: [0, 0, 0, 0, 0, 0, 0, 0] },
];
const SINGLE: [Figure; 1] = [
    Figure { offset: [(0, 11), (0, 11), (0, 11), (0, 11), (0, 11), (0, 11), (0, 11), (0, 11)], depth: [0, 0, 0, 0, 0, 0, 0, 0] },
];
const ELEPHANTS: [Figure; 3] = [
    Figure { offset: [(37, 23), (21, 20), (0, 0), (-27, 16), (-39, 10), (-23, -9), (0, 0), (20, -12)], depth: [0; 8] },
    Figure { offset: [(-39, 23), (-23, -9), (0, 0), (20, -12), (37, 10), (24, 20), (0, 0), (-27, 16)], depth: [0; 8] },
    Figure { offset: [(-1, 10), (13, -2), (0, 0), (9, 9), (-1, 23), (13, -2), (0, 0), (9, 9)], depth: [0; 8] },
];
const CHARIOTS: [Figure; 3] = [
    Figure { offset: [(27, 16), (24, 20), (0, 0), (-32, 18), (-49, 10), (-13, -9), (0, 0), (20, -12)], depth: [0; 8] },
    Figure { offset: [(-49, 16), (-23, -9), (0, 0), (15, -12), (27, 10), (34, 20), (0, 0), (-27, 16)], depth: [0; 8] },
    Figure { offset: [(-11, 3), (13, -2), (0, 0), (4, 8), (-11, 23), (-3, 11), (0, 0), (-17, -3)], depth: [0; 8] },
];
const OXYBELES: [Figure; 1] =
    [Figure { offset: [(0, 45), (0, 20), (0, 20), (0, 20), (0, 20), (0, 20), (0, 20), (0, 20)], depth: [0; 8] }];
