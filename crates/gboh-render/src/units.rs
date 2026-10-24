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
use gboh_formats::walkanim::Kind;

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

    /// Figures that never fall however worn the unit (the original's
    /// presentation classes set `+0xfe`: 10 for the 20-figure class, 2 for
    /// the 6-figure one, 0 for single figures, 3 otherwise).
    pub fn reserve(self) -> usize {
        match self {
            Formation::Phalanx => 10,
            Formation::Cavalry => 2,
            Formation::Oxybeles | Formation::Single => 0,
            _ => 3,
        }
    }

    /// Figures still standing for a unit with `hits` cohesion hits on
    /// troop quality `tq` (the original: `(figures − reserve) × p / 100`
    /// fall, `p = hits × 100 / tq` capped at 100).
    pub fn standing(self, hits: u16, tq: u16) -> usize {
        let n = self.figures().len();
        let p = if tq == 0 { 0 } else { (hits as usize * 100 / tq as usize).min(100) };
        n - (n.saturating_sub(self.reserve()) * p / 100)
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
    /// Routed pose per screen facing: the `StandingR` strip where the art
    /// has one, else the first cel of each running strip (`RoutN`… or
    /// `RN`…).
    routed: Vec<(Sprite, (i32, i32))>,
    /// Walking frames per screen facing (strips `N`…`NW`).
    walking: Vec<Vec<(Sprite, (i32, i32))>>,
    /// Running (routed) frames per screen facing (`RoutN`… or `RN`…).
    running: Vec<Vec<(Sprite, (i32, i32))>>,
    /// Fallen men: killed in shock, by missiles, trampled, and any death
    /// (strips `Shock`, `Arrow`, `Trample`, `General`).
    corpses: [Vec<(Sprite, (i32, i32))>; 4],
    /// Shooting frames per screen facing (strips `MN`…`MNW`), empty when
    /// the art has none.
    firing: Vec<Vec<(Sprite, (i32, i32))>>,
    /// Throwing down arms on breaking, per screen facing (`DropN`…).
    dropping: Vec<Vec<(Sprite, (i32, i32))>>,
    /// Fighting in shock, per screen facing: the combat strips `CN`…
    /// where the art has them, else the walking strips (the original's
    /// strip base at `+0xf6`).
    fighting: Vec<Vec<(Sprite, (i32, i32))>>,
    /// Arms thrown down on breaking, where the art has its own.
    pub debris: DebrisSprites,
}

/// Arms left on the ground by men who break (`Spear…`, `Shield`, `Shield U`
/// strips: a unit's own art, or the generic `RDEBRIS` art).
#[derive(Default)]
pub struct DebrisSprites {
    /// Per screen facing.
    spears: Vec<Vec<(Sprite, (i32, i32))>>,
    /// `Shield` and `Shield U`.
    shields: [Vec<(Sprite, (i32, i32))>; 2],
    /// `Bow`.
    bows: Vec<(Sprite, (i32, i32))>,
}

impl DebrisSprites {
    pub fn load(eif: &Eif) -> Self {
        let cels = |name: &str| -> Vec<(Sprite, (i32, i32))> {
            let Some(s) = eif.strips.iter().find(|s| s.name == name) else { return Vec::new() };
            let sprites = strip(eif, name).unwrap_or_default();
            sprites.into_iter().zip(s.cels.iter().map(|c| (c.hot_x as i32, c.hot_y as i32))).collect()
        };
        let spears = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"].iter().map(|d| cels(&format!("Spear{d}"))).collect();
        Self { spears, shields: [cels("Shield"), cels("Shield U")], bows: cels("Bow") }
    }

    /// The cels for a piece of debris (`Corpse::kind` from [`SPEAR`]).
    fn cels(&self, kind: usize) -> &[(Sprite, (i32, i32))] {
        match kind {
            k if (SPEAR + 1..=SPEAR + 8).contains(&k) => self.spears.get(k - SPEAR - 1).map_or(&[], |c| c.as_slice()),
            SHIELD => &self.shields[0],
            SHIELD_UP => &self.shields[1],
            BOW => &self.bows,
            _ => &[],
        }
    }
}

/// `Corpse::kind` for arms on the ground: a spear dropped facing screen
/// direction `f` is `SPEAR + f`; the two shield strips.
pub const SPEAR: usize = 10;
pub const SHIELD: usize = 20;
pub const SHIELD_UP: usize = 21;
pub const BOW: usize = 22;
/// `Corpse::kind` for spent missiles: `MISSILE + ` the set (0 arrows,
/// 1 javelins, 2 bolts, 3 stones), drawn from `Missile Debris`.
pub const MISSILE: usize = 30;

