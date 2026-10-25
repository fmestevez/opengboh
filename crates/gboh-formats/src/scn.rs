//! `.SCN` scenarios: leaders, map/AI references, sides and the order of battle.
//!
//! ```text
//! file    := u16 n_leaders, leader[n_leaders], u16 unk,
//!            str8 title, str8 map_file, str8 unit_file (ignored by the game),
//!            side home, side visitor,
//!            u8 n_pairs, (u8, u8)[n_pairs],
//!            u8 n_home_ai, u8 n_visitor_ai, str8 ai_file[n_home_ai + n_visitor_ai],
//!            unit* (until end of file)
//! leader  := u16 id, u16 len, name[len]
//! side    := u8 nation, u8 retreat direction, u8 rout level (u16 in Hannibal's
//!            and Caesar's)
//! unit    := u8 kind, base, kind-specific fields
//! base    := str8 name, u8 nation, u16 unk_a, u16 unk_b, u16 hex, u8 facing,
//!            (Hannibal's and Caesar's:) u16 arrival turn
//! ```
//! Hannibal's and Caesar's leaders add a u8 (elite initiative) and an i16
//! (link group, −1 none) to Alexander's four ratings; their legion kinds
//! (21, and Caesar's 25) add an i16 legion type to the missile fields. The
//! layout isn't marked in the file: [`Scenario::parse`] tells it by which
//! one reads the file through.
//! `hex` is a map hex id, `map.hex_id_origin + x * 100 + y`
//! (see [`crate::map::Map::hex_from_id`]). `facing` is a map direction 1..8 (N, NE, E,
//! SE, S, SW, W, NW); scenarios use 2, 3, 4, 6, 7 and 8. The nation and missile bytes are
//! remapped by the game, see [`crate::units`].

use crate::read::Reader;
use crate::{Error, Result};

/// The scenario layouts: Alexander's, and Hannibal's (also Caesar's, which
/// is told from Hannibal's by its nations and unit kinds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    Alexander,
    Hannibal,
    Caesar,
}

impl Layout {
    /// Hannibal's record layout (Hannibal's or Caesar's).
    pub fn newer(self) -> bool {
        self != Layout::Alexander
    }
}

#[derive(Debug, Clone)]
pub struct Scenario {
    /// The layout the file was read in.
    pub layout: Layout,
    pub leaders: Vec<(u16, String)>,
    pub unk: u16,
    pub title: String,
    pub map_file: String,
    pub unit_file: String,
    pub home: Side,
    pub visitor: Side,
    pub pairs: Vec<(u8, u8)>,
    pub home_ai: Vec<String>,
    pub visitor_ai: Vec<String>,
    pub units: Vec<Unit>,
}

#[derive(Debug, Clone, Copy)]
pub struct Side {
    pub nation: u8,
    /// Map direction (1 N, 3 E, 5 S, 7 W) the army's routed units flee in.
    pub retreat: u8,
    /// Rout points at which the army withdraws (loses).
    pub rout_level: u16,
}

#[derive(Debug, Clone)]
pub struct Unit {
    pub kind: u8,
    pub name: String,
    pub nation: u8,
    pub unk_a: u16,
    pub unk_b: u16,
    /// Map hex id; resolve with [`crate::map::Map::hex_from_id`].
    pub hex: u16,
    pub facing: u8,
    pub extra: UnitExtra,
    /// The turn the unit arrives on the field (0: there from the start;
    /// Hannibal's and Caesar's scenarios only).
    pub arrival: u16,
    /// A leader's elite initiative flag and link group (−1 none; Hannibal's
    /// and Caesar's scenarios only).
    pub elite: u8,
    pub link: i16,
    /// The layout of the scenario it was read from (which game's unit
    /// types it has).
    pub layout: Layout,
    /// The nation its art is chosen by: Caesar's scenarios' nation pairs
    /// applied (its Sullans drawn as Pompeians...); else its nation.
    pub art_nation: u8,
}

