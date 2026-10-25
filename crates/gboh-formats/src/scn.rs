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
//!
//! The *Collector's Edition*'s scenario editor writes its own layout
//! ([`Layout::Editor`], marked by a u32 0 and a u32 version 2), whose units
//! name their art directly and mix the three games':
//!
//! ```text
//! file  := u32 0, u32 2, guid[16], str8 title, str8 author, u32 n + text
//!          description, str8 map_file, u8 nation ×2, u8 retreat ×2,
//!          u16 rout level ×2, u8 n_pairs, (u8, u8)[n_pairs],
//!          u8 n + str8[n] home AI, u8 n + str8[n] visitor AI,
//!          u8 turn limit on, u16 turns − 1, u8 side,
//!          u16 n_groups, group[n_groups], u32 n_units, unit[n_units]
//! group := i16 id, str16 name, (u32 type, u16 turns, u16 hex)[20] orders,
//!          u32 behaviour
//! unit  := u32 kind (0..17), str8 name, u32 facing, i16 movement, i32
//!          nation, i16 hex, i16 (−1), i16 artwork, i32 catalogue id, i16
//!          group, kind's tail
//! ```
//! Tails: leaders i16 initiative, command range, charisma, u8 elite;
//! combat units i16 TQ, size, u8 two-hex (heavy cavalry, Roman and barbarian
//! cavalry a u8 cataphract more); missile units the combat fields, u32
//! missile, i16 range, i16 supply (legionaries and cohorts a u8 type more,
//! 0..=2).

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
    /// The *Collector's Edition* scenario editor's.
    Editor,
}

impl Layout {
    /// Hannibal's record layout (Hannibal's or Caesar's).
    pub fn newer(self) -> bool {
        self != Layout::Alexander
    }
}

#[derive(Debug, Clone, PartialEq)]
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
    /// What only the editor's layout has.
    pub editor: Option<EditorInfo>,
}

/// The *Collector's Edition* editor's scenario fields beyond the games'.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EditorInfo {
    pub guid: [u8; 16],
    pub author: String,
    pub description: String,
    /// The turn limit, if on: turns, and the side (0 home, 1 visitor) it
    /// goes to.
    pub turn_limit: Option<(u16, u8)>,
    pub groups: Vec<Group>,
}

/// An editor group: its leader's group id, the name its AI plan knows it
/// by, its timed orders (type 0 none, 1 defend, 2 move, 3 attack; turns;
/// hex) and its behaviour (1 defend, 3 attack).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Group {
    pub id: i16,
    pub name: String,
    pub orders: Vec<(u32, u16, u16)>,
    pub behaviour: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Side {
    pub nation: u8,
    /// Map direction (1 N, 3 E, 5 S, 7 W) the army's routed units flee in.
    pub retreat: u8,
    /// Rout points at which the army withdraws (loses).
    pub rout_level: u16,
}

#[derive(Debug, Clone, PartialEq)]
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
    /// The editor's: the art it is drawn with (its table's index, −1 none)
    /// and its unit catalogue entry.
    pub artwork: i16,
    pub catalogue: i32,
}