/// Where a breaking archer's bow lands, per screen facing (the table at
/// `0x60dafe`, `FUN_004c0da7`).
const BOW_OFFSETS: [(i32, i32); 8] = [(-30, -30), (-20, -15), (0, 0), (-13, -29), (-1, -26), (-17, -14), (0, 0), (-28, -20)];

/// Where a breaking man's spear and shield land from where he stands, per
/// screen facing (1 N … 8 NW), in zoom-in pixels (tables built at start-up
/// by the code at `0x4f6d7c`: spears at `0x60da9e` infantry, `0x60dabe`
/// mounted, `0x60dade` phalanx; shields at `0x60da7e` for unit types 11
/// and 37, `0x60da5e` for 3 and 14, `0x60da3e` for the rest).
pub fn debris_offsets(type_id: usize, facing: usize) -> ((i32, i32), (i32, i32)) {
    const SPEAR_INFANTRY: [(i32, i32); 8] = [(20, -40), (51, -11), (0, 0), (-35, -15), (-13, -32), (31, -19), (0, 0), (-42, -15)];
    const SPEAR_MOUNTED: [(i32, i32); 8] = [(16, -43), (48, -18), (0, 0), (-32, -15), (-8, -30), (24, -26), (0, 0), (-16, -36)];
    const SPEAR_PHALANX: [(i32, i32); 8] = [(-9, -63), (57, -16), (0, 0), (-43, -21), (-16, -46), (31, -28), (0, 0), (-67, -44)];
    const SHIELD_11_37: [(i32, i32); 8] = [(-17, -22), (-1, -18), (0, 0), (6, -23), (17, -17), (4, -9), (0, 0), (-13, -11)];
    const SHIELD_3_14: [(i32, i32); 8] = [(-21, -4), (-21, -8), (0, 0), (13, 9), (17, -9), (0, 3), (0, 0), (-28, 0)];
    const SHIELD_OTHER: [(i32, i32); 8] = [(-21, -4), (-19, -6), (0, 0), (5, -8), (17, -4), (2, 5), (0, 0), (-23, -5)];
    let f = facing.clamp(1, 8) - 1;
    let spear = match Formation::of_type(type_id) {
        Formation::Phalanx => SPEAR_PHALANX,
        Formation::Cavalry => SPEAR_MOUNTED,
        _ => SPEAR_INFANTRY,
    };
    let shield = match type_id {
        11 | 37 => SHIELD_11_37,
        3 | 14 => SHIELD_3_14,
        _ => SHIELD_OTHER,
    };
    (spear[f], shield[f])
}

/// Original milliseconds a shock combat's deaths are spread over
/// (`FUN_004eb617`).
pub const FIGHT_MS: u32 = 7200;

/// How far a fighting man steps toward the enemy, per screen facing (1 N
/// … 8 NW), in zoom-in pixels (the original's tables at `0x60da02` and
/// `0x60da12`).
const LUNGE: [(i32, i32); 8] = [(0, -10), (20, -12), (30, 0), (20, 12), (0, 10), (-20, 12), (-30, 0), (-20, -12)];

/// Something a unit's men do in place, with its frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Shooting a volley (frames per [`FIRE_CELS`]).
    Shoot(usize),
    /// Throwing down their arms as they break: each cel of the `Drop`
    /// strip for two frames, the last for one (the original's
    /// `TerminalRout` handler).
    Drop(usize),
    /// Walking one step in screen direction `dir` (1 N … 8 NW), `ms`
    /// milliseconds in, each man on his own way (the walking data); the
    /// unit's hex is the one being left.
    Walk { dir: usize, ms: u32 },
    /// Fighting in shock, `ms` original milliseconds in, with enemies on
    /// `sides` (bit `d` for side `d` relative to the front, 1 front,
    /// clockwise to 8); formations without exposure tables turn toward
    /// `toward` (scenario facing) instead.
    Fight { ms: u32, sides: u16, toward: u8 },
}

impl UnitSprites {
    /// Walking cel `frame` (cycling) for screen facing `facing` (1 N … 8 NW).
    pub fn walk_cel(&self, facing: usize, frame: usize) -> Option<&(Sprite, (i32, i32))> {
        let cels = self.walking.get(facing.checked_sub(1)?)?;
        cels.get(frame % cels.len().max(1))
    }

    /// Frames the arms-throwing takes for `facing` (screen, 1 N … 8 NW):
    /// 0 when the art has none for it.
    pub fn drop_frames(&self, facing: usize) -> usize {
        self.dropping.get(facing.wrapping_sub(1)).map_or(0, |c| (2 * c.len()).saturating_sub(1))
    }
}

