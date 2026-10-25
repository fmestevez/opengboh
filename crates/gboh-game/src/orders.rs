//! Orders: what an activated leader's units do, and what it costs him.

use crate::battle::UnitState;
use crate::battle::{Battle, flank, neighbour};
use crate::hex::Hex;
use crate::missile::Volley;
use crate::movement::{MoveClass, arc, path_to, reachable};
use gboh_formats::crt::BorlandRand;
use gboh_formats::map::Map;

/// Hannibal's map code for a tower (Baecula's camp).
const TOWER: u16 = 14;

/// Why an order was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    NoMovementPoints,
    Unreachable,
    /// The turn would put a two-hex unit's hex off the map or on another
    /// unit.
    Blocked,
    /// Routed units take no order but a rally.
    Routed,
    /// The active leader may give no orders (but designate shocks): by
    /// Alexander's manual, a rule option, he began in an enemy's zone.
    Held,
}

/// Facing after one 60° turn: left is anticlockwise (3 → 2 → 8 → 7 → 6 →
/// 4 → 3), right the reverse. Facings 1 and 5 are skipped.
pub fn turned(facing: u8, left: bool) -> u8 {
    const ORDER: [u8; 6] = [2, 3, 4, 6, 7, 8];
    let i = ORDER.iter().position(|&f| f == facing).unwrap_or(0);
    ORDER[if left { (i + 5) % 6 } else { (i + 1) % 6 }]
}

/// Turn `unit` 60° left or right. It costs one movement point (skirmishers
/// pay only for their first turn of the turn), and chariots take two
/// cohesion hits. (The manual's two points for elephants, chariots and
/// cataphracts is not in the game's code.) A two-hex unit pivots: on a
/// left turn its main hex stays and the second hex swings, on a right turn
/// the second hex stays and the main hex swings.
/// Movement points a turn costs: 1, or by the manuals (an option) 2 for
/// elephants and chariots, Alexander's cataphracts and Caesar's heavy
/// cavalry too.
fn turn_cost(battle: &Battle, unit: usize) -> i32 {
    let ty = battle.units[unit].type_id;
    let heavy = match MoveClass::of_type(ty) {
        Some(MoveClass::Elephant | MoveClass::Chariot) => true,
        Some(MoveClass::HeavyCavalry) if battle.caesar() => true,
        _ => !battle.hannibal() && gboh_formats::units::cataphract(ty),
    };
    if battle.options.manual_turning && heavy { 2 } else { 1 }
}

pub fn turn(battle: &mut Battle, map: &Map, unit: usize, left: bool) -> Result<(), Refused> {
    if battle.units[unit].state == UnitState::Routed {
        return Err(Refused::Routed);
    }
    let u = &battle.units[unit];
    let free = u.turned && MoveClass::of_type(u.type_id) == Some(MoveClass::Skirmisher);
    if !free && u.mp < turn_cost(battle, unit) {
        return Err(Refused::NoMovementPoints);
    }
    if ordered_again(battle, unit) {
        return Err(Refused::Routed);
    }
    let u = &battle.units[unit];
    let facing = turned(u.facing, left);
    let main = match (u.second_hex(), left) {
        (Some(second), false) => {
            // the main hex is the one whose flank is the second hex
            let away = (facing as usize + 5) % 8 + 1;
            neighbour(second, away).ok_or(Refused::Blocked)?
        }
        _ => u.hex,
    };
    if u.two_hex() {
        let second = flank(main, facing).ok_or(Refused::Blocked)?;
        for h in [main, second] {
            let on_map = h.x >= 0 && h.y >= 0 && (h.x as usize) < map.width && (h.y as usize) < map.height;
            let taken = (0..battle.units.len()).any(|i| i != unit && battle.hexes(i).contains(&h));
            if !on_map || taken {
                return Err(Refused::Blocked);
            }
        }
    }
    let cost = turn_cost(battle, unit);
    let u = &mut battle.units[unit];
    if !free {
        u.mp -= cost;
    }
    if MoveClass::of_type(u.type_id) == Some(MoveClass::Chariot) {
        u.hits += 2;
    }
    u.turned = true;
    u.facing = facing;
    let old = u.hex;
    u.hex = main;
    // a two-hex unit pivoting pays for the ground as a step from its old
    // main hex to the new (`FUN_0052843b`; 🟡 the "from" hex is taken to be
    // the old main hex)
    if battle.units[unit].two_hex() {
        let hits = terrain_hits(battle, map, unit, old, main);
        let u = &mut battle.units[unit];
        u.hits += hits;
        if u.hits >= u.tq as i32 {
            u.state = UnitState::Routed;
        }
    }
    Ok(())
}