/// Fields that depend on the unit's class in the original code.
#[derive(Debug, Clone, PartialEq)]
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
        (Layout::Hannibal | Layout::Caesar | Layout::Editor, 0x15 | 0x19) => Legion,
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
        if data.len() >= 8 && data[..8] == [0, 0, 0, 0, 2, 0, 0, 0] {
            return Self::parse_editor(data);
        }
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
            units.push(Unit {
                kind,
                name,
                nation,
                unk_a,
                unk_b,
                hex,
                facing,
                extra,
                arrival,
                elite,
                link,
                layout,
                art_nation: nation,
                artwork: -1,
                catalogue: -1,
            });
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
        Ok(Self { layout, leaders, unk, title, map_file, unit_file, home, visitor, pairs, home_ai, visitor_ai, units, editor: None })
    }

    /// The scenario as a *Collector's Edition* editor file (see the
    /// module's notes): what [`Scenario::parse_editor`] reads. Units take
    /// their editor kind from `kind` (0..=17) and their art from `artwork`.
    pub fn write_editor(&self) -> Vec<u8> {
        let info = self.editor.clone().unwrap_or_default();
        let mut o = Vec::new();
        let str8 = |o: &mut Vec<u8>, s: &str| {
            let b: Vec<u8> = s.chars().take(255).map(|c| c as u8).collect();
            o.push(b.len() as u8);
            o.extend(b);
        };
        o.extend(0u32.to_le_bytes());
        o.extend(2u32.to_le_bytes());
        o.extend(info.guid);
        str8(&mut o, &self.title);
        str8(&mut o, &info.author);
        o.extend((info.description.len() as u32).to_le_bytes());
        o.extend(info.description.chars().map(|c| c as u8));
        str8(&mut o, &self.map_file);
        o.extend([self.home.nation, self.visitor.nation, self.home.retreat, self.visitor.retreat]);
        o.extend(self.home.rout_level.to_le_bytes());
        o.extend(self.visitor.rout_level.to_le_bytes());
        o.push(self.pairs.len() as u8);
        for &(a, b) in &self.pairs {
            o.extend([a, b]);
        }
        for ai in [&self.home_ai, &self.visitor_ai] {
            o.push(ai.len() as u8);
            for f in ai.iter() {
                str8(&mut o, f);
            }
        }
        let (on, turns, side) = info.turn_limit.map_or((0, 0, 0), |(t, s)| (1, t.saturating_sub(1), s));
        o.push(on);
        o.extend(turns.to_le_bytes());
        o.push(side);
        o.extend((info.groups.len() as u16).to_le_bytes());
        for g in &info.groups {
            o.extend(g.id.to_le_bytes());
            o.extend((g.name.len() as u16).to_le_bytes());
            o.extend(g.name.bytes());
            for k in 0..20 {
                let (t, d, h) = g.orders.get(k).copied().unwrap_or((0, 0, 0));
                o.extend(t.to_le_bytes());
                o.extend(d.to_le_bytes());
                o.extend(h.to_le_bytes());
            }
            o.extend(g.behaviour.to_le_bytes());
        }
        o.extend((self.units.len() as u32).to_le_bytes());
        for u in &self.units {
            o.extend((u.kind as u32).to_le_bytes());
            str8(&mut o, &u.name);
            o.extend((u.facing as u32).to_le_bytes());
            o.extend(u.unk_a.to_le_bytes());
            o.extend((u.nation as u32).to_le_bytes());
            o.extend(u.hex.to_le_bytes());
            o.extend((-1i16).to_le_bytes());
            o.extend(u.artwork.to_le_bytes());
            o.extend(u.catalogue.to_le_bytes());
            o.extend(u.unk_b.to_le_bytes());
            match &u.extra {
                UnitExtra::Leader([charisma, range, initiative, _]) => {
                    o.extend(initiative.to_le_bytes());
                    o.extend(range.to_le_bytes());
                    o.extend(charisma.to_le_bytes());
                    o.push(u.elite);
                }
                UnitExtra::Combat { a, b, c, more } => {
                    o.extend(b.to_le_bytes());
                    o.extend(a.to_le_bytes());
                    o.push(*c);
                    if let Some(m) = more {
                        o.push(*m);
                    }
                }
                UnitExtra::Missile { a, b, c, missile, m1, m2, legion, .. } => {
                    o.extend(b.to_le_bytes());
                    o.extend(a.to_le_bytes());
                    o.push(*c);
                    o.extend((*missile as u32).to_le_bytes());
                    o.extend(m1.to_le_bytes());
                    o.extend(m2.to_le_bytes());
                    if let Some(l) = legion {
                        o.push((*l - 1).max(0) as u8);
                    }
                }
            }
        }
        o
    }

    /// Read a scenario of the *Collector's Edition* editor's (see the
    /// module's notes) into the games' model: sizes and TQ in the games'
    /// order, leaders' ratings as theirs (charisma, command range,
    /// initiative, personal combat 0), legion types from 1, facing 0 as 3.
    pub fn parse_editor(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        if r.u32()? != 0 || r.u32()? != 2 {
            return Err(Error::Invalid { offset: 0, msg: "not an editor scenario (version 2)".into() });
        }
        let guid: [u8; 16] = r.bytes(16)?.try_into().unwrap();
        let title = str8(&mut r)?;
        let author = str8(&mut r)?;
        let n = r.u32()? as usize;
        let description = r.bytes(n)?.iter().map(|&b| b as char).collect();
        let map_file = str8(&mut r)?;
        let (hn, vn, he, ve) = (u8(&mut r)?, u8(&mut r)?, u8(&mut r)?, u8(&mut r)?);
        let (hr, vr) = (r.u16()?, r.u16()?);
        let home = Side { nation: hn, retreat: he, rout_level: hr };
        let visitor = Side { nation: vn, retreat: ve, rout_level: vr };
        let pairs: Vec<(u8, u8)> = (0..u8(&mut r)?).map(|_| Ok((u8(&mut r)?, u8(&mut r)?))).collect::<Result<_>>()?;
        let home_ai = (0..u8(&mut r)?).map(|_| str8(&mut r)).collect::<Result<_>>()?;
        let visitor_ai = (0..u8(&mut r)?).map(|_| str8(&mut r)).collect::<Result<_>>()?;
        let (on, turns, side) = (u8(&mut r)?, r.u16()?, u8(&mut r)?);
        let turn_limit = (on != 0).then_some((turns + 1, side));
        let mut groups = Vec::new();
        for _ in 0..r.u16()? {
            let id = r.u16()? as i16;
            let len = r.u16()? as usize;
            let name = r.bytes(len)?.iter().map(|&b| b as char).collect();
            let orders = (0..20).map(|_| Ok((r.u32()?, r.u16()?, r.u16()?))).collect::<Result<_>>()?;
            groups.push(Group { id, name, orders, behaviour: r.u32()? });
        }
        let units = (0..r.u32()?).map(|_| editor_unit(&mut r)).collect::<Result<_>>()?;
        let leaders = groups.iter().map(|g| (g.id as u16, g.name.clone())).collect();
        let next = groups.iter().map(|g| g.id as u16 + 1).max().unwrap_or(0);
        Ok(Self {
            layout: Layout::Editor,
            leaders,
            unk: next,
            title,
            map_file,
            unit_file: String::new(),
            home,
            visitor,
            pairs,
            home_ai,
            visitor_ai,
            units,
            editor: Some(EditorInfo { guid, author, description, turn_limit, groups }),
        })
    }
}

