//! Routed units fleeing and rallying (see `docs/formats.md`, "Routs").

use crate::battle::{Battle, neighbour};
use crate::hex::Hex;
use crate::movement::{MoveClass, in_enemy_zoc, on_map, step_cost};
use gboh_formats::map::Map;

/// Map directions to try, in order, for a retreat direction.
fn preferences(retreat: u8) -> &'static [usize] {
    match retreat {
        1 => &[1, 8, 2],
        3 => &[2, 4, 1, 5],
        5 => &[5, 6, 4],
        7 => &[8, 6, 1, 5],
        _ => &[],
    }
}

/// Index into `Hex::neighbours` for a map side.
fn side_index(side: usize) -> Option<usize> {
    [1, 2, 4, 5, 6, 8].iter().position(|&s| s == side)
}

/// How a rout move ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flight {
    /// The unit moved along `path` (possibly through friendly units).
    Moved(Vec<Hex>),
    /// It reached the map edge or could go no further: it is out of the
    /// battle (terminal rout or elimination).
    Gone(Vec<Hex>),
}

/// Flee up to `steps` hexes towards the unit's retreat direction
/// (`FUN_0057ae96`). Each step tries the retreat's directions in order (east
/// and west zig-zag: odd steps swap the first two), in three passes: a
/// passable empty hex outside enemy zones of control; then one outside the
/// zones holding friendly units; then one holding friendly units, zones or
/// not (an empty hex in a zone is never entered, nor an enemy's). The first
/// direction off the map takes it off the battlefield; no hex, or one it
/// has already been through, means a terminal rout. It runs a step further
/// if it ends on an occupied hex, and another if next to an enemy leader.
/// Two-hex units are moved by their main hex.
pub fn flee(battle: &Battle, map: &Map, unit: usize, steps: u32) -> Flight {
    let u = &battle.units[unit];
    let class = MoveClass::of_type(u.type_id).unwrap_or(MoveClass::HeavyInfantry);
    let prefs = preferences(u.retreat);
    let mut here = u.hex;
    let mut path = Vec::new();
    let occupant = |h: Hex| battle.units.iter().enumerate().find(|&(i, o)| i != unit && o.on_field() && battle.hexes(i).contains(&h)).map(|(_, o)| o.side);
    let mut budget = steps;
    let mut extended = false;
    let mut step = 0;
    while step < budget {
        let mut order: Vec<usize> = prefs.to_vec();
        if matches!(u.retreat, 3 | 7) && step % 2 == 1 && order.len() > 1 {
            order.swap(0, 1);
        }
        let mut next = None;
        'pass: for pass in 0..3 {
            for &side in &order {
                let Some(to) = neighbour(here, side) else { continue };
                if !on_map(map, to) {
                    // fled off the map
                    path.push(to);
                    return Flight::Gone(path);
                }
                let passable = side_index(side).and_then(|d| step_cost(map, class, here, d)).is_some();
                let zoc = in_enemy_zoc(battle, map, u.side, to);
                let ok = passable
                    && match (pass, occupant(to)) {
                        (0, None) => !zoc,
                        (1, Some(s)) => s == u.side && !zoc,
                        (2, Some(s)) => s == u.side,
                        _ => false,
                    };
                if ok {
                    next = Some(to);
                    break 'pass;
                }
            }
        }
        match next {
            Some(to) if to != u.hex && !path.contains(&to) => {
                path.push(to);
                here = to;
            }
            _ => return Flight::Gone(path),
        }
        step += 1;
        if step == budget && !extended {
            extended = true;
            let enemy_leader_near = battle.units.iter().any(|o| o.side != u.side && o.on_field() && o.leader.is_some() && o.hex.distance(here) == 1);
            budget += occupant(here).is_some() as u32 + enemy_leader_near as u32;
        }
    }
    Flight::Moved(path)
}

/// Hits removed by a successful rally, by `roll ÷ 2` and TQ band (under 4,
/// 4–5, 6 or more); `None` means the rally fails after all.
const RALLY: [[Option<i32>; 3]; 5] = [
    [Some(1), Some(1), Some(1)],
    [Some(2), Some(2), Some(2)],
    [Some(2), Some(2), Some(2)],
    [None, Some(3), Some(3)],
    [None, Some(4), Some(4)],
];

