//! Movement: terrain costs and the hexes a unit can reach (see
//! `docs/formats.md`, "Movement").

use crate::battle::{Battle, UnitState};
#[cfg(test)]
use crate::battle::Unit;
use crate::hex::Hex;
use gboh_formats::map::{Map, Side};
use std::collections::{BinaryHeap, HashMap};

/// Movement classes of combat units, with the original's class codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveClass {
    Phalanx = 0,
    HeavyInfantry = 2,
    MediumInfantry = 3,
    /// Macedonian and Greek light infantry.
    LightInfantryMG = 5,
    LightInfantry = 6,
    /// Javelinists, archers and slingers.
    Skirmisher = 7,
    HeavyCavalry = 8,
    Lancer = 11,
    LightCavalry = 12,
    Elephant = 13,
    Chariot = 14,
    Oxybeles = 15,
}

impl MoveClass {
    /// The class of unit type `ty`; `None` for leaders, which move by
    /// rules not ported yet.
    pub fn of_type(ty: usize) -> Option<Self> {
        use MoveClass::*;
        Some(match ty {
            17 => Phalanx,
            1 | 3 | 18 | 31 => HeavyInfantry,
            11 | 33 => MediumInfantry,
            20 | 21 => LightInfantryMG,
            14 | 32 | 36 | 37 | 43 => LightInfantry,
            2 | 10 | 13 | 15 | 29 | 30 | 42 => Skirmisher,
            8 | 12 | 41 | 44 => HeavyCavalry,
            9 | 27 | 28 => Lancer,
            16 | 19 | 25 | 26 | 34 | 39 => LightCavalry,
            24 | 40 => Elephant,
            22 | 23 => Chariot,
            38 => Oxybeles,
            _ => return None,
        })
    }

    fn horse(self) -> bool {
        matches!(self, MoveClass::HeavyCavalry | MoveClass::Lancer | MoveClass::LightCavalry)
    }

    /// The steepest climb (in levels, elevation 0 counted as 1) the class
    /// can make in one step.
    fn max_climb(self) -> i32 {
        match self {
            MoveClass::Skirmisher => 3,
            c if c.horse() || matches!(c, MoveClass::Elephant | MoveClass::Chariot) => 1,
            _ => 2,
        }
    }
}

/// Map hex codes and side codes that matter for movement.
const STREAM: u16 = 6;
const SIDE_11: u16 = 11;
const IMPASSABLE_SIDE: u16 = 20;

/// Cost of hex terrain types 0..=5 and 10 when entering a hex that has
/// them (map hex codes; only 1 woods and 2 rocky occur in the shipped maps,
/// so rocky ground costs no extra).
fn hex_cost(code: u16) -> i32 {
    match code {
        0 | 2 | 4 => 1,
        1 | 3 | 5 | 10 => 2,
        _ => 0,
    }
}

/// The six directions in `Hex::neighbours` order, as map sides.
const SIDES: [Side; 6] = Side::ALL;

/// The two directions (indices into [`SIDES`]) a unit with scenario facing
/// `facing` may move in.
pub fn forward(facing: u8) -> Option<[usize; 2]> {
    Some(match facing {
        2 => [1, 0],
        3 => [1, 2],
        4 => [2, 3],
        6 => [3, 4],
        7 => [4, 5],
        8 => [5, 0],
        _ => return None,
    })
}

pub(crate) fn on_map(map: &Map, h: Hex) -> bool {
    h.x >= 0 && h.y >= 0 && (h.x as usize) < map.width && (h.y as usize) < map.height
}

fn elevation(map: &Map, h: Hex) -> i32 {
    map.elevation_at(h.x as usize, h.y as usize) as i32
}