/// About face (unit slot 0xf4, `FUN_005719dc` → `FUN_005253f2`): three
/// movement points and the facing reversed (three turns the same way); a
/// two-hex unit's hexes swap, its second hex becoming its main. (🟡 when the
/// button is enabled is not traced: here, with three points left.)
pub fn about_face(battle: &mut Battle, unit: usize) -> Result<(), Refused> {
    let u = &battle.units[unit];
    if u.state == UnitState::Routed {
        return Err(Refused::Routed);
    }
    if u.mp < 3 {
        return Err(Refused::NoMovementPoints);
    }
    if ordered_again(battle, unit) {
        return Err(Refused::Routed);
    }
    let u = &battle.units[unit];
    let facing = turned(turned(turned(u.facing, false), false), false);
    let main = u.second_hex().unwrap_or(u.hex);
    let u = &mut battle.units[unit];
    u.mp -= 3;
    u.facing = facing;
    u.hex = main;
    Ok(())
}

/// What a move brought about besides the move: reaction fire (shooter,
/// hits), the units that withdrew from it, and enemy leaders hurt
/// withdrawing (they roll for a casualty).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Moved {
    pub fire: Vec<(usize, i32)>,
    pub withdrew: Vec<usize>,
    pub leaders_hit: Vec<usize>,
}

/// Move `unit` to `to` along the cheapest legal path (see [`reachable`]),
/// one hex at a time. Each hex entered next to an enemy missile unit that
/// has missiles and the hex in its arc draws a reaction volley (range 1);
/// a unit broken by it stops there, routed. Enemies next to each hex
/// entered may then withdraw (see [`withdraw`]); the mover is not stopped
/// by it. A unit already ordered this turn takes one cohesion hit for
/// being ordered again, once an activation.
pub fn move_to(battle: &mut Battle, map: &Map, unit: usize, to: Hex, rng: &mut BorlandRand) -> Result<Moved, Refused> {
    if battle.units[unit].state == UnitState::Routed {
        return Err(Refused::Routed);
    }
    let mp = battle.units[unit].mp;
    let path = path_to(battle, map, unit, mp, to).ok_or(Refused::Unreachable)?;
    if ordered_again(battle, unit) {
        return Ok(Moved::default());
    }
    let costs = reachable(battle, map, unit, mp);
    let mut moved = Moved::default();
    for h in path {
        let from = battle.units[unit].hex;
        battle.units[unit].mp = mp - costs[&h];
        battle.units[unit].hex = h;
        battle.units[unit].hits += terrain_hits(battle, map, unit, from, h);
        for s in reacting(battle, map, unit) {
            let class = MoveClass::of_type(battle.units[unit].type_id).unwrap_or(MoveClass::HeavyInfantry);
            let Some((weapon, left)) = battle.units[s].missile else { continue };
            battle.units[s].missile = Some((weapon, left - 1));
            let cat = gboh_formats::units::cataphract(battle.units[unit].type_id);
            // (Hannibal's die on `random()`)
            let roll = if battle.hannibal() { rng.random() } else { rng.next() } % 10;
            let r = Volley::reaction(weapon, class, cat).resolve(roll);
            battle.units[unit].hits += r.hits;
            moved.fire.push((s, r.hits));
        }
        let u = &battle.units[unit];
        if u.hits >= u.tq as i32 {
            battle.units[unit].state = UnitState::Routed;
            break;
        }
        let (units, leaders) = withdrawers(battle, map, unit);
        for e in units {
            if withdraw(battle, map, e, unit, rng) {
                moved.withdrew.push(e);
            }
        }
        for l in leaders {
            if leader_withdraw(battle, map, l, unit, rng) {
                moved.leaders_hit.push(l);
            }
        }
    }
    Ok(moved)
}