/// One unit record of the *Collector's Edition* editor's (its scenarios'
/// and its unit catalogues').
fn editor_unit(r: &mut Reader) -> Result<Unit> {
    let at = r.pos;
    let kind = r.u32()?;
    let name = str8(r)?;
    let facing = match r.u32()? {
        0 => 3,
        f => f as u8,
    };
    let movement = r.u16()?;
    let nation = r.u32()? as u8;
    let hex = r.u16()?;
    r.u16()?;
    let artwork = r.u16()? as i16;
    let catalogue = r.u32()? as i32;
    let group = r.u16()?;
    let (mut elite, kind_u8) = (0, kind as u8);
    let extra = match kind {
        0 => {
            let (initiative, range, charisma) = (r.u16()?, r.u16()?, r.u16()?);
            elite = u8(r)?;
            UnitExtra::Leader([charisma, range, initiative, 0])
        }
        1..=17 => {
            let (tq, size, double) = (r.u16()?, r.u16()?, u8(r)?);
            match kind {
                6 | 9 | 12 => UnitExtra::Combat { a: size, b: tq, c: double, more: None },
                2 | 14 | 16 => UnitExtra::Combat { a: size, b: tq, c: double, more: Some(u8(r)?) },
                _ => {
                    let (missile, range, supply) = (r.u32()? as u8, r.u16()?, r.u16()?);
                    let legion = if matches!(kind, 13 | 17) { Some(u8(r)? as i16 + 1) } else { None };
                    UnitExtra::Missile { a: size, b: tq, c: double, missile, m1: range, m2: supply, more: None, legion }
                }
            }
        }
        _ => return Err(Error::Invalid { offset: at, msg: format!("unknown editor unit kind {kind}") }),
    };
    Ok(Unit {
        kind: kind_u8,
        name,
        nation,
        unk_a: movement,
        unk_b: group,
        hex,
        facing,
        extra,
        arrival: 0,
        elite,
        link: -1,
        layout: Layout::Editor,
        art_nation: nation,
        artwork,
        catalogue,
    })
}

/// The *Collector's Edition* editor's unit catalogue (`MainUnits.unt`: a
/// u32 count and that many unit records): units of every nation of the
/// three games, to draw from.
pub fn parse_catalogue(data: &[u8]) -> Result<Vec<Unit>> {
    let mut r = Reader::new(data);
    (0..r.u32()?).map(|_| editor_unit(&mut r)).collect()
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