/// A rally attempt on a routed unit with troop quality `tq` and `hits`, by
/// a leader with `initiative`. The first roll fails if it exceeds the
/// initiative + 1; the second, halved, and the TQ band pick the hits
/// removed (or a failure). Returns the unit's hits after a success; `None`
/// means it failed and the unit routs terminally.
pub fn rally(tq: u16, hits: i32, initiative: u16, rallied: bool, mut roll: impl FnMut() -> u32) -> Option<i32> {
    // (Caesar's: a unit rallied before rolls a point worse, and two worse
    // on the hits it sheds)
    if roll() as i32 + rallied as i32 > initiative as i32 + 1 {
        return None;
    }
    let band = match tq {
        0..=3 => 0,
        4 | 5 => 1,
        _ => 2,
    };
    let r = if rallied { (roll() + 2).min(9) } else { roll() };
    let removed = RALLY[(r / 2).min(4) as usize][band]?;
    Some((hits - removed).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::Unit;

    fn unit(side: u8, hex: Hex, retreat: u8) -> Unit {
        Unit {
            source: 0,
            name: String::new(),
            type_id: 31,
            nation: side,
            side,
            group: 0,
            hex,
            facing: 3,
            leader: None,
            retreat,
            max_mp: 5,
            mp: 5,
            turned: false,
            size: 6,
            tq: 6,
            hits: 6,
            state: crate::battle::UnitState::Fighting,
            arrival: 0,
            elite: false,
            superior: None,
            link: -1,
            legion: None,
            missile: None,
            fired: false,
            wounded: false,
            reordered: false,
            reorder_hit: false,
            rallied: false,
            done: false,
        }
    }

    fn map() -> Map {
        Map {
            name: String::new(),
            width: 12,
            height: 12,
            hex_id_origin: 1000,
            unk_b: 1,
            elevation: vec![1; 144],
            cells: vec![Vec::new(); (2 * 12 + 2) * (4 * 12 + 1)],
            caesar: false,
            fast_roads: false,
        }
    }

    #[test]
    fn flees_west_in_a_straight_line() {
        let b = Battle { units: vec![unit(2, Hex::new(6, 5), 7)], ..Default::default() };
        let Flight::Moved(path) = flee(&b, &map(), 0, 4) else { panic!() };
        assert_eq!(path.len(), 4);
        assert_eq!(path[3].x, 2);
        // zig-zag keeps the row within half a hex
        assert!(path.iter().all(|h| (h.y - 5).abs() <= 1));
    }

    #[test]
    fn rallies() {
        let rolls = |a: u32, b: u32| {
            let mut v = vec![b, a];
            move || v.pop().unwrap()
        };
        // initiative 7: the first roll must be at most 8
        assert_eq!(rally(6, 6, 7, false, rolls(9, 0)), None);
        assert_eq!(rally(6, 6, 7, false, rolls(8, 9)), Some(2));
        // poor troops can fail on the second roll
        assert_eq!(rally(3, 3, 7, false, rolls(0, 6)), None);
        assert_eq!(rally(3, 3, 7, false, rolls(0, 0)), Some(2));
    }

    #[test]
    fn leaves_the_map_at_the_edge() {
        let b = Battle { units: vec![unit(2, Hex::new(1, 5), 7)], ..Default::default() };
        assert!(matches!(flee(&b, &map(), 0, 4), Flight::Gone(_)));
    }

    #[test]
    fn avoids_enemy_zones() {
        // an enemy west of the path forces the unit round it
        let mut b = Battle { units: vec![unit(2, Hex::new(6, 5), 7), unit(1, Hex::new(4, 4), 3)], ..Default::default() };
        b.units[1].hits = 0;
        let Flight::Moved(path) = flee(&b, &map(), 0, 1) else { panic!() };
        assert!(path[0].distance(Hex::new(4, 4)) > 1);
    }
}