/// Cohesion hits for one step of a move (`FUN_0056ef04`, unit slot 0x3c,
/// called for every step by `FUN_00522d69`): by the unit's class group
/// (infantry and oxybeles, skirmishers, horse, elephants, chariots), for
/// each terrain of the hex entered (hex codes 1–5 and 10: woods, rocky
/// ground, marsh, major rivers, abatis) and of the side crossed (6 minor
/// river, 8 steep-banked stream, 9 steep bank, 11), and for climbing or
/// descending; horses take 2 more for stepping next to an enemy elephant
/// ("… moved next to an elephant for 2 hits"). The table is the
/// original's (`0x63bae8`).
pub fn terrain_hits(battle: &Battle, map: &Map, unit: usize, from: Hex, to: Hex) -> i32 {
    // rows: terrain codes 0–11, then up 1, down 1, up 2, down 2, 3 levels
    const TABLE: [[i32; 5]; 17] = [
        [0, 0, 0, 0, 0],
        [1, 0, 1, 1, 99],
        [1, 0, 2, 1, 99],
        [1, 1, 1, 99, 99],
        [1, 1, 1, 0, 99],
        [1, 1, 2, 99, 99],
        [1, 1, 1, 0, 99],
        [1, 0, 0, 0, 1],
        [2, 1, 2, 1, 99],
        [1, 0, 1, 0, 99],
        [1, 1, 1, 99, 99],
        [99, 99, 99, 99, 99],
        [1, 1, 1, 0, 1],
        [1, 0, 0, 0, 1],
        [2, 2, 99, 99, 99],
        [2, 1, 99, 99, 99],
        [99, 3, 99, 99, 99],
    ];
    use MoveClass::*;
    let u = &battle.units[unit];
    let Some(class) = MoveClass::of_type(u.type_id) else { return 0 };
    let group = match class {
        Skirmisher => 1,
        HeavyCavalry | RomanCavalry | BarbarianCavalry | Lancer | LightCavalry => 2,
        Elephant => 3,
        Chariot => 4,
        _ => 0,
    };
    let on_map = |h: Hex| h.x >= 0 && h.y >= 0 && (h.x as usize) < map.width && (h.y as usize) < map.height;
    if !on_map(from) || !on_map(to) {
        return 0;
    }
    let mut hits = 0;
    let hex_codes = map.hex_features(to.x as usize, to.y as usize);
    let side = from.neighbours().iter().position(|&n| n == to).map(|d| gboh_formats::map::Side::ALL[d]);
    let side_codes = side.map_or(&[][..], |s| map.side_features(from.x as usize, from.y as usize, s));
    if map.caesar {
        // Caesar's table (`FUN_0057d32f`, `0x64ce08`): woods, rough ground,
        // marsh, a ditch (and a hit more onto a berm from off it), a stream,
        // the lowest ground
        const CAESAR: [(u16, [i32; 5]); 6] =
            [(1, [1, 1, 1, 0, 99]), (2, [1, 0, 1, 0, 99]), (3, [1, 1, 1, 99, 99]), (4, [1, 1, 1, 1, 99]), (5, [1, 1, 1, 99, 99]), (6, [1, 1, 1, 1, 99])];
        const LOWEST: [i32; 5] = [1, 1, 1, 99, 99];
        let row = |k: u16| CAESAR.iter().find(|r| r.0 == k).map_or(0, |r| r.1[group]);
        let add = |h: i32| if h < 99 { h } else { 0 };
        for k in [1, 2, 3, 5] {
            if hex_codes.contains(&k) {
                hits += add(row(k));
            }
        }
        if side_codes.contains(&10) {
            hits += add(row(4));
            if hex_codes.contains(&4) && !map.hex_features(from.x as usize, from.y as usize).contains(&4) {
                hits += 1;
            }
        }
        if side_codes.contains(&6) {
            hits += add(row(6));
        }
        if map.elevation_at(to.x as usize, to.y as usize) == 0 {
            hits += add(LOWEST[group]);
        }
    }
    for k in (0..15u16).filter(|_| !map.caesar) {
        let present = match k {
            0..=5 | 10 => hex_codes.contains(&k),
            6 | 8 | 9 | 11 => side_codes.contains(&k),
            _ => false,
        };
        if present {
            let h = TABLE[k as usize][group];
            if h < 99 {
                hits += h;
            }
        }
    }
    let level = |h: Hex| (map.elevation_at(h.x as usize, h.y as usize) as i32).max(1);
    let row = match level(from) - level(to) {
        -1 => Some(12),
        1 => Some(13),
        -2 => Some(14),
        2 => Some(15),
        3 | -3 => Some(16),
        _ => None,
    };
    if let Some(r) = row
        && TABLE[r][group] < 99
    {
        hits += TABLE[r][group];
    }
    if group == 2 {
        let near = to.neighbours();
        let elephant = battle
            .units
            .iter()
            .any(|o| o.side != u.side && o.state == UnitState::Fighting && near.contains(&o.hex) && MoveClass::of_type(o.type_id) == Some(Elephant));
        if elephant {
            hits += 2;
        }
    }
    hits
}

/// The cohesion hit for being ordered again (`FUN_00538e7c` and the
/// turning and firing orders): a unit already ordered this turn takes one,
/// the first time it acts in an activation. Returns whether it broke (the
/// order is then not carried out).
pub fn ordered_again(battle: &mut Battle, unit: usize) -> bool {
    let u = &mut battle.units[unit];
    if !u.reordered || u.reorder_hit {
        return false;
    }
    u.reorder_hit = true;
    u.hits += 1;
    if u.hits >= u.tq as i32 {
        u.state = UnitState::Routed;
        return true;
    }
    false
}

/// A TQ check (`FUN_0057e846`): passed when `roll` is at most the unit's
/// TQ (1 when routed).
fn tq_passes(u: &crate::battle::Unit, roll: u32) -> bool {
    let tq = if u.state == UnitState::Routed { 1 } else { u.tq as u32 };
    roll <= tq
}