/// Movement points to step from `from` into its neighbour in direction
/// `dir` for a unit of class `class`, or `None` if the terrain forbids it.
/// A leader's step (the original's `FUN_005655da`): 1 + the height
/// difference, impossible across an impassable side or up or down two
/// levels or more; the terrain does not matter.
pub fn leader_step_cost(map: &Map, from: Hex, dir: usize) -> Option<i32> {
    let to = from.neighbours()[dir];
    if !on_map(map, to) || map.side_features(from.x as usize, from.y as usize, SIDES[dir]).contains(&IMPASSABLE_SIDE) {
        return None;
    }
    let d = (elevation(map, from).max(1) - elevation(map, to).max(1)).abs();
    (d < 2).then_some(d + 1)
}

/// Where a leader can end a move with `points` (the original's
/// `FUN_00561d02`): any of the six directions, never into a hex holding an
/// enemy or next to one, ending only in an empty hex (or staying put).
pub fn leader_moves(battle: &Battle, map: &Map, leader: usize, points: i32) -> Vec<Hex> {
    let l = &battle.units[leader];
    let enemy_at = |h: Hex| battle.units.iter().any(|u| u.side != l.side && u.on_field() && u.hex == h);
    let blocked = |h: Hex| enemy_at(h) || h.neighbours().into_iter().any(enemy_at);
    let mut best: HashMap<Hex, i32> = HashMap::from([(l.hex, 0)]);
    let mut heap = BinaryHeap::from([(0i32, l.hex.x, l.hex.y)]);
    while let Some((neg, x, y)) = heap.pop() {
        let (spent, here) = (-neg, Hex::new(x, y));
        if best.get(&here).is_some_and(|&b| b < spent) {
            continue;
        }
        for d in 0..6 {
            let Some(c) = leader_step_cost(map, here, d) else { continue };
            let to = here.neighbours()[d];
            let total = spent + c;
            if total > points || blocked(to) || best.get(&to).is_some_and(|&b| b <= total) {
                continue;
            }
            best.insert(to, total);
            heap.push((-total, to.x, to.y));
        }
    }
    let empty = |h: &Hex| battle.units.iter().enumerate().all(|(i, u)| i == leader || u.state == UnitState::Gone || u.hex != *h);
    let mut out: Vec<Hex> = best.into_keys().filter(|h| *h != l.hex && empty(h)).collect();
    // in a fixed order: whoever picks among them picks the same every time
    out.sort_by_key(|h| (h.x, h.y));
    out.push(l.hex);
    out
}

pub fn step_cost(map: &Map, class: MoveClass, from: Hex, dir: usize) -> Option<i32> {
    let to = from.neighbours()[dir];
    if !on_map(map, to) {
        return None;
    }
    let side = map.side_features(from.x as usize, from.y as usize, SIDES[dir]);
    if side.contains(&IMPASSABLE_SIDE) {
        return None;
    }
    let codes = map.hex_features(to.x as usize, to.y as usize);
    if class == MoveClass::Chariot && codes.iter().any(|&c| (1..=5).contains(&c)) {
        return None;
    }
    if class == MoveClass::Elephant && codes.contains(&3) {
        return None;
    }
    let (z0, z1) = (elevation(map, from), elevation(map, to));
    if (z0.max(1) - z1.max(1)).abs() > class.max_climb() {
        return None;
    }
    let mut cost = 1;
    for &c in codes {
        cost = cost.max(hex_cost(c));
    }
    if side.contains(&SIDE_11) {
        cost = cost.max(99);
    }
    let extra = if side.contains(&STREAM) { 1 } else { 0 };
    Some(cost + extra + (z0 - z1).abs())
}