/// Fields that depend on the unit's class in the original code.
#[derive(Debug, Clone)]
pub enum UnitExtra {
    /// `LeaderUnitData`.
    Leader([u16; 4]),
    /// `CombatUnitData`: two u16 values (the second looks like troop quality) and a u8.
    Combat { a: u16, b: u16, c: u8, more: Option<u8> },
    /// `MissileCapableUnitData`: combat fields plus missile type (1..=4) and two u16.
    /// `legion` is Hannibal's and Caesar's legion type (1, 2, 3).
    Missile { a: u16, b: u16, c: u8, missile: u8, m1: u16, m2: u16, more: Option<u8>, legion: Option<i16> },
}

/// How a unit kind's fields are read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Combat,
    /// Combat fields and a trailing u8 (heavy cavalry: the cataphract flag).
    CombatMore,
    Missile,
    /// Missile fields and a trailing u8 (elephants).
    MissileMore,
    /// Missile fields and an i16 legion type.
    Legion,
}

/// Which reader the original game uses for each unit kind byte.
fn unit_class(layout: Layout, kind: u8) -> Option<Class> {
    use Class::*;
    Some(match (layout, kind) {
        (_, 1 | 3 | 4 | 7 | 8 | 0x0a | 0x11 | 0x12) => Missile,
        (_, 5) => MissileMore,
        (_, 6 | 9 | 0x0e | 0x0f | 0x13) => Combat,
        (_, 2) => CombatMore,
        // Hannibal's legions and Roman cavalry, Caesar's tribal missile
        // troops, Germans and cohorts
        (Layout::Hannibal | Layout::Caesar, 0x15 | 0x19) => Legion,
        (Layout::Hannibal | Layout::Caesar, 0x16) => CombatMore,
        (Layout::Hannibal | Layout::Caesar, 0x17) => Missile,
        (Layout::Hannibal | Layout::Caesar, 0x18) => Combat,
        _ => return None,
    })
}

fn str8(r: &mut Reader) -> Result<String> {
    let n = r.bytes(1)?[0] as usize;
    Ok(r.bytes(n)?.iter().map(|&b| b as char).collect())
}

fn u8(r: &mut Reader) -> Result<u8> {
    Ok(r.bytes(1)?[0])
}

impl Scenario {
    /// Read a scenario of any of the games, in whichever layout reads it
    /// through (Alexander's, else Hannibal's; then, for an early Hannibal
    /// file, with leaders lacking the elite byte).
    pub fn parse(data: &[u8]) -> Result<Self> {
        Self::parse_as(data, Layout::Alexander, false)
            .or_else(|e| Self::parse_as(data, Layout::Hannibal, false).map_err(|_| e))
            .or_else(|e| Self::parse_as(data, Layout::Hannibal, true).map_err(|_| e))
    }

    /// Read a scenario in `layout` (`short_leaders`: leaders without the
    /// elite byte, as in an early Hannibal file the game no longer reads),
    /// checking each unit looks right.
    pub fn parse_as(data: &[u8], layout: Layout, short_leaders: bool) -> Result<Self> {
        let newer = layout.newer();
        let mut r = Reader::new(data);
        let n = r.u16()?;
        let mut leaders = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let id = r.u16()?;
            let len = r.u16()? as usize;
            leaders.push((id, r.bytes(len)?.iter().map(|&b| b as char).collect()));
        }
        let unk = r.u16()?;
        let title = str8(&mut r)?;
        let map_file = str8(&mut r)?;
        let unit_file = str8(&mut r)?;
        let mut side = || -> Result<Side> {
            let (nation, retreat) = (u8(&mut r)?, u8(&mut r)?);
            let rout_level = if newer { r.u16()? } else { u8(&mut r)? as u16 };
            Ok(Side { nation, retreat, rout_level })
        };
        let home = side()?;
        let visitor = side()?;
        let pairs: Vec<(u8, u8)> = (0..u8(&mut r)?).map(|_| Ok((u8(&mut r)?, u8(&mut r)?))).collect::<Result<_>>()?;
        let (nh, nv) = (u8(&mut r)?, u8(&mut r)?);
        let home_ai = (0..nh).map(|_| str8(&mut r)).collect::<Result<_>>()?;
        let visitor_ai = (0..nv).map(|_| str8(&mut r)).collect::<Result<_>>()?;