/// Enemy units next to `mover` that may withdraw from it
/// (`FUN_00577e3c`), in the order of the hexes around it; and enemy
/// leaders standing alone next to it. Light and heavy cavalry withdraw
/// from anything, lancers from all but horse, skirmishers from heavy
/// infantry and phalanxes; not while another enemy of theirs holds them
/// in its zone of control.
fn withdrawers(battle: &Battle, map: &Map, mover: usize) -> (Vec<usize>, Vec<usize>) {
    use MoveClass::*;
    let m = &battle.units[mover];
    let m_class = MoveClass::of_type(m.type_id);
    let m_horse = m_class.is_some_and(MoveClass::horse);
    let hannibal = battle.hannibal();
    let (mut units, mut leaders) = (Vec::new(), Vec::new());
    let near: Vec<Hex> = battle.hexes(mover).iter().flat_map(|h| h.neighbours()).collect();
    for n in near {
        for e in 0..battle.units.len() {
            let o = &battle.units[e];
            if o.side == m.side || !o.on_field() || o.hex != n || units.contains(&e) || leaders.contains(&e) {
                continue;
            }
            if o.leader.is_some() {
                let alone = battle.units.iter().enumerate().all(|(i, x)| i == e || !x.on_field() || x.hex != n);
                if alone && o.state == UnitState::Fighting {
                    leaders.push(e);
                }
                continue;
            }
            if o.state != UnitState::Fighting {
                continue;
            }
            let eligible = match MoveClass::of_type(o.type_id) {
                // Hannibal's: horse withdraw only before foot
                Some(HeavyCavalry | RomanCavalry | BarbarianCavalry | LightCavalry | Lancer) if hannibal => !m_horse,
                Some(HeavyCavalry | LightCavalry) => true,
                Some(Lancer) => !m_horse,
                Some(Skirmisher) => matches!(m_class, Some(HeavyInfantry | Phalanx)),
                _ => false,
            };
            let held = (0..battle.units.len()).any(|x| {
                let other = &battle.units[x];
                x != mover
                    && other.side == m.side
                    && other.state == UnitState::Fighting
                    && other.hex.distance(o.hex) == 1
                    && crate::movement::zoc_of(battle, map, x).contains(&o.hex)
            });
            if eligible && !held {
                units.push(e);
            }
        }
    }
    (units, leaders)
}

/// The three sides (indices into `Hex::neighbours`) facing away from side
/// `d`: the opposite and its two neighbours.
fn away(d: usize) -> [usize; 3] {
    [(d + 2) % 6, (d + 3) % 6, (d + 4) % 6]
}

/// Orderly withdrawal of `unit` from `mover` (`FUN_00578783`), automatic:
/// cavalry an elephant approaches first takes `max(1, max(0, r − TQ) −
/// TQ)` hits (`r = rand() % 10`); a unit with `TQ − 2` hits or more cannot
/// withdraw; then a TQ check (`rand() % 10 ≤ TQ`). It moves with 2
/// movement points, stepping only away from the approacher, through empty
/// hexes, zones of control ignored (`FUN_00524ce4`), to the hex reached
/// farthest from the nearest enemy (up to 3; the later on a tie). It
/// fails from beside an enemy leader, or where it cannot move. After the
/// move: one cohesion hit if approached from the flank or rear (skirmishers
/// only the rear), and one more for a failed TQ check (`FUN_005778f8`).
/// (🟡 the original also fails if the approacher's hex is of terrain type 4
/// or 5; taken here as the map's hex codes 4 and 5.)
pub fn withdraw(battle: &mut Battle, map: &Map, unit: usize, mover: usize, rng: &mut BorlandRand) -> bool {
    let u = &battle.units[unit];
    let class = MoveClass::of_type(u.type_id).unwrap_or(MoveClass::HeavyInfantry);
    let tq = u.tq as i32;
    if class.horse() && MoveClass::of_type(battle.units[mover].type_id) == Some(MoveClass::Elephant) {
        let r = (rng.next() % 10) as i32;
        let hits = ((r - tq).max(0) - tq).max(1);
        battle.units[unit].hits += hits;
        if battle.units[unit].hits >= tq {
            battle.units[unit].state = UnitState::Routed;
            return false;
        }
    }
    if battle.units[unit].hits >= tq - 2 || !tq_passes(&battle.units[unit], rng.next() % 10) {
        return false;
    }
    let u = &battle.units[unit];
    let from_hexes = battle.hexes(mover);
    let Some(d) = from_hexes.iter().find_map(|&m| u.hex.neighbours().iter().position(|&n| n == m)) else { return false };
    let mover_hex = battle.units[mover].hex;
    if map.hex_features(mover_hex.x.max(0) as usize, mover_hex.y.max(0) as usize).iter().any(|&c| c == 4 || c == 5) {
        return false;
    }
    let dirs = away(d);
    let occupied = |h: Hex| battle.units.iter().enumerate().any(|(i, o)| i != unit && o.on_field() && battle.hexes(i).contains(&h));
    // the hexes reachable with 2 points, in the order found
    let mut found: Vec<Hex> = Vec::new();
    let mut frontier = vec![(u.hex, 0)];
    while let Some((here, spent)) = frontier.first().copied() {
        frontier.remove(0);
        for &dir in &dirs {
            let Some(c) = crate::movement::step_cost(map, class, here, dir) else { continue };
            let to = here.neighbours()[dir];
            if spent + c > 2 || occupied(to) || to == u.hex || found.contains(&to) {
                continue;
            }
            found.push(to);
            frontier.push((to, spent + c));
        }
    }
    let side = u.side;
    let enemies: Vec<usize> = (0..battle.units.len()).filter(|&i| battle.units[i].side != side && battle.units[i].on_field()).collect();
    let mut best: Option<(i32, Hex, bool)> = None;
    for &h in &found {
        let nearest = enemies.iter().map(|&e| battle.units[e].hex.distance(h)).min().unwrap_or(3).min(3);
        let leader_near = enemies.iter().any(|&e| battle.units[e].leader.is_some() && battle.units[e].hex.distance(h) == nearest);
        if best.is_none_or(|(b, _, _)| nearest >= b) {
            best = Some((nearest, h, leader_near));
        }
    }
    let Some((score, to, leader_near)) = best else { return false };
    if score == 1 && leader_near {
        return false;
    }
    let angle = crate::shock::angle(u.hex, u.facing, mover_hex);
    let skirmisher = class == MoveClass::Skirmisher;
    let mut hits = if angle == 2 || (angle == 1 && !skirmisher) { 1 } else { 0 };
    battle.units[unit].hex = to;
    if !tq_passes(&battle.units[unit], rng.next() % 10) {
        hits += 1;
    }
    let u = &mut battle.units[unit];
    u.hits += hits;
    if u.hits >= u.tq as i32 {
        u.state = UnitState::Routed;
    }
    true
}

