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
    /// The turn it arrives on the field (0: there from the start).
    pub arrival: u16,
    /// A leader's elite initiative, and his superior (a unit index; the
    /// leader whose group his scenario record links him to): Hannibal's.
    pub elite: bool,
    pub superior: Option<usize>,
    /// The group a leader's scenario record links him to (−1 none).
    pub link: i16,
    /// Hannibal's legionaries: which line of the legion (1 hastati, 2
    /// principes, 3 cohorts), which manipular line extension pairs by.
    pub legion: Option<i16>,
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
    /// Caesar's: rallied once already (`+0xd2`): a point worse on its TQ
    /// checks and later rallies, a point better at shooting.
    pub rallied: bool,
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
    /// Left the map by a scenario's exit (Pelium); not a loss.
    Exited,
    /// Not yet on the field: it arrives at its hex on turn `arrival`
    /// (Hannibal's and Caesar's scenarios); not counted in the rout totals.
    Waiting,
    /// Off the map after riding a routed unit off its edge: a leaderless
    /// unit that comes back from that edge on a free units' turn.
    Away,
}

impl Unit {
    /// Whether the unit covers two hexes: the phalanx and double-sized
    /// hoplites.
    /// Still on the battlefield (fighting or routed).
    pub fn on_field(&self) -> bool {
        matches!(self.state, UnitState::Fighting | UnitState::Routed)
    }

    pub fn two_hex(&self) -> bool {
        gboh_formats::units::two_hex(self.type_id)
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
#[derive(Debug, Clone, Default)]
pub struct Battle {
    pub units: Vec<Unit>,
    /// Caesar's Vercellae heat (an option): points added to the Gauls'
    /// (nation 11, but horse) TQ checks.
    pub heat: u32,
    /// The rule options in force (see [`crate::options`]).
    pub options: crate::options::Options,
    /// The layout of the scenario it comes from (the *Collector's
    /// Edition*'s, whose units mix the games', plays by Caesar's engine).
    pub layout: gboh_formats::scn::Layout,
    /// Hannibal's Cannae: the wind blowing this turn (from direction 2): a
    /// unit facing into it (6 or 7) moves a hex less and shoots a hex
    /// shorter (`FUN_0057c790`, `FUN_00578260`; leaders not).
    pub wind: bool,
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
                        // (a legion's pilum: one)
                        let supply = |w: crate::missile::Weapon| if class == crate::movement::MoveClass::Legion { 1 } else { w.supply() };
                        crate::missile::Weapon::of(missile, class).map(|w| (w, supply(w)))
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
                    state: if u.arrival > 0 { UnitState::Waiting } else { UnitState::Fighting },
                    arrival: u.arrival,
                    elite: u.elite != 0,
                    legion: match u.extra {
                        UnitExtra::Missile { legion, .. } => legion,
                        _ => None,
                    },
                    superior: None,
                    link: u.link,
                    missile,
                    fired: false,
                    wounded: false,
                    reordered: false,
                    reorder_hit: false,
                    rallied: false,
                    done: false,
                })
            })
            .collect::<Vec<Unit>>();
        // a leader's link names his superior's group
        let mut units = units;
        for i in 0..units.len() {
            let (link, side) = (units[i].link, units[i].side);
            if units[i].leader.is_some() && link >= 0 {
                units[i].superior =
                    (0..units.len()).find(|&k| k != i && units[k].leader.is_some() && units[k].side == side && units[k].group as i32 == link as i32);
            }
        }
        Self { units, wind: false, heat: 0, options: Default::default(), layout: scn.layout }
    }

    /// What a unit adds to its TQ check rolls: a rally's mark, the heat.
    pub fn tq_burden(&self, unit: usize) -> u32 {
        let u = &self.units[unit];
        let tired = u.nation == 11 && !crate::movement::MoveClass::of_type(u.type_id).is_some_and(|c| c.horse());
        u.rallied as u32 + if tired { self.heat } else { 0 }
    }

    /// Whether `unit` faces into the wind.
    pub fn into_wind(&self, unit: usize) -> bool {
        let u = &self.units[unit];
        self.wind && u.leader.is_none() && matches!(u.facing, 6 | 7)
    }

    /// The range `unit`'s missiles reach now (the wind's taken off).
    pub fn missile_range(&self, unit: usize) -> i32 {
        self.units[unit].missile.map_or(0, |(w, _)| w.range() - self.into_wind(unit) as i32)
    }

    /// Whether the battle is one of Hannibal's (its units are), whose
    /// rules differ in places.
    pub fn hannibal(&self) -> bool {
        self.layout.newer() || self.units.first().is_some_and(|u| u.type_id >= gboh_formats::units::HANNIBAL)
    }

    /// Whether the battle is one of Caesar's (Hannibal's engine; its
    /// nations' and battles' own rules not).
    pub fn caesar(&self) -> bool {
        matches!(self.layout, gboh_formats::scn::Layout::Caesar | gboh_formats::scn::Layout::Editor)
            || self.units.first().is_some_and(|u| u.type_id >= gboh_formats::units::CAESAR)
    }

    /// Indices of the leaders still in the battle.
    pub fn leaders(&self) -> impl Iterator<Item = usize> + '_ {
        self.units.iter().enumerate().filter(|(_, u)| u.leader.is_some() && u.on_field()).map(|(i, _)| i)
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