/// The hexes `unit` holds in its zone of control (the original's set at
/// `+0xe6`, `FUN_0057fd28`/`FUN_0057fd86`): the two front hexes of each of
/// its hexes that it could step into itself. Leaders, routed and
/// eliminated units exert none.
pub fn zoc_of(battle: &Battle, map: &Map, unit: usize) -> Vec<Hex> {
    let u = &battle.units[unit];
    let (Some(class), Some(dirs)) = (MoveClass::of_type(u.type_id), forward(u.facing)) else { return Vec::new() };
    if u.leader.is_some() || u.state != UnitState::Fighting {
        return Vec::new();
    }
    let mut out = Vec::new();
    for h in battle.hexes(unit) {
        for &d in &dirs {
            let n = h.neighbours()[d];
            if step_cost(map, class, h, d).is_some() && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// Whether `hex` is in an enemy zone of control for `side`
/// (`FUN_004b4287`): an enemy next to it holds it in its zone. A hex
/// where a skirmisher stands never is.
pub(crate) fn in_enemy_zoc(battle: &Battle, map: &Map, side: u8, hex: Hex) -> bool {
    let skirmisher_there = battle
        .units
        .iter()
        .enumerate()
        .any(|(i, u)| u.on_field() && battle.hexes(i).contains(&hex) && MoveClass::of_type(u.type_id) == Some(MoveClass::Skirmisher));
    if skirmisher_there {
        return false;
    }
    let near = hex.neighbours();
    (0..battle.units.len()).any(|e| {
        let o = &battle.units[e];
        o.side != side && o.state == UnitState::Fighting && battle.hexes(e).iter().any(|h| near.contains(h)) && zoc_of(battle, map, e).contains(&hex)
    })
}

/// Whether horses may not enter `hex` (`FUN_00571b5e`): it is in an enemy
/// zone of control and an enemy elephant next to it holds it.
fn elephant_bars(battle: &Battle, map: &Map, side: u8, hex: Hex) -> bool {
    let near = hex.neighbours();
    in_enemy_zoc(battle, map, side, hex)
        && (0..battle.units.len()).any(|e| {
            let o = &battle.units[e];
            o.side != side && near.contains(&o.hex) && MoveClass::of_type(o.type_id) == Some(MoveClass::Elephant) && zoc_of(battle, map, e).contains(&hex)
        })
}

/// Whether `mover` may end a step on `hex`: free, or shared only with a
/// friendly leader (the second hexes of two-hex units count as occupied).
fn can_enter(battle: &Battle, mover: usize, hex: Hex) -> bool {
    let side = battle.units[mover].side;
    battle.units.iter().enumerate().all(|(i, u)| {
        i == mover
            || u.state == UnitState::Gone
            || !battle.hexes(i).contains(&hex)
            || (u.leader.is_some() && u.side == side)
    })
}

/// Hexes the unit at index `unit` can reach with `points` movement points,
/// with the points each costs. It moves only into the two hexes in front
/// (its facing does not change), stops on entering an enemy zone of
/// control, and horses will not step where an enemy elephant holds the
/// zone.
/// A two-hex unit moves both hexes the same way; both steps must be legal,
/// the cost is the main hex's, and the result lists the positions of its
/// main hex (see [`highlighted`] for both).
pub fn reachable(battle: &Battle, map: &Map, unit: usize, points: i32) -> HashMap<Hex, i32> {
    search(battle, map, unit, points).0
}

/// The cheapest path from the unit's hex to `to` (excluding its start), if
/// reachable.
pub fn path_to(battle: &Battle, map: &Map, unit: usize, points: i32, to: Hex) -> Option<Vec<Hex>> {
    let (costs, parent) = search(battle, map, unit, points);
    costs.get(&to)?;
    let mut path = vec![to];
    let mut h = to;
    while let Some(&p) = parent.get(&h) {
        if p == battle.units[unit].hex {
            break;
        }
        path.push(p);
        h = p;
    }
    path.reverse();
    Some(path)
}

type Search = (HashMap<Hex, i32>, HashMap<Hex, Hex>);

fn search(battle: &Battle, map: &Map, unit: usize, points: i32) -> Search {
    let u = &battle.units[unit];
    let mut out = HashMap::new();
    let mut parent = HashMap::new();
    let (Some(class), Some(dirs)) = (MoveClass::of_type(u.type_id), forward(u.facing)) else { return (out, parent) };
    let pair = |h: Hex| if u.two_hex() { crate::battle::flank(h, u.facing) } else { None };
    // no step out of an enemy zone (`FUN_0056e7d7`), but from where the
    // unit's order began (it starts with full points); a two-hex unit whose
    // second hex is in a zone cannot step at all
    let start_free = u.mp >= u.max_mp;
    let stuck = |h: Hex| {
        let zoc = in_enemy_zoc(battle, map, u.side, h) && !(h == u.hex && start_free);
        zoc || pair(h).is_some_and(|p| in_enemy_zoc(battle, map, u.side, p))
    };
    // Dijkstra on spent points (max-heap of negated cost)
    let mut heap = BinaryHeap::from([(0i32, u.hex.x, u.hex.y)]);
    let mut best: HashMap<Hex, i32> = HashMap::from([(u.hex, 0)]);
    while let Some((neg, x, y)) = heap.pop() {
        let (spent, here) = (-neg, Hex::new(x, y));
        if best.get(&here).is_some_and(|&b| b < spent) {
            continue;
        }
        if here != u.hex {
            out.insert(here, spent);
        }
        if stuck(here) {
            continue;
        }
        for &d in &dirs {
            let Some(c) = step_cost(map, class, here, d) else { continue };
            let to = here.neighbours()[d];
            let total = spent + c;
            if total > points || !can_enter(battle, unit, to) {
                continue;
            }
            if let Some(p) = pair(here) {
                let Some(pt) = pair(to) else { continue };
                if step_cost(map, class, p, d).is_none() || !can_enter(battle, unit, pt) {
                    continue;
                }
            }
            if class.horse() && elephant_bars(battle, map, u.side, to) {
                continue;
            }
            if best.get(&to).is_none_or(|&b| total < b) {
                best.insert(to, total);
                parent.insert(to, here);
                heap.push((-total, to.x, to.y));
            }
        }
    }
    (out, parent)
}

/// Hexes within `range` forward steps of `from` for a unit facing
/// `facing`: its missile arc (the manual: range extends from the frontal
/// hexes).
pub fn arc(from: Hex, facing: u8, range: i32) -> Vec<Hex> {
    let Some(dirs) = forward(facing) else { return Vec::new() };
    let mut out = Vec::new();
    let mut ring = vec![from];
    for _ in 0..range {
        let mut next = Vec::new();
        for h in ring {
            for &d in &dirs {
                let n = h.neighbours()[d];
                if !out.contains(&n) {
                    out.push(n);
                    next.push(n);
                }
            }
        }
        ring = next;
    }
    out
}

/// The hexes the game highlights for `unit`: every reachable position,
/// with the second hex too for two-hex units.
pub fn highlighted(battle: &Battle, map: &Map, unit: usize, points: i32) -> Vec<Hex> {
    let u = &battle.units[unit];
    let mut out: Vec<Hex> = Vec::new();
    for h in reachable(battle, map, unit, points).into_keys() {
        out.push(h);
        if let Some(p) = u.two_hex().then(|| crate::battle::flank(h, u.facing)).flatten() {
            out.push(p);
        }
    }
    out.retain(|&h| h != u.hex && Some(h) != u.second_hex());
    out.sort_by_key(|h| (h.x, h.y));
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_map(w: usize, h: usize) -> Map {
        Map {
            name: String::new(),
            width: w,
            height: h,
            hex_id_origin: 1000,
            unk_b: 1,
            elevation: vec![1; w * h],
            cells: vec![Vec::new(); (2 * w + 2) * (4 * h + 1)],
        }
    }

    fn unit(type_id: usize, side: u8, hex: Hex, facing: u8) -> Unit {
        Unit {
            source: 0,
            name: String::new(),
            type_id,
            nation: side,
            side,
            group: 0,
            hex,
            facing,
            leader: None,
            retreat: 7,
            max_mp: 6,
            mp: 6,
            turned: false,
            size: 6,
            tq: 6,
            hits: 0,
            state: crate::battle::UnitState::Fighting,
            missile: None,
            fired: false,
            wounded: false,
            reordered: false,
            reorder_hit: false,
            done: false,
        }
    }

    #[test]
    fn leaders_keep_away_from_enemies() {
        let map = flat_map(12, 12);
        let mut l = unit(4, 2, Hex::new(3, 5), 3);
        l.leader = Some(crate::battle::LeaderRatings { charisma: 1, command_range: 4, initiative: 3, personal_combat: 1 });
        let battle = Battle { units: vec![l, unit(31, 1, Hex::new(6, 5), 7), unit(31, 2, Hex::new(3, 4), 3)] };
        let m = leader_moves(&battle, &map, 0, 2);
        // any direction, two steps, but not next to the enemy nor onto a friend
        assert!(m.contains(&Hex::new(1, 5)) && m.contains(&Hex::new(3, 7)));
        assert!(!m.contains(&Hex::new(5, 5)) && !m.contains(&Hex::new(3, 4)));
        assert!(m.contains(&Hex::new(3, 5)));
    }

    #[test]
    fn moves_only_forward() {
        let map = flat_map(10, 10);
        let battle = Battle { units: vec![unit(31, 2, Hex::new(4, 4), 3)] };
        let r = reachable(&battle, &map, 0, 1);
        // facing 3: the NE and SE neighbours
        let n = Hex::new(4, 4).neighbours();
        let mut got: Vec<_> = r.keys().copied().collect();
        got.sort_by_key(|h| (h.x, h.y));
        let mut want = vec![n[1], n[2]];
        want.sort_by_key(|h| (h.x, h.y));
        assert_eq!(got, want);
    }

    #[test]
    fn stops_next_to_enemy() {
        let map = flat_map(12, 10);
        // facing 4 moves SE and S; an enemy at (3, 5) facing them (8: NW
        // and N) holds both first steps in its zone of control
        let start = Hex::new(2, 4);
        let battle = Battle { units: vec![unit(31, 2, start, 4), unit(31, 1, Hex::new(3, 5), 8)] };
        let zone = zoc_of(&battle, &map, 1);
        let r = reachable(&battle, &map, 0, 6);
        let mut got: Vec<_> = r.keys().copied().collect();
        got.sort_by_key(|h| (h.x, h.y));
        assert!(got.iter().all(|h| zone.contains(h)), "{got:?} {zone:?}");
        assert!(!got.is_empty());
        // facing away, it holds nothing the unit walks through
        let battle = Battle { units: vec![unit(31, 2, start, 4), unit(31, 1, Hex::new(3, 5), 4)] };
        assert!(reachable(&battle, &map, 0, 6).len() > 2);
        // a skirmisher stopped in a zone moves on from where it stands
        let mut battle = Battle { units: vec![unit(2, 2, start, 4), unit(31, 1, Hex::new(3, 5), 8)] };
        let first = *got.first().unwrap();
        battle.units[0].hex = first;
        battle.units[0].mp = 3;
        assert!(!reachable(&battle, &map, 0, 3).is_empty());
        let mut battle = Battle { units: vec![unit(31, 2, first, 4), unit(31, 1, Hex::new(3, 5), 8)] };
        battle.units[0].mp = 3;
        assert!(reachable(&battle, &map, 0, 3).is_empty(), "others stay once stopped");
    }

    #[test]
    fn woods_cost_two() {
        let mut map = flat_map(10, 10);
        let target = Hex::new(4, 4).neighbours()[1];
        let (gx, gy) = Map::hex_cell(target.x as usize, target.y as usize);
        let gw = map.grid_width();
        map.cells[gy as usize * gw + gx as usize] = vec![1]; // woods
        let class = MoveClass::HeavyInfantry;
        assert_eq!(step_cost(&map, class, Hex::new(4, 4), 1), Some(2));
        assert_eq!(step_cost(&map, MoveClass::Chariot, Hex::new(4, 4), 1), None);
    }
}