/// An enemy leader standing alone next to `mover` steps away from it
/// (`FUN_00567aaf`), at a risk: 90 in 100 from cavalry, 60 from other
/// combat units (oxybeles none), rolled with `random() % 100`. Returns
/// whether he must roll for a casualty. (🟡 the original takes the highest
/// risk along his way; the rewrite the approacher's.)
pub(crate) fn leader_withdraw(battle: &mut Battle, map: &Map, leader: usize, mover: usize, rng: &mut BorlandRand) -> bool {
    let from = battle.units[mover].hex;
    let points = battle.units[leader].max_mp.max(1);
    let to = crate::movement::leader_moves(battle, map, leader, points).into_iter().max_by_key(|h| h.distance(from));
    if let Some(to) = to {
        battle.units[leader].hex = to;
    }
    let risk = match MoveClass::of_type(battle.units[mover].type_id) {
        // (the manuals: 5 in 100; an option)
        Some(MoveClass::Oxybeles) | None => 0,
        Some(_) if battle.options.manual_command => 5,
        Some(c) if c.horse() => 90,
        Some(_) => 60,
    };
    (rng.random() % 100) < risk
}

/// Enemy missile units that react to `unit` standing where it is.
fn reacting(battle: &Battle, map: &Map, unit: usize) -> Vec<usize> {
    let u = &battle.units[unit];
    // Hannibal's: a unit on a tower (hex code 14) fires all round
    let tower = |s: &crate::battle::Unit| {
        battle.hannibal() && s.hex.x >= 0 && s.hex.y >= 0 && map.hex_features(s.hex.x as usize, s.hex.y as usize).contains(&TOWER) && s.hex.distance(u.hex) == 1
    };
    battle
        .units
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.side != u.side
                && s.state == UnitState::Fighting
                && s.missile.is_some_and(|(_, n)| n > 0)
                && (arc(s.hex, s.facing, 1).contains(&u.hex) || tower(s))
        })
        .map(|(i, _)| i)
        .collect()
}

/// Order `shooter` to volley at `target`: once per leader's orders, needs a
/// missile and the target in its arc within the weapon's range.
/// `cover`: a further modifier for the target's ground (the manual's woods,
/// an option).
pub fn volley(battle: &mut Battle, shooter: usize, target: usize, cover: i32, rng: &mut BorlandRand) -> Result<i32, Refused> {
    let s = &battle.units[shooter];
    let t = &battle.units[target];
    if s.state == UnitState::Routed {
        return Err(Refused::Routed);
    }
    let Some((weapon, left)) = s.missile else { return Err(Refused::Unreachable) };
    let range = s.hex.distance(t.hex);
    if s.fired || left == 0 || !arc(s.hex, s.facing, battle.missile_range(shooter)).contains(&t.hex) {
        return Err(Refused::Unreachable);
    }
    if MoveClass::of_type(s.type_id) != Some(MoveClass::Oxybeles) && ordered_again(battle, shooter) {
        return Err(Refused::Routed);
    }
    let (s, t) = (&battle.units[shooter], &battle.units[target]);
    let class = MoveClass::of_type(t.type_id).unwrap_or(MoveClass::HeavyInfantry);
    let v = Volley {
        weapon,
        range,
        target: class,
        from_front: arc(t.hex, t.facing, 1).contains(&s.hex),
        moved: s.mp < s.max_mp,
        target_cataphract: gboh_formats::units::cataphract(t.type_id),
        // (Caesar's: ballistas by their own table; a shooter that rallied
        // +1; the barbarians' bows −1, `FUN_00548d26` 🟡 its wiring of the
        // ballista and cataphract arguments read as meant)
        ballista: gboh_formats::units::title(s.type_id).is_some_and(|t| t.contains("Ballista")),
        bonus: s.rallied as i32 - (s.nation == 11 && weapon == crate::missile::Weapon::Bow) as i32 + cover,
    };
    let hits = v.resolve(rng.next() % 10).hits;
    battle.units[shooter].missile = Some((weapon, left - 1));
    battle.units[shooter].fired = true;
    let t = &mut battle.units[target];
    t.hits += hits;
    if t.hits >= t.tq as i32 {
        t.state = UnitState::Routed;
    }
    Ok(hits)
}