/// The cels of a shooting strip shown on each frame of a volley (the
/// original's missile unit handler, `FUN_0052ee77`): draw, hold, release,
/// follow through.
pub const FIRE_CELS: [usize; 13] = [0, 1, 1, 1, 1, 1, 2, 3, 3, 3, 3, 3, 3];
/// The volley frame on which the missiles leave (the release cel).
pub const FIRE_RELEASE: usize = 6;

/// A fallen man on the battlefield.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Corpse {
    pub hex: (usize, usize),
    /// Whose art (unit type).
    pub type_id: usize,
    /// 0 shock, 1 missile, 2 trampled, 3 any (a destroyed unit); or arms
    /// on the ground ([`SPEAR`], [`SHIELD`], [`SHIELD_UP`]).
    pub kind: usize,
    /// Which cel of the strip.
    pub cel: usize,
    /// Zoom-in pixels from the hex centre.
    pub offset: (i32, i32),
}

impl UnitSprites {
    pub fn load(eif: &Eif) -> Option<Self> {
        let cels = |name: &str| -> Option<Vec<(Sprite, (i32, i32))>> {
            let s = eif.strips.iter().find(|s| s.name == name)?;
            let sprites = strip(eif, name)?;
            let hot = s.cels.iter().map(|c| (c.hot_x as i32, c.hot_y as i32));
            Some(sprites.into_iter().zip(hot).collect())
        };
        let standing = cels("Standing")?;
        let running = |prefix: &str| -> Option<Vec<(Sprite, (i32, i32))>> {
            ["N", "NE", "E", "SE", "S", "SW", "W", "NW"]
                .iter()
                .map(|d| cels(&format!("{prefix}{d}"))?.into_iter().next())
                .collect()
        };
        let routed = cels("StandingR")
            .filter(|r| r.len() >= 8)
            .or_else(|| running("Rout"))
            .or_else(|| running("R"))
            .unwrap_or_else(|| standing.clone());
        const DIRS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
        let walking = DIRS.iter().map(|d| cels(d).unwrap_or_default()).collect();
        let running = DIRS
            .iter()
            .map(|d| cels(&format!("Rout{d}")).or_else(|| cels(&format!("R{d}"))).unwrap_or_default())
            .collect();
        let corpses = ["Shock", "Arrow", "Trample", "General"].map(|n| cels(n).unwrap_or_default());
        let firing = DIRS.iter().map(|d| cels(&format!("M{d}")).unwrap_or_default()).collect();
        let dropping = DIRS.iter().map(|d| cels(&format!("Drop{d}")).unwrap_or_default()).collect();
        let fighting = DIRS.iter().map(|d| cels(&format!("C{d}")).or_else(|| cels(d)).unwrap_or_default()).collect();
        let debris = DebrisSprites::load(eif);
        Some(Self { standing, routed, walking, running, corpses, firing, dropping, fighting, debris })
    }
}

/// The screen neighbour of map hex `hex` in screen direction `dir` (1 N …
/// 8 NW; N and S have no neighbour on the pointy-topped screen grid).
fn screen_neighbour(layout: &Layout, hex: (usize, usize), dir: usize) -> Option<(usize, usize)> {
    let (r, c) = layout.to_tile(hex.0, hex.1);
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
    /// Drawn in its routed pose.
    pub routed: bool,
    /// While walking: the frame of the walking animation, and how far it is
    /// along the way to the next hex, in canvas pixels.
    pub walk: Option<(usize, (i32, i32))>,
    /// Cohesion hits and troop quality: worn units show fewer figures.
    pub hits: u16,
    pub tq: u16,
    /// What its men are doing in place, if anything.
    pub act: Option<Act>,
    /// Which of its men are down (bit per figure); `None`: as many as its
    /// hits take, in [`standing_order`].
    pub fallen: Option<u32>,
}

/// The drawable for a fallen man. Like the original (`FUN_004e5a75`), the
/// body is one of the cels of the strip for how he died and the `General`
/// strip together (a destroyed unit's men: `General` only).
pub fn corpse_drawable<'a>(map: &Map, layout: &Layout, c: &Corpse, art: &'a UnitSprites) -> Option<Drawable<'a>> {
    corpse_drawable_with(map, layout, c, art, None)
}