        let mut units = Vec::new();
        while r.remaining() > 0 {
            let at = r.pos;
            let bad = |msg: String| Error::Invalid { offset: at, msg };
            let kind = u8(&mut r)?;
            let name = str8(&mut r)?;
            let nation = u8(&mut r)?;
            let unk_a = r.u16()?;
            let unk_b = r.u16()?;
            let hex = r.u16()?;
            let facing = u8(&mut r)?;
            // a misread layout shows soon: names of control characters,
            // facings or nations out of range
            if name.is_empty() || name.chars().any(|c| (c as u32) < 0x20) || facing > 8 || nation > 15 {
                return Err(bad(format!("not a unit record ({layout:?} layout)")));
            }
            let arrival = if newer { r.u16()? } else { 0 };
            let (mut elite, mut link) = (0, -1);
            let extra = if kind == 0 {
                let ratings = [r.u16()?, r.u16()?, r.u16()?, r.u16()?];
                if newer {
                    if !short_leaders {
                        elite = u8(&mut r)?;
                    }
                    link = r.u16()? as i16;
                }
                UnitExtra::Leader(ratings)
            } else {
                let class = unit_class(layout, kind).ok_or_else(|| bad(format!("unknown unit kind {kind}")))?;
                let (a, b, c) = (r.u16()?, r.u16()?, u8(&mut r)?);
                match class {
                    Class::Combat | Class::CombatMore => {
                        let more = if class == Class::CombatMore { Some(u8(&mut r)?) } else { None };
                        UnitExtra::Combat { a, b, c, more }
                    }
                    Class::Missile | Class::MissileMore | Class::Legion => {
                        let (m, m1, m2) = (u8(&mut r)?, r.u16()?, r.u16()?);
                        let more = if class == Class::MissileMore { Some(u8(&mut r)?) } else { None };
                        let legion = if class == Class::Legion { Some(r.u16()? as i16) } else { None };
                        UnitExtra::Missile { a, b, c, missile: m, m1, m2, more, legion }
                    }
                }
            };
            units.push(Unit { kind, name, nation, unk_a, unk_b, hex, facing, extra, arrival, elite, link, layout, art_nation: nation });
        }
        // Caesar's: told by its nations (9 Caesarian, 10 Pompeian, 11
        // barbarians) or its kinds (23..=25); its units' art by the header's
        // nation pairs (`FUN_004a4318`, in order)
        let caesar = newer && units.iter().any(|u| u.nation >= 9 || u.kind >= 0x17);
        let layout = if caesar { Layout::Caesar } else { layout };
        for u in &mut units {
            u.layout = layout;
            if caesar {
                for &(a, b) in &pairs {
                    if u.art_nation == a {
                        u.art_nation = b;
                    }
                }
            }
        }
        Ok(Self { layout, leaders, unk, title, map_file, unit_file, home, visitor, pairs, home_ai, visitor_ai, units })
    }
}

/// Map files in the order of the original's battle table (`0x5e4394`); many
/// per-battle rules are indexed by it.
const BATTLES: [&str; 11] = ["CHAERONE", "LYGINUS", "PELIUM", "GRANICUS", "ISSUS", "GAUGAMEL", "SAMARKAN", "JAXARTES", "ARIGAEUM", "HYDASPES", "PARAIT"];

/// The battle index of a map file name (case-insensitive, extension
/// ignored).
pub fn battle_index(map_file: &str) -> Option<usize> {
    let stem = map_file.split('.').next().unwrap_or("").to_ascii_uppercase();
    BATTLES.iter().position(|b| *b == stem)
}
