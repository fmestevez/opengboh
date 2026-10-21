//! Battle state built from a scenario.

use crate::hex::Hex;
use gboh_formats::map::Map;
use gboh_formats::scn::{Scenario, UnitExtra};
use gboh_formats::units::unit_type;

/// A leader's ratings, from the scenario's four leader values (in file
/// order; the names are the AI scripts' attribute constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaderRatings {
    /// First value: charisma (picks the leader in a shock's casualty roll).
    pub charisma: u16,
    /// Second value: how far, in hexes, the leader commands. Wounds reduce
    /// it.
    pub command_range: u16,
    /// Third value: shown in game as `I:`, and the starting number of
    /// command points (`Com: n/n`).
    pub initiative: u16,
    /// Fourth value: personal combat, added to the casualty roll; wounds
    /// reduce it with the command range.
    pub personal_combat: u16,
}

/// One unit (or leader) on the battlefield.
#[derive(Debug, Clone)]
pub struct Unit {
    /// Index of the unit in the scenario.
    pub source: usize,
    pub name: String,
    /// Unit type id (see `gboh_formats::units`).
    pub type_id: usize,
    /// Scenario nation byte.
    pub nation: u8,
    /// The side the unit fights for: the nation byte of the home or the
    /// visitor army (allied nations are mapped by the scenario's pairs).
    pub side: u8,
    /// Command group: a unit is commanded by the leader with the same group.
    pub group: u16,
    pub hex: Hex,
    /// Facing as in the scenario (map directions 1..8).
    pub facing: u8,
    pub leader: Option<LeaderRatings>,
    /// Map direction the unit flees in when routed (its side's retreat).
    pub retreat: u8,
    /// Movement allowance (the scenario's `unk_a`).
    pub max_mp: i32,
    /// Movement points left this turn.
    pub mp: i32,
    /// Whether the unit has changed facing this turn (skirmishers pay only
    /// for their first turn).
    pub turned: bool,
    /// Combat units: size and troop quality (the first two combat values
    /// of the scenario, shown in game as `Size:` and `TQ:`).
    pub size: u16,
    pub tq: u16,
    /// Cohesion hits taken; the unit routs when they reach its TQ.
    pub hits: i32,
    pub state: UnitState,
    /// Missile weapon and missiles left, for missile units.
    pub missile: Option<(crate::missile::Weapon, u16)>,
    /// Whether the unit has volleyed during the current leader's orders.
    pub fired: bool,
    /// Leaders: wounded (a second wound kills) and finished for the turn.
    pub wounded: bool,
    /// Given movement again by momentum after moving this turn: its next
    /// move costs a cohesion hit (once).
    pub reordered: bool,
    pub reorder_hit: bool,
    /// Done for the turn (after a shock order or a recovery): no leader
    /// may order it again.
    pub done: bool,
}

/// Where a unit stands in the battle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnitState {
    #[default]
    Fighting,
    Routed,
    /// Out of the battle: eliminated or terminally routed (its rout points
    /// count against its army).
    Gone,
}

impl Unit {
    /// Whether the unit covers two hexes: the phalanx and double-sized
    /// hoplites.
    pub fn two_hex(&self) -> bool {
        matches!(self.type_id, 17 | 18)
    }

    /// A two-hex unit's second hex: its neighbour in map direction
    /// facing + 2 (a quarter turn clockwise, its right flank).
    pub fn second_hex(&self) -> Option<Hex> {
        if !self.two_hex() {
            return None;
        }
        flank(self.hex, self.facing)
    }
}

/// The neighbour of `hex` a quarter turn clockwise from scenario facing
/// `facing` (map directions 1..8; the result is always a hex side).
pub fn flank(hex: Hex, facing: u8) -> Option<Hex> {
    neighbour(hex, (facing as usize + 1) % 8 + 1)
}

/// The neighbour across map side `side` (1 N, 2 NE, 4 SE, 5 S, 6 SW, 8 NW).
pub fn neighbour(hex: Hex, side: usize) -> Option<Hex> {
    let i = [1, 2, 4, 5, 6, 8].iter().position(|&s| s == side)?;
    Some(hex.neighbours()[i])
}

/// The state of a battle.
#[derive(Debug, Clone)]
pub struct Battle {
    pub units: Vec<Unit>,
}

impl Battle {
    /// Units of `scn` placed on `map`. Units the game would reject (unknown
    /// type, off the map) are left out.
    pub fn from_scenario(scn: &Scenario, map: &Map) -> Self {
        let units = scn
            .units
            .iter()
            .enumerate()
            .filter_map(|(source, u)| {
                let (x, y) = map.hex_from_id(u.hex)?;
                let (size, tq) = match u.extra {
                    UnitExtra::Combat { a, b, .. } | UnitExtra::Missile { a, b, .. } => (a, b),
                    UnitExtra::Leader(_) => (0, 0),
                };
                let type_id = unit_type(u)?;
                let missile = match u.extra {
                    UnitExtra::Missile { missile, .. } => {
                        let class = crate::movement::MoveClass::of_type(type_id)?;
                        crate::missile::Weapon::of(missile, class).map(|w| (w, w.supply()))
                    }
                    _ => None,
                };
                let leader = match u.extra {
                    UnitExtra::Leader([charisma, command_range, initiative, personal_combat]) => {
                        Some(LeaderRatings { charisma, command_range, initiative, personal_combat })
                    }
                    _ => None,
                };
                Some(Unit {
                    source,
                    name: u.name.clone(),
                    type_id,
                    nation: u.nation,
                    side: side_of(scn, u.nation),
                    retreat: if side_of(scn, u.nation) == scn.home.nation { scn.home.retreat } else { scn.visitor.retreat },
                    group: u.unk_b,
                    hex: Hex::new(x as i32, y as i32),
                    facing: u.facing,
                    leader,
                    max_mp: u.unk_a as i32,
                    mp: u.unk_a as i32,
                    turned: false,
                    size,
                    tq,
                    hits: 0,
                    state: UnitState::Fighting,
                    missile,
                    fired: false,
                    wounded: false,
                    reordered: false,
                    reorder_hit: false,
                    done: false,
                })
            })
            .collect();
        Self { units }
    }

    /// Indices of the leaders still in the battle.
    pub fn leaders(&self) -> impl Iterator<Item = usize> + '_ {
        self.units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.leader.is_some() && u.state != UnitState::Gone)
            .map(|(i, _)| i)
    }

    /// The hexes `unit` covers: its own, plus its flank neighbour for
    /// two-hex units.
    pub fn hexes(&self, unit: usize) -> Vec<Hex> {
        let u = &self.units[unit];
        let mut out = vec![u.hex];
        if let Some(second) = u.second_hex() {
            out.push(second);
        }
        out
    }

    /// The unit standing on `hex`, if any (leaders included).
    pub fn unit_at(&self, hex: Hex) -> impl Iterator<Item = usize> + '_ {
        self.units.iter().enumerate().filter(move |(_, u)| u.hex == hex).map(|(i, _)| i)
    }
}

/// The side a scenario nation fights for: its partner in the scenario's
/// `(nation, side)` pairs, else itself. (At Granicus `(0, 1)` puts Memnon's
/// Greek mercenaries with the Persians.)
pub fn side_of(scn: &Scenario, nation: u8) -> u8 {
    scn.pairs.iter().find(|&&(n, _)| n == nation).map_or(nation, |&(_, side)| side)
}