/// As [`corpse_drawable`], with `generic` the generic rout debris for arms
/// the unit's own art has none of.
pub fn corpse_drawable_with<'a>(map: &Map, layout: &Layout, c: &Corpse, art: &'a UnitSprites, generic: Option<&'a DebrisSprites>) -> Option<Drawable<'a>> {
    if c.kind >= SPEAR {
        let own = art.debris.cels(c.kind);
        let cels = if own.is_empty() { generic?.cels(c.kind) } else { own };
        let (sprite, hot) = cels.get(c.cel % cels.len().max(1))?;
        return Some(Drawable { anchor: anchor_at(map, layout, c.hex, c.offset), depth: -1, sprite, hot: *hot });
    }
    let general = &art.corpses[3];
    let own: &[_] = if c.kind < 3 { &art.corpses[c.kind] } else { &[] };
    let n = own.len() + general.len();
    let i = c.cel % n.max(1);
    let (sprite, hot) = own.get(i).or_else(|| general.get(i - own.len().min(i)))?;
    Some(Drawable { anchor: anchor_at(map, layout, c.hex, c.offset), depth: 0, sprite, hot: *hot })
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
            routed: false,
            walk: None,
            hits: 0,
            tq: 0,
            act: None,
            fallen: None,
        })
    }
}

/// Drawables for `unit` standing on its hex.
pub fn drawables<'a>(map: &Map, layout: &Layout, unit: &UnitView, art: &'a UnitSprites) -> Vec<Drawable<'a>> {
    drawables_with(map, layout, unit, art, None)
}

/// As [`drawables`], with `walk` the walking data for the unit's kind, so
/// that a walking unit's men each go their own way.
pub fn drawables_with<'a>(map: &Map, layout: &Layout, unit: &UnitView, art: &'a UnitSprites, walk: Option<&Kind>) -> Vec<Drawable<'a>> {
    if let Some(Act::Fight { ms, sides, toward }) = unit.act {
        return fight_drawables(map, layout, unit, art, ms, sides, toward);
    }
    if let (Some(Act::Walk { dir, ms }), Some(kind)) = (unit.act, walk)
        && let Some(d) = walk_drawables(map, layout, unit, art, kind, dir, ms)
    {
        return d;
    }
    let facing = screen_facing(layout.facing(unit.facing));
    let walking = unit.walk.and_then(|(frame, _)| {
        let set = if unit.routed { &art.running } else { &art.walking };
        let cels = set.get(facing - 1).filter(|c| !c.is_empty()).or_else(|| art.walking.get(facing - 1))?;
        cels.get(frame % cels.len().max(1))
    });
    let acting = unit.act.and_then(|a| match a {
        Act::Shoot(f) if !unit.routed => {
            let cels = art.firing.get(facing - 1)?;
            cels.get(FIRE_CELS[f.min(FIRE_CELS.len() - 1)].min(cels.len().checked_sub(1)?))
        }
        Act::Drop(f) => {
            let cels = art.dropping.get(facing - 1)?;
            cels.get((f / 2).min(cels.len().checked_sub(1)?))
        }
        _ => None,
    });
    let poses = if unit.routed { &art.routed } else { &art.standing };
    let Some((sprite, hot)) = walking.or(acting).or(poses.get(facing - 1)) else { return Vec::new() };
    figure_spots(map, layout, unit)
        .into_iter()
        .map(|(anchor, depth)| Drawable { anchor, depth, sprite, hot: *hot })
        .collect()
}

/// Walking men (`FUN_004ee88a`): each goes from his place to the first
/// waypoint of his plain way for the step (relative to the hex he leaves),
/// then to the second (his place in the next hex, relative to it), in the
/// walking data's times, facing the way of the step. `None` when the data
/// has no way for this unit, facing or step.
#[allow(clippy::too_many_arguments)]
fn walk_drawables<'a>(map: &Map, layout: &Layout, unit: &UnitView, art: &'a UnitSprites, kind: &Kind, dir: usize, ms: u32) -> Option<Vec<Drawable<'a>>> {
    let facing = screen_facing(layout.facing(unit.facing));
    let set = if unit.routed { &art.running } else { &art.walking };
    let cels = set.get(dir.checked_sub(1)?).filter(|c| !c.is_empty()).or_else(|| art.walking.get(dir - 1))?;
    let frame = unit.walk.map_or(0, |w| w.0);
    let (sprite, hot) = cels.get(frame % cels.len().max(1))?;
    let mut out = Vec::new();
    for (i, hex, _, depth) in figure_places(layout, unit) {
        let man = kind.men.get(i)?;
        let way = man.facings.get(facing - 1)?;
        let to = screen_neighbour(layout, hex, dir)?;
        let level = |(x, y): (usize, usize)| map.elevation_at(x, y);
        let variant = Kind::variant(level(hex), level(to));
        let leg = way.steps.get(dir - 1)?[variant];
        let p = |(x, y): (i16, i16)| (x as i32, y as i32);
        // the waypoints and the hex each is measured from: the first from
        // the hex left, the last from the next; going down the middle one
        // is by the first, going up by the last (🟡 inferred from the data)
        let mut points = vec![anchor_at(map, layout, hex, p(way.point))];
        let mut times = Vec::new();
        let n = if variant == 0 { 2 } else { 3 };
        for k in 0..n {
            let from_next = k == n - 1 || (k == 1 && variant == 2);
            points.push(anchor_at(map, layout, if from_next { to } else { hex }, p(leg.points[k])));
            times.push(leg.ms[k].max(1) as f32);
        }
        let lerp = |u: (i32, i32), v: (i32, i32), f: f32| {
            let f = f.clamp(0.0, 1.0);
            (u.0 + ((v.0 - u.0) as f32 * f) as i32, u.1 + ((v.1 - u.1) as f32 * f) as i32)
        };
        let mut t = ms as f32;
        let mut anchor = *points.last().unwrap();
        for k in 0..n {
            if t < times[k] {
                anchor = lerp(points[k], points[k + 1], t / times[k]);
                break;
            }
            t -= times[k];
        }
        out.push(Drawable { anchor, depth, sprite, hot: *hot });
    }
    Some(out)
}