/// Chances (percent) that recovering removes 0, 1, 2 or 3 cohesion hits,
/// before the leader's adjustment.
const RECOVER: [i32; 4] = [5, 40, 50, 5];

/// Order a unit to recover cohesion (the original's code): the leader's
/// initiative moves `(initiative − 4) × 4` points of chance from removing
/// none to removing three (a negative "none" comes off "one"); the roll is
/// `rand() % 100`, 40 lower in an enemy zone of control. Returns the hits
/// removed. 🟡 The unit's two flags the original sets (likely "ordered" and
/// "may not move") are approximated by spending its movement.
pub fn recover(battle: &mut Battle, map: &Map, unit: usize, initiative: u16, rng: &mut BorlandRand) -> i32 {
    let shift = (initiative as i32 - 4) * 4;
    let mut w = RECOVER;
    w[0] -= shift;
    w[3] += shift;
    if w[0] < 0 {
        w[1] += w[0];
        w[0] = 0;
    }
    let mut roll = (rng.next() % 100) as i32;
    let u = &battle.units[unit];
    if crate::movement::in_enemy_zoc(battle, map, u.side, u.hex) {
        // (Hannibal's: 60)
        roll -= if battle.hannibal() { 60 } else { 40 };
    }
    let roll = roll.max(0);
    let mut total = 0;
    let removed = (0..4).find(|&i| {
        total += w[i];
        roll < total
    });
    let u = &mut battle.units[unit];
    let removed = removed.map_or(0, |i| (i as i32).min(u.hits));
    u.hits -= removed;
    u.mp = 0;
    removed
}

/// Missile replenishment at a leader's activation (`FUN_005672ba` →
/// `FUN_0056bac3`): each unit given that may take orders, is not routed and
/// is out of missiles gets `random() % 5` (javelins `random() % 2`) and is
/// done for the turn. Returns the units replenished.
pub fn resupply(battle: &mut Battle, units: &[usize], rng: &mut BorlandRand) -> Vec<usize> {
    use crate::missile::Weapon;
    let hannibal = battle.hannibal();
    let mut out = Vec::new();
    for &u in units {
        let o = &battle.units[u];
        if o.state != UnitState::Fighting || o.done || o.mp < o.max_mp {
            continue;
        }
        // (Hannibal's legions throw their one pilum and get no more)
        if MoveClass::of_type(o.type_id) == Some(MoveClass::Legion) {
            continue;
        }
        if let Some((w, 0)) = o.missile {
            // (the manual: up to half the supply, rounded up; an option)
            let n = if battle.options.manual_missiles {
                w.supply().div_ceil(2) as u32 + 1
            } else if matches!(w, Weapon::Javelin | Weapon::MountedJavelin) {
                2
            } else {
                5
            };
            let o = &mut battle.units[u];
            o.missile = Some((w, (rng.random() % n) as u16));
            o.done = true;
            // (in Hannibal's rules resupplying isn't an order)
            o.reordered = !hannibal;
            out.push(u);
        }
    }
    out
}

/// An activated leader's command points: `Com: left/max` in game, starting
/// at his initiative.
#[derive(Debug, Clone)]
pub struct CommandPoints {
    pub max: u16,
    pub used: u16,
    last: Option<usize>,
}

impl CommandPoints {
    pub fn new(battle: &Battle, leader: usize) -> Self {
        Self { max: battle.units[leader].leader.map_or(0, |r| r.initiative), used: 0, last: None }
    }

    pub fn left(&self) -> u16 {
        self.max - self.used
    }

    /// The AI's count (the original's AI keeps its own): one order per new
    /// unit, wherever the unit stands.
    pub fn ai_unit_done(&mut self, unit: usize) {
        if self.last != Some(unit) {
            self.used += 1;
            self.last = Some(unit);
        }
    }

    /// Whether the leader may give orders to `unit`: always to the unit he
    /// is ordering, to a new one only with points left.
    pub fn can_order(&self, unit: usize) -> bool {
        self.last == Some(unit) || !self.finished()
    }

    /// Whether the leader has used all his points.
    pub fn finished(&self) -> bool {
        self.used >= self.max
    }

    /// Charge the leader for `unit` finishing its orders: one point each
    /// time he switches to a different unit, two if that unit is not in his
    /// own command group.
    pub fn unit_done(&mut self, battle: &Battle, leader: usize, unit: usize) {
        if self.last == Some(unit) {
            return;
        }
        self.last = Some(unit);
        let l = &battle.units[leader];
        let own = battle.units[unit].group == l.group;
        // (Caesar's staff leaders, whose own group has no units, pay no
        // more for another's, `FUN_0056b057`)
        let staff =
            (battle.caesar() || battle.options.staff_leaders) && !battle.units.iter().any(|u| u.leader.is_none() && u.group == l.group && u.side == l.side);
        self.used = (self.used + if own || staff { 1 } else { 2 }).min(self.max);
    }

