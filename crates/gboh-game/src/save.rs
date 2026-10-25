//! Reading the original's saved games (`*.alx`, e.g. `Autosave.alx`) into a
//! battle (see `docs/formats.md`, "Saved games").
//!
//! Only the units' state is read: the file also holds the commentary, the
//! sprites and the activation state, which are skipped. The unit section is
//! found by its shape (the first offset where every record parses with sane
//! values), as the parts before it are not decoded.

use crate::battle::{Battle, UnitState};
use crate::hex::Hex;
use crate::missile::Weapon;
use crate::movement::MoveClass;

/// One unit's saved state (the original's base and combat records).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Saved {
    pub mp: u16,
    pub done: bool,
    pub reordered: bool,
    pub hex: u16,
    pub facing: u32,
    pub gone: bool,
    pub hits: u16,
    pub routed: bool,
    pub missiles: Option<u16>,
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]))
}

/// The base record: movement left, an unknown value, done, ordered again,
/// an unknown flag, a flag adding a trailing value, hex, second hex, facing
/// (4 bytes), out of the battle. Returns the record and the next offset.
fn base(d: &[u8], mut o: usize, lenient: bool) -> Option<(Saved, usize, u16)> {
    let mut v = [0u16; 8];
    for x in &mut v {
        *x = u16_at(d, o)?;
        o += 2;
    }
    let facing = u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?);
    o += 4;
    let gone = u16_at(d, o)?;
    o += 2;
    if v[5] != 0 {
        o += 2;
    }
    let [mp, _, done, reordered, a42, extra, hex, _] = v;
    let sane = mp <= 20 && done <= 1 && reordered <= 1 && a42 <= 1 && extra <= 1 && gone <= 1;
    let on_map = |h: u16| h / 100 < 60 && h % 100 < 60;
    let placed = on_map(hex) && (1..=8).contains(&facing);
    if !sane || (gone == 0 && !placed && !lenient) {
        return None;
    }
    // a leader killed in battle keeps a stale record with no valid hex
    let gone = gone != 0 || !placed;
    let s = Saved { mp, done: done != 0, reordered: reordered != 0, hex, facing, gone, hits: 0, routed: false, missiles: None };
    Some((s, o, extra))
}

/// A combat unit's record: base, hits, routed, two unknown values, and for
/// missile units missiles left and two unknown values.
fn combat(d: &[u8], o: usize, missile: bool) -> Option<(Saved, usize)> {
    let (mut s, mut o, extra) = base(d, o, false)?;
    s.hits = u16_at(d, o)?;
    let routed = u16_at(d, o + 2)?;
    o += 8;
    if extra != 0 {
        o += 2;
        let n = u16_at(d, o)? as usize;
        o += 2 + 2 * n;
    }
    if routed > 1 || s.hits > 30 {
        return None;
    }
    s.routed = routed != 0;
    if missile {
        let m = u16_at(d, o)?;
        if m > 99 {
            return None;
        }
        s.missiles = Some(m);
        o += 6;
    }
    Some((s, o))
}

/// A leader's record (the original's `FUN_005622d4`): a flag and, if set,
/// 20 bytes of activation state; a flag and, if set, the four ratings; a
/// chain of named entries; then base.
fn leader(d: &[u8], mut o: usize) -> Option<(Saved, usize)> {
    match u16_at(d, o)? {
        0 => o += 2,
        1 => o += 22,
        _ => return None,
    }
    match u16_at(d, o)? {
        0 => o += 2,
        1 => o += 10,
        _ => return None,
    }
    loop {
        match u16_at(d, o)? {
            0 => break,
            1 => {
                let n = u16_at(d, o + 2)? as usize;
                if n > 256 {
                    return None;
                }
                o += 4 + n + 2;
            }
            _ => return None,
        }
    }
    let (s, o, _) = base(d, o + 2, true)?;
    Some((s, o))
}

/// The scenario units in the order the save lists them: combat units of the
/// home army, then the visitor's, each in scenario order, then the leaders
/// the same way. Each with whether its record has the missile fields.
fn order(battle: &Battle, home: u8) -> (Vec<(usize, bool)>, Vec<usize>) {
    let mut units: Vec<usize> = (0..battle.units.len()).collect();
    units.sort_by_key(|&u| (battle.units[u].side != home, battle.units[u].source));
    let missile = |u: usize| battle.units[u].missile.is_some() && MoveClass::of_type(battle.units[u].type_id) != Some(MoveClass::Chariot);
    let combat = units.iter().filter(|&&u| battle.units[u].leader.is_none()).map(|&u| (u, missile(u))).collect();
    // leaders are saved in plain scenario order (not by side)
    let mut leaders: Vec<usize> = units.into_iter().filter(|&u| battle.units[u].leader.is_some()).collect();
    leaders.sort_by_key(|&u| battle.units[u].source);
    (combat, leaders)
}

/// Parse the unit section starting at `o`.
fn parse_at(d: &[u8], o: usize, combat_order: &[(usize, bool)], leaders: &[usize]) -> Option<Vec<(usize, Saved)>> {
    parse_from(d, o, combat_order, leaders).ok()
}