/// Original milliseconds per cel of the fighting strips (🟡 the sprite
/// engine's frame time is not traced).
const FIGHT_CEL_MS: u32 = 100;

/// Fighting men (`FUN_004eb617`): each man exposed on a side where an
/// enemy stands (the side nearest him, by the formation's table) turns
/// that way and, after a short random pause, steps toward the enemy and then
/// swings between his place and twice the step, 1800 ms each way, playing
/// the strip's forward cels going in and its backward cels coming out; the
/// others stand. The cels: 0–4 and back for cavalry, 0–3 and back for the
/// phalanx, otherwise 1, 2, 3 in and 3, 3, 2, 2, 1, 1 out.
#[allow(clippy::too_many_arguments)]
fn fight_drawables<'a>(map: &Map, layout: &Layout, unit: &UnitView, art: &'a UnitSprites, ms: u32, sides: u16, toward: u8) -> Vec<Drawable<'a>> {
    let formation = Formation::of_type(unit.type_id);
    let table = exposure(formation);
    let mut view = *unit;
    if table.is_empty() {
        view.facing = toward;
    }
    let own = screen_facing(layout.facing(view.facing));
    let (forward, back): (&[usize], &[usize]) = match formation {
        Formation::Cavalry => (&[0, 1, 2, 3, 4], &[4, 3, 2, 1, 0]),
        Formation::Phalanx => (&[0, 1, 2, 3], &[3, 2, 1, 0]),
        _ => (&[1, 2, 3], &[3, 3, 2, 2, 1, 1]),
    };
    // the way each man fights: toward his exposed side, if any
    let fights = |i: usize| -> Option<usize> {
        let Some(row) = table.get(i) else { return Some(own) };
        let mut side = 0;
        for d in 1..9 {
            if sides & (1 << d) != 0 && row[d] < row[side] {
                side = d;
            }
        }
        (side > 0).then(|| (own - 1 + side - 1) % 8 + 1)
    };
    let standing = art.standing.get(own - 1);
    figure_places(layout, &view)
        .into_iter()
        .filter_map(|(i, hex, (ox, oy), depth)| {
            // the man's own pause and scatter
            let mut seed = (unit.id as u32).wrapping_mul(2654435761) ^ (i as u32).wrapping_mul(40503) ^ 0x5bd1;
            let mut r = |n: u32| {
                seed = seed.wrapping_mul(22695477).wrapping_add(1);
                ((seed >> 16) & 0x7fff) % n
            };
            let start = (2 + r(5)) * FIGHT_CEL_MS;
            let step_in = 640 - r(81);
            let (jx, jy) = (10 - r(21) as i32, 5 - r(11) as i32);
            let facing = fights(i);
            let cels = facing.and_then(|f| art.fighting.get(f - 1)).filter(|c| !c.is_empty());
            let (lx, ly) = facing.map_or((0, 0), |f| LUNGE[f - 1]);
            let (sprite, hot, (dx, dy)) = match (ms.checked_sub(start), cels) {
                (Some(t), Some(cels)) => {
                    let (near, step, far) = ((jx, jy), (lx + jx, ly + jy), (2 * lx + jx, 2 * ly + jy));
                    let (from, to, leg_t, leg, out) = if t < step_in {
                        (near, step, t, step_in, false)
                    } else {
                        let k = (t - step_in) / 1800;
                        let leg_t = (t - step_in) % 1800;
                        match (k, k % 2) {
                            (0, _) => (step, far, leg_t, 1800, false),
                            (_, 1) => (far, near, leg_t, 1800, true),
                            _ => (near, far, leg_t, 1800, false),
                        }
                    };
                    let f = leg_t as f32 / leg as f32;
                    let at = (from.0 + ((to.0 - from.0) as f32 * f) as i32, from.1 + ((to.1 - from.1) as f32 * f) as i32);
                    let seq = if out { back } else { forward };
                    let c = seq[((leg_t / FIGHT_CEL_MS) as usize).min(seq.len() - 1)];
                    let (sp, h) = cels.get(c.min(cels.len() - 1))?;
                    (sp, h, at)
                }
                _ => {
                    let (sp, h) = standing?;
                    (sp, h, (0, 0))
                }
            };
            let anchor = anchor_at(map, layout, hex, (ox + dx, oy + dy));
            Some(Drawable { anchor, depth, sprite, hot: *hot })
        })
        .collect()
}