    /// A line command uses all remaining points.
    pub fn personal_combat(&mut self) {
        self.used = self.max;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::{LeaderRatings, Unit};

    fn unit(type_id: usize, hex: Hex, facing: u8, group: u16, leader: Option<LeaderRatings>) -> Unit {
        Unit {
            source: 0,
            name: String::new(),
            type_id,
            nation: 2,
            side: 2,
            group,
            hex,
            facing,
            leader,
            retreat: 7,
            max_mp: 5,
            mp: 5,
            turned: false,
            size: 6,
            tq: 6,
            hits: 0,
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
            manual_slopes: None,
        }
    }

    /// The manual's turning (an option): chariots pay 2.
    #[test]
    fn manual_turning_costs_two() {
        let mut b = Battle { units: vec![unit(22, Hex::new(5, 5), 3, 0, None)], ..Default::default() };
        turn(&mut b, &map(), 0, true).unwrap();
        assert_eq!(b.units[0].mp, 4);
        b.options.manual_turning = true;
        turn(&mut b, &map(), 0, true).unwrap();
        assert_eq!(b.units[0].mp, 2);
    }

    /// Roads (an option for Alexander and Hannibal): road to road a point.
    #[test]
    fn roads_cost_one() {
        let mut m = map();
        let (a, b) = (Hex::new(4, 4), Hex::new(4, 5));
        for h in [a, b] {
            let (gx, gy) = Map::hex_cell(h.x as usize, h.y as usize);
            let w = m.grid_width();
            m.cells[gy as usize * w + gx as usize] = vec![12, 1];
        }
        let d = a.neighbours().iter().position(|&n| n == b).unwrap();
        assert_eq!(crate::movement::step_cost(&m, MoveClass::HeavyInfantry, a, d), Some(2));
        m.fast_roads = true;
        assert_eq!(crate::movement::step_cost(&m, MoveClass::HeavyInfantry, a, d), Some(1));
    }

    #[test]
    fn about_face_reverses_and_swaps() {
        let mut b = Battle { units: vec![unit(17, Hex::new(5, 5), 2, 0, None)], ..Default::default() };
        b.units[0].mp = 4;
        let second = b.units[0].second_hex().unwrap();
        about_face(&mut b, 0).unwrap();
        assert_eq!((b.units[0].facing, b.units[0].mp, b.units[0].hex), (6, 1, second));
        assert_eq!(b.units[0].second_hex(), Some(Hex::new(5, 5)));
        assert_eq!(about_face(&mut b, 0), Err(Refused::NoMovementPoints));
    }

    #[test]
    fn turning_cycles_through_six_facings() {
        let mut f = 3;
        for _ in 0..6 {
            f = turned(f, true);
        }
        assert_eq!(f, 3);
        assert_eq!(turned(3, true), 2);
        assert_eq!(turned(2, true), 8);
        assert_eq!(turned(4, false), 6);
    }

    #[test]
    fn turning_costs_a_point_but_skirmishers_only_once() {
        let mut b = Battle { units: vec![unit(31, Hex::new(5, 5), 3, 0, None), unit(2, Hex::new(8, 8), 3, 0, None)], ..Default::default() };
        let m = map();
        turn(&mut b, &m, 0, true).unwrap();
        turn(&mut b, &m, 0, true).unwrap();
        assert_eq!(b.units[0].mp, 3);
        turn(&mut b, &m, 1, true).unwrap();
        turn(&mut b, &m, 1, false).unwrap();
        assert_eq!(b.units[1].mp, 4);
    }

    #[test]
    fn two_hex_units_pivot() {
        let m = map();
        let mut b = Battle { units: vec![unit(17, Hex::new(5, 5), 3, 0, None)], ..Default::default() };
        let second = b.units[0].second_hex().unwrap();
        // right turn: the second hex stays
        turn(&mut b, &m, 0, false).unwrap();
        assert_eq!(b.units[0].second_hex(), Some(second));
        assert_eq!(b.units[0].hex.distance(second), 1);
        // left turn: the main hex stays
        let main = b.units[0].hex;
        turn(&mut b, &m, 0, true).unwrap();
        assert_eq!(b.units[0].hex, main);
    }

    #[test]
    fn volleys_need_arc_range_and_missiles() {
        use crate::missile::Weapon;
        let mut b = Battle { units: vec![unit(10, Hex::new(2, 4), 3, 0, None), unit(31, Hex::new(4, 4), 7, 0, None)], ..Default::default() };
        b.units[1].side = 1;
        b.units[0].missile = Some((Weapon::Bow, 10));
        let mut rng = BorlandRand::new(3);
        assert!(volley(&mut b, 0, 1, 0, &mut rng).is_ok());
        assert_eq!(b.units[0].missile, Some((Weapon::Bow, 9)));
        // once per leader's orders
        assert!(volley(&mut b, 0, 1, 0, &mut rng).is_err());
        // not behind
        b.units[0].fired = false;
        b.units[0].facing = 7;
        assert!(volley(&mut b, 0, 1, 0, &mut rng).is_err());
    }

    #[test]
    fn moving_next_to_javelins_draws_reaction_fire() {
        use crate::missile::Weapon;
        let m = map();
        // the mover faces 3 and walks east past a javelin unit facing it
        let mut b = Battle { units: vec![unit(31, Hex::new(2, 5), 3, 0, None), unit(2, Hex::new(5, 5), 7, 0, None)], ..Default::default() };
        b.units[1].side = 1;
        b.units[1].missile = Some((Weapon::Javelin, 3));
        let target = Hex::new(4, 5);
        let moved = move_to(&mut b, &m, 0, target, &mut BorlandRand::new(1)).unwrap();
        assert!(!moved.fire.is_empty());
        assert!(b.units[1].missile.unwrap().1 < 3);
    }

    #[test]
    fn resupply_only_empty_units() {
        use crate::missile::Weapon;
        let mut b = Battle { units: vec![unit(2, Hex::new(2, 2), 3, 0, None), unit(2, Hex::new(3, 3), 3, 0, None)], ..Default::default() };
        b.units[0].missile = Some((Weapon::Javelin, 0));
        b.units[1].missile = Some((Weapon::Javelin, 2));
        let r = resupply(&mut b, &[0, 1], &mut BorlandRand::new(5));
        assert_eq!(r, vec![0]);
        assert!(b.units[0].missile.unwrap().1 <= 1 && b.units[0].done);
        assert_eq!(b.units[1].missile.unwrap().1, 2);
    }

    #[test]
    fn terrain_costs_cohesion() {
        let mut m = map();
        // woods ahead: infantry and horse take a hit, skirmishers none
        let target = Hex::new(3, 5);
        let (gx, gy) = Map::hex_cell(target.x as usize, target.y as usize);
        let gw = m.grid_width();
        m.cells[gy as usize * gw + gx as usize] = vec![1];
        let b = Battle {
            units: vec![unit(31, Hex::new(2, 5), 3, 0, None), unit(2, Hex::new(2, 5), 3, 0, None), unit(8, Hex::new(2, 5), 3, 0, None)],
            ..Default::default()
        };
        assert_eq!(terrain_hits(&b, &m, 0, Hex::new(2, 5), target), 1);
        assert_eq!(terrain_hits(&b, &m, 1, Hex::new(2, 5), target), 0);
        // horses next to an enemy elephant: 2 more
        let mut b = b;
        b.units.push(unit(24, Hex::new(4, 5), 7, 0, None));
        b.units[3].side = 1;
        assert_eq!(terrain_hits(&b, &m, 2, Hex::new(2, 5), target), 3);
        // up one level: a hit for all but elephants
        let mut m2 = map();
        let i = target.y as usize * m2.width + target.x as usize;
        m2.elevation[i] = 2;
        assert_eq!(terrain_hits(&b, &m2, 0, Hex::new(2, 5), target), 1);
    }

    #[test]
    fn cavalry_withdraws_from_infantry() {
        let m = map();
        // infantry walks up to cavalry facing it; the cavalry (TQ 9, 0 hits)
        // always passes and backs away
        let mut b = Battle { units: vec![unit(31, Hex::new(2, 5), 3, 0, None), unit(8, Hex::new(5, 5), 7, 0, None)], ..Default::default() };
        b.units[1].side = 1;
        b.units[1].tq = 9;
        move_to(&mut b, &m, 0, Hex::new(4, 5), &mut BorlandRand::new(1)).unwrap();
        assert!(b.units[1].hex.distance(b.units[0].hex) > 1);
    }

    #[test]
    fn recovery_chances_follow_the_leader() {
        let mut b = Battle { units: vec![unit(31, Hex::new(2, 2), 3, 0, None)], ..Default::default() };
        let mut removed = [0; 4];
        let mut rng = BorlandRand::new(9);
        for _ in 0..4000 {
            b.units[0].hits = 3;
            removed[recover(&mut b, &map(), 0, 7, &mut rng) as usize] += 1;
        }
        // initiative 7: 0%, 33%, 50%, 17%
        assert_eq!(removed[0], 0);
        assert!((1200..1450).contains(&removed[1]), "{removed:?}");
        assert!((550..800).contains(&removed[3]), "{removed:?}");
    }

    #[test]
    fn command_points() {
        let ratings = LeaderRatings { charisma: 0, command_range: 5, initiative: 3, personal_combat: 1 };
        let b = Battle {
            units: vec![unit(4, Hex::new(1, 1), 3, 1, Some(ratings)), unit(31, Hex::new(2, 1), 3, 1, None), unit(31, Hex::new(3, 1), 3, 2, None)],
            ..Default::default()
        };
        let mut c = CommandPoints::new(&b, 0);
        c.unit_done(&b, 0, 1);
        c.unit_done(&b, 0, 1);
        assert_eq!(c.left(), 2);
        c.unit_done(&b, 0, 2);
        assert!(c.finished());
    }
}