/// As `parse_at`, but on failure gives the index of the record that failed
/// and its offset (useful when a new save variant does not parse).
fn parse_from(d: &[u8], o: usize, combat_order: &[(usize, bool)], leaders: &[usize]) -> Result<Vec<(usize, Saved)>, (usize, usize)> {
    let mut o = o;
    let mut out = Vec::new();
    for (i, &(u, missile)) in combat_order.iter().enumerate() {
        // chariots save the missile fields or not depending on their class
        // (not decoded): take the form after which the next record parses
        let next_ok = |n: usize| match combat_order.get(i + 1) {
            Some(&(_, m)) => combat(d, n, m).is_some() || combat(d, n, !m).is_some(),
            None => leader(d, n).is_some(),
        };
        let (s, n) = [missile, !missile].into_iter().filter_map(|m| combat(d, o, m)).find(|&(_, n)| next_ok(n)).ok_or((i, o))?;
        out.push((u, s));
        o = n;
    }
    for (i, &u) in leaders.iter().enumerate() {
        let (s, n) = leader(d, o).ok_or((combat_order.len() + i, o))?;
        out.push((u, s));
        o = n;
    }
    Ok(out)
}

/// Why a save could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

/// The units' states in a save of `battle`'s scenario (whose home army is
/// nation `home`), by battle index.
pub fn read(d: &[u8], battle: &Battle, home: u8) -> Result<Vec<(usize, Saved)>, Error> {
    let (combat_order, leaders) = order(battle, home);
    (0..d.len()).find_map(|o| parse_at(d, o, &combat_order, &leaders)).ok_or_else(|| Error("no unit section found".into()))
}

/// The turn number in the save's title ("…; Turn #8; Spitaces").
pub fn turn(d: &[u8]) -> Option<u16> {
    let head = String::from_utf8_lossy(&d[..d.len().min(256)]).into_owned();
    let rest = &head[head.find("Turn #")? + 6..];
    rest.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// Leaders who have been active in the save's current turn, from the
/// Running Commentary it holds ("--- Turn #N ---  X is now active. …"), in
/// order.
pub fn active_this_turn(d: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(&d[..d.len().min(0x20000)]).into_owned();
    let Some(start) = text.rfind("--- Turn #") else { return Vec::new() };
    let mut out = Vec::new();
    for part in text[start..].split(" is now active.").collect::<Vec<_>>().split_last().map_or(&[][..], |(_, r)| r) {
        let name = part.rsplit("  ").next().unwrap_or("").trim();
        let name = name.rsplit(".  ").next().unwrap_or(name).trim();
        if !name.is_empty() && !out.iter().any(|n: &String| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// Apply saved states to `battle`.
pub fn apply(battle: &mut Battle, saved: &[(usize, Saved)]) {
    for &(u, s) in saved {
        let unit = &mut battle.units[u];
        unit.state = if s.gone {
            UnitState::Gone
        } else if s.routed {
            UnitState::Routed
        } else {
            UnitState::Fighting
        };
        if !s.gone {
            unit.hex = Hex::new((s.hex / 100) as i32, (s.hex % 100) as i32);
            unit.facing = s.facing as u8;
        }
        unit.mp = s.mp as i32;
        unit.hits = s.hits as i32;
        unit.done = s.done;
        unit.reordered = s.reordered;
        if let (Some((w, _)), Some(m)) = (unit.missile, s.missiles) {
            unit.missile = Some((w, m.min(Weapon::Bolt.supply())));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a saved game of Hydaspes; runs only with `GBOH_DATA` (the
    /// game's DATA directory) and `GBOH_SAVE` (an autosave of Hydaspes).
    #[test]
    fn reads_an_autosave() {
        let (Ok(dir), Ok(path)) = (std::env::var("GBOH_DATA"), std::env::var("GBOH_SAVE")) else { return };
        let dir = std::path::Path::new(&dir);
        let scn = gboh_formats::scn::Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = gboh_formats::map::Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let battle = Battle::from_scenario(&scn, &map);
        let d = std::fs::read(path).unwrap();
        let saved = read(&d, &battle, scn.home.nation).unwrap();
        assert_eq!(saved.len(), battle.units.len());
        // rout points must agree with the original's Game Overview
        let mut b = battle.clone();
        apply(&mut b, &saved);
        let lost: u16 = b
            .units
            .iter()
            .filter(|u| u.state == UnitState::Gone)
            .map(|u| crate::combat::rout_points(u.type_id, u.tq, u.leader.map_or(0, |r| r.initiative)))
            .sum();
        println!("rout points lost (both armies): {lost}");
        for &(u, sv) in &saved {
            if !sv.gone && ![2, 3, 4, 6, 7, 8].contains(&sv.facing) {
                println!("odd facing {}: {} at {}", sv.facing, battle.units[u].name, sv.hex);
            }
            if std::env::var("GBOH_LIST").is_ok() && battle.units[u].nation == 2 && !sv.gone {
                println!("{:24} hex {:4} facing {} hits {} routed {} mp {}", battle.units[u].name, sv.hex, sv.facing, sv.hits, sv.routed, sv.mp);
            }
        }
        assert!(turn(&d).is_some());
        let active = active_this_turn(&d);
        println!("active this turn: {active:?}");
        assert!(!active.is_empty());
    }
}