/// Where `unit`'s standing figures are drawn (canvas anchor and depth),
/// walking shift included.
pub fn figure_spots(map: &Map, layout: &Layout, unit: &UnitView) -> Vec<((i32, i32), i32)> {
    let shift = unit.walk.map_or((0, 0), |(_, s)| s);
    figure_places(layout, unit)
        .into_iter()
        .map(|(_, hex, offset, depth)| {
            let (x, y) = anchor_at(map, layout, hex, offset);
            ((x + shift.0, y + shift.1), depth)
        })
        .collect()
}

/// `unit`'s figures in the order they stand: the last falls first. The
/// original picks the fallen at random (favouring the exposed side, not
/// reproduced); a fixed random order per unit keeps the same men standing
/// from frame to frame.
pub fn standing_order(unit: &UnitView) -> Vec<usize> {
    let n = Formation::of_type(unit.type_id).figures().len();
    let mut order: Vec<usize> = (0..n).collect();
    let mut seed = (unit.id as u32).wrapping_mul(2654435761).wrapping_add(17);
    order.sort_by_key(|&i| {
        seed = seed.wrapping_mul(22695477).wrapping_add(1 + i as u32);
        seed >> 8
    });
    order
}

/// The men down for `unit`'s hits, taken in [`standing_order`] (for units
/// whose falls were not followed, such as a saved game's).
pub fn fallen_by_hits(unit: &UnitView) -> u32 {
    let formation = Formation::of_type(unit.type_id);
    let standing = formation.standing(unit.hits, unit.tq);
    standing_order(unit)[standing..].iter().fold(0, |m, &i| m | 1 << i)
}

/// For each figure of a formation: a threshold, then for each side
/// relative to the unit's front (1 front, clockwise to 8) how near the
/// figure stands to it; below the threshold it is exposed on that side (the
/// original's tables at `0x60d768`, `0x60d8d0`, `0x60d960`, `0x60d9cc`).
fn exposure(formation: Formation) -> &'static [[i16; 9]] {
    const PHALANX_X: [[i16; 9]; 20] = [
        [9, 3, 4, 9, 9, 9, 9, 1, 2], [9, 1, 3, 9, 9, 9, 9, 9, 2], [9, 1, 3, 9, 9, 9, 9, 9, 2], [9, 1, 2, 9, 9, 9, 9, 9, 3],
        [9, 2, 1, 9, 9, 9, 9, 9, 3], [9, 2, 3, 9, 9, 9, 9, 9, 1], [9, 1, 3, 9, 9, 9, 9, 9, 2], [9, 1, 3, 9, 9, 9, 9, 9, 2],
        [9, 1, 2, 9, 9, 9, 9, 9, 3], [9, 3, 2, 1, 9, 9, 9, 9, 4], [9, 5, 6, 9, 9, 3, 2, 1, 4], [9, 4, 5, 9, 9, 1, 2, 9, 3],
        [9, 4, 5, 9, 3, 1, 2, 9, 6], [9, 4, 3, 9, 2, 1, 9, 9, 5], [9, 4, 3, 9, 2, 1, 9, 9, 5], [9, 4, 5, 9, 9, 2, 1, 9, 3],
        [9, 4, 5, 9, 9, 1, 2, 9, 3], [9, 4, 6, 9, 3, 1, 2, 9, 5], [9, 3, 4, 9, 2, 1, 9, 9, 6], [9, 5, 4, 1, 3, 2, 9, 9, 6],
    ];
    const INFANTRY_X: [[i16; 9]; 8] = [
        [9, 3, 4, 9, 9, 9, 9, 1, 2], [9, 1, 3, 9, 9, 9, 9, 9, 2], [9, 1, 2, 9, 9, 9, 9, 9, 3], [9, 3, 2, 1, 9, 9, 9, 9, 4],
        [9, 4, 5, 9, 9, 9, 2, 1, 3], [9, 4, 5, 9, 9, 1, 2, 9, 3], [9, 4, 3, 9, 2, 1, 9, 9, 5], [9, 4, 3, 1, 2, 9, 9, 9, 5],
    ];
    const MOUNTED_X: [[i16; 9]; 6] = [
        [9, 3, 4, 5, 9, 9, 9, 2, 1], [9, 1, 2, 4, 9, 9, 9, 5, 3], [9, 3, 1, 2, 9, 9, 9, 5, 4],
        [9, 6, 8, 7, 5, 3, 1, 2, 4], [9, 8, 6, 4, 2, 1, 3, 5, 7], [9, 6, 4, 2, 1, 3, 5, 7, 8],
    ];
    const BEASTS_X: [[i16; 9]; 3] = [[9, 1, 6, 9, 9, 5, 4, 2, 3], [9, 1, 3, 2, 4, 5, 9, 9, 6], [9, 1, 2, 4, 9, 9, 9, 5, 3]];
    match formation {
        Formation::Phalanx => &PHALANX_X,
        Formation::Infantry => &INFANTRY_X,
        Formation::Cavalry => &MOUNTED_X,
        Formation::Elephants | Formation::Chariots => &BEASTS_X,
        Formation::Oxybeles | Formation::Single => &[],
    }
}

