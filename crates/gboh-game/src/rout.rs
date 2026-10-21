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

/// Flee up to `steps` hexes towards the unit's retreat direction. Each step
/// tries the retreat's directions in order (east and west zig-zag: odd steps
/// swap the first two) and takes the first that is on the map, passable,
/// outside enemy zones of control and empty; failing that, the first such
/// hex holding friendly units (it routs through them). Two-hex units are
/// moved by their main hex. 🟡 What the original does with units routed
/// through is not applied here.
pub fn flee(battle: &Battle, map: &Map, unit: usize, steps: u32) -> Flight {
    let u = &battle.units[unit];
    let class = MoveClass::of_type(u.type_id).unwrap_or(MoveClass::HeavyInfantry);
    let prefs = preferences(u.retreat);
    let mut here = u.hex;
    let mut path = Vec::new();
    for step in 0..steps {
        let mut order: Vec<usize> = prefs.to_vec();
        if matches!(u.retreat, 3 | 7) && step % 2 == 1 && order.len() > 1 {
            order.swap(0, 1);
        }
        let free = |h: Hex, allow_friends: bool| {
            battle.units.iter().enumerate().all(|(i, o)| {
                i == unit || o.state == crate::battle::UnitState::Gone || o.hex != h || (allow_friends && o.side == u.side)
            })
        };
        let mut next = None;
        'pass: for allow_friends in [false, true] {
            for &side in &order {
                let Some(to) = neighbour(here, side) else { continue };
                if !on_map(map, to) {
                    // fled off the map
                    path.push(to);
                    return Flight::Gone(path);
                }
                let passable = side_index(side).and_then(|d| step_cost(map, class, here, d)).is_some();
                if passable && !in_enemy_zoc(battle, u.side, to) && free(to, allow_friends) {
                    next = Some(to);
                    break 'pass;
                }
            }
        }
        match next {
            Some(to) => {
                path.push(to);
                here = to;
            }
            None => return Flight::Gone(path),
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
pub fn rally(tq: u16, hits: i32, initiative: u16, mut roll: impl FnMut() -> u32) -> Option<i32> {
    if roll() as i32 > initiative as i32 + 1 {
        return None;
    }
    let band = match tq {
        0..=3 => 0,
        4 | 5 => 1,
        _ => 2,
    };
    let removed = RALLY[(roll() / 2).min(4) as usize][band]?;
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
            missile: None,
            fired: false,
            wounded: false,
            reordered: false,
            reorder_hit: false,
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
        }
    }

    #[test]
    fn flees_west_in_a_straight_line() {
        let b = Battle { units: vec![unit(2, Hex::new(6, 5), 7)] };
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
        assert_eq!(rally(6, 6, 7, rolls(9, 0)), None);
        assert_eq!(rally(6, 6, 7, rolls(8, 9)), Some(2));
        // poor troops can fail on the second roll
        assert_eq!(rally(3, 3, 7, rolls(0, 6)), None);
        assert_eq!(rally(3, 3, 7, rolls(0, 0)), Some(2));
    }

    #[test]
    fn leaves_the_map_at_the_edge() {
        let b = Battle { units: vec![unit(2, Hex::new(1, 5), 7)] };
        assert!(matches!(flee(&b, &map(), 0, 4), Flight::Gone(_)));
    }

    #[test]
    fn avoids_enemy_zones() {
        // an enemy west of the path forces the unit round it
        let mut b = Battle { units: vec![unit(2, Hex::new(6, 5), 7), unit(1, Hex::new(4, 4), 3)] };
        b.units[1].hits = 0;
        let Flight::Moved(path) = flee(&b, &map(), 0, 1) else { panic!() };
        assert!(path[0].distance(Hex::new(4, 4)) > 1);
    }
}
