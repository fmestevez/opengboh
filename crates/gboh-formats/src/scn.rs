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
//! side    := u8 nation, u8 retreat direction, u8 rout level
//! unit    := u8 kind, base, kind-specific fields
//! base    := str8 name, u8 nation, u16 unk_a, u16 unk_b, u16 hex, u8 facing
//! ```
//! `hex` is a map hex id, `map.hex_id_origin + x * 100 + y`
//! (see [`crate::map::Map::hex_from_id`]). `facing` is a map direction 1..8 (N, NE, E,
//! SE, S, SW, W, NW); scenarios use 2, 3, 4, 6, 7 and 8. The nation and missile bytes are
//! remapped by the game, see [`crate::units`].

use crate::read::Reader;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Scenario {
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
    pub rout_level: u8,
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
}

/// Fields that depend on the unit's class in the original code.
#[derive(Debug, Clone)]
pub enum UnitExtra {
    /// `LeaderUnitData`.
    Leader([u16; 4]),
    /// `CombatUnitData`: two u16 values (the second looks like troop quality) and a u8.
    Combat { a: u16, b: u16, c: u8, more: Option<u8> },
    /// `MissileCapableUnitData`: combat fields plus missile type (1..=4) and two u16.
    Missile { a: u16, b: u16, c: u8, missile: u8, m1: u16, m2: u16, more: Option<u8> },
}

/// Which reader the original game uses for each unit kind byte.
fn unit_class(kind: u8) -> Option<(bool, bool)> {
    // (missile capable, has trailing u8)
    match kind {
        1 | 3 | 4 | 7 | 8 | 0x0a | 0x11 | 0x12 => Some((true, false)),
        5 => Some((true, true)),
        6 | 9 | 0x0e | 0x0f | 0x13 => Some((false, false)),
        2 => Some((false, true)),
        _ => None,
    }
}

fn str8(r: &mut Reader) -> Result<String> {
    let n = r.bytes(1)?[0] as usize;
    Ok(r.bytes(n)?.iter().map(|&b| b as char).collect())
}

fn u8(r: &mut Reader) -> Result<u8> {
    Ok(r.bytes(1)?[0])
}

impl Scenario {
    pub fn parse(data: &[u8]) -> Result<Self> {
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
        let mut side = || -> Result<Side> { Ok(Side { nation: u8(&mut r)?, retreat: u8(&mut r)?, rout_level: u8(&mut r)? }) };
        let home = side()?;
        let visitor = side()?;
        let pairs = (0..u8(&mut r)?).map(|_| Ok((u8(&mut r)?, u8(&mut r)?))).collect::<Result<_>>()?;
        let (nh, nv) = (u8(&mut r)?, u8(&mut r)?);
        let home_ai = (0..nh).map(|_| str8(&mut r)).collect::<Result<_>>()?;
        let visitor_ai = (0..nv).map(|_| str8(&mut r)).collect::<Result<_>>()?;

        let mut units = Vec::new();
        while r.remaining() > 0 {
            let at = r.pos;
            let kind = u8(&mut r)?;
            let name = str8(&mut r)?;
            let nation = u8(&mut r)?;
            let unk_a = r.u16()?;
            let unk_b = r.u16()?;
            let hex = r.u16()?;
            let facing = u8(&mut r)?;
            let extra = if kind == 0 {
                UnitExtra::Leader([r.u16()?, r.u16()?, r.u16()?, r.u16()?])
            } else {
                let (missile, more) = unit_class(kind)
                    .ok_or_else(|| Error::Invalid { offset: at, msg: format!("unknown unit kind {kind}") })?;
                let (a, b, c) = (r.u16()?, r.u16()?, u8(&mut r)?);
                if missile {
                    let (m, m1, m2) = (u8(&mut r)?, r.u16()?, r.u16()?);
                    let more = if more { Some(u8(&mut r)?) } else { None };
                    UnitExtra::Missile { a, b, c, missile: m, m1, m2, more }
                } else {
                    let more = if more { Some(u8(&mut r)?) } else { None };
                    UnitExtra::Combat { a, b, c, more }
                }
            };
            units.push(Unit { kind, name, nation, unk_a, unk_b, hex, facing, extra });
        }
        Ok(Self { leaders, unk, title, map_file, unit_file, home, visitor, pairs, home_ai, visitor_ai, units })
    }
}

/// Map files in the order of the original's battle table (`0x5e4394`); many
/// per-battle rules are indexed by it.
const BATTLES: [&str; 11] = [
    "CHAERONE", "LYGINUS", "PELIUM", "GRANICUS", "ISSUS", "GAUGAMEL", "SAMARKAN", "JAXARTES",
    "ARIGAEUM", "HYDASPES", "PARAIT",
];

/// The battle index of a map file name (case-insensitive, extension
/// ignored).
pub fn battle_index(map_file: &str) -> Option<usize> {
    let stem = map_file.split('.').next().unwrap_or("").to_ascii_uppercase();
    BATTLES.iter().position(|b| *b == stem)
}