/// The side of a unit facing `facing` (scenario facings) that `toward`
/// lies on: 1 front, clockwise to 8.
pub fn relative_side(facing: u8, toward: u8) -> usize {
    (screen_facing(toward) as i32 - screen_facing(facing) as i32).rem_euclid(8) as usize + 1
}

/// The random numbers the original picks the fallen with: its own
/// generator, seeded with the figure count each time (`FUN_0057a3be`),
/// `state × 16807 mod 0x47483647` (in 32 bits), scaled to `0..100`
/// (`FUN_0057a362`).
struct PickRng(u32);

impl PickRng {
    fn roll(&mut self) -> u32 {
        const M: u32 = 0x4748_3647;
        self.0 = self.0.wrapping_mul(16807) % M;
        (100.0 * self.0 as f64 / M as f64) as u32
    }
}

/// Which `count` more of `unit`'s men fall, those in `down` being down
/// already, in the order they fall (the original's presentation handler,
/// `FUN_004e77de`). Visiting the living men in turn, each is taken with a
/// 1-in-figures chance; in shock (`enemies`: the sides, relative to the
/// front, where enemies stand) only men exposed on such a side, until a
/// whole round has gone by, after which anyone.
pub fn pick_fallen(type_id: usize, down: u32, count: usize, enemies: Option<[bool; 9]>) -> Vec<usize> {
    let formation = Formation::of_type(type_id);
    let n = formation.figures().len();
    let table = exposure(formation);
    let exposed: Vec<bool> = (0..n)
        .map(|i| {
            let (Some(row), Some(sides)) = (table.get(i), enemies) else { return false };
            (1..9).any(|d| sides[d] && row[d] < row[0])
        })
        .collect();
    let alive = |i: usize, picked: &[usize]| down & (1 << i) == 0 && !picked.contains(&i);
    let mut rng = PickRng(n as u32);
    let chance = 100 / n as u32;
    let mut picked = Vec::new();
    for _ in 0..count {
        if !(0..n).any(|i| alive(i, &picked)) {
            break;
        }
        let (mut i, mut wrapped) = (0, false);
        loop {
            if alive(i, &picked) {
                if enemies.is_some() {
                    if rng.roll() < chance && exposed[i] {
                        break;
                    }
                    if rng.roll() < chance && wrapped {
                        break;
                    }
                } else if rng.roll() < chance {
                    break;
                }
            }
            i += 1;
            if i == n {
                i = 0;
                wrapped = true;
            }
        }
        picked.push(i);
    }
    picked
}

/// The arms `unit`'s standing men throw down as it breaks: per man a spear
/// and, on foot, a shield (`FUN_004e6128`, `FUN_004e6413`), or for archers
/// (`bow`) a bow instead of the spear (`FUN_004c0da7`), each a random cel
/// (`cel` gives the random numbers), where he stands plus the per-facing
/// offset. Elephants, chariots, leaders and bolt throwers drop none. (🟡
/// The archer classes' second piece, slot `0x60`, is not traced: they drop
/// no shield here.)
pub fn rout_debris(layout: &Layout, unit: &UnitView, bow: bool, cel: &mut impl FnMut() -> usize) -> Vec<Corpse> {
    let formation = Formation::of_type(unit.type_id);
    if !matches!(formation, Formation::Phalanx | Formation::Infantry | Formation::Cavalry) {
        return Vec::new();
    }
    let facing = screen_facing(layout.facing(unit.facing));
    let (spear, shield) = debris_offsets(unit.type_id, facing);
    // `Shield U` facing N, NE or SE, `Shield` facing S, SW or NW
    let shield_kind = if matches!(facing, 1 | 2 | 4) { SHIELD_UP } else { SHIELD };
    let mut out = Vec::new();
    for (_, hex, (ox, oy), _) in figure_places(layout, unit) {
        let at = |(dx, dy): (i32, i32)| (ox + dx, oy + dy);
        if bow {
            out.push(Corpse { hex, type_id: unit.type_id, kind: BOW, cel: cel(), offset: at(BOW_OFFSETS[facing - 1]) });
            continue;
        }
        out.push(Corpse { hex, type_id: unit.type_id, kind: SPEAR + facing, cel: cel(), offset: at(spear) });
        if formation != Formation::Cavalry {
            out.push(Corpse { hex, type_id: unit.type_id, kind: shield_kind, cel: cel(), offset: at(shield) });
        }
    }
    out
}

/// The walking data's kind for unit type `type_id`.
pub fn walk_kind(type_id: usize) -> &'static str {
    match Formation::of_type(type_id) {
        Formation::Phalanx => "Phalanx",
        Formation::Cavalry => "Mounted",
        Formation::Elephants => "Elephant",
        Formation::Chariots => "Chariot",
        Formation::Oxybeles => "Oxybeles",
        Formation::Single => "Leader",
        Formation::Infantry => "Foot",
    }
}

/// A standing figure: index in the formation, map hex, offset in zoom-in
/// pixels, depth.
pub type FigurePlace = (usize, (usize, usize), (i32, i32), i32);

/// `unit`'s standing figures: index in the formation, map hex, offset from
/// the hex centre in zoom-in pixels, and depth. (Zoom does not matter to
/// `layout` here.)
pub fn figure_places(layout: &Layout, unit: &UnitView) -> Vec<FigurePlace> {
    let (hex, ty) = (unit.hex, unit.type_id);
    let facing = screen_facing(layout.facing(unit.facing));
    let formation = Formation::of_type(ty);
    let second = (facing + FLANK_TURN - 1) % 8 + 1;
    let second = screen_neighbour(layout, hex, second);
    let figures = formation.figures();
    let fallen = unit.fallen.unwrap_or_else(|| fallen_by_hits(unit));
    figures
        .iter()
        .enumerate()
        .filter(|(i, _)| fallen & (1 << i) == 0)
        .filter_map(|(i, f)| {
            // two-hex formations put every other group of five on the second hex
            let on = if formation == Formation::Phalanx && (i / 5) % 2 == 1 { second? } else { hex };
            Some((i, on, f.offset[facing - 1], f.depth[facing - 1]))
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
    // a walking unit's flag goes with it
    let shift = unit.walk.map_or((0, 0), |(_, s)| s);
    let moved = |(x, y): (i32, i32)| (x + shift.0, y + shift.1);
    let pole = moved(anchor_at(map, layout, hex, base));
    let top = moved(anchor_at(map, layout, hex, (base.0 + PENNANT.0, base.1 + PENNANT.1)));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worn_units_stand_thinner() {
        // a phalanx at 6 hits of TQ 7 keeps 12 of 20 (seen in the original)
        assert_eq!(Formation::Phalanx.standing(6, 7), 12);
        assert_eq!(Formation::Phalanx.standing(0, 7), 20);
        assert_eq!(Formation::Cavalry.standing(8, 8), 2);
        assert_eq!(Formation::Elephants.standing(5, 6), 3);
        assert_eq!(Formation::Infantry.standing(4, 8), 6);
    }

    #[test]
    fn the_exposed_fall_first_in_shock() {
        // infantry (type 2) attacked from the front: the front rank (0–3)
        // is preferred (after a round without a pick anyone may fall)
        let mut front = [false; 9];
        front[1] = true;
        let picked = pick_fallen(2, 0, 6, Some(front));
        assert_eq!(picked.len(), 6);
        assert!(picked[0] < 4, "{picked:?}");
        assert!(picked.iter().filter(|&&i| i < 4).count() >= 3, "{picked:?}");
        let mut rear = [false; 9];
        rear[5] = true;
        let picked = pick_fallen(2, 0, 6, Some(rear));
        assert!(picked.iter().filter(|&&i| i >= 4).count() >= 3, "{picked:?}");
        // never one already down, never more than stand
        assert!(!pick_fallen(2, 0b1111, 3, Some(front)).iter().any(|&i| i < 4));
        assert_eq!(pick_fallen(2, 0b0111_1111, 5, None), vec![7]);
        assert_eq!(relative_side(2, 2), 1);
        assert_eq!(relative_side(2, 6), 5);
    }
}
