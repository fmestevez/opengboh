//! Orders: what an activated leader's units do, and what it costs him.

use crate::battle::{Battle, flank, neighbour};
use crate::hex::Hex;
use crate::battle::UnitState;
use crate::missile::Volley;
use crate::movement::{MoveClass, arc, path_to, reachable};
use gboh_formats::crt::BorlandRand;
use gboh_formats::map::Map;

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
pub fn turn(battle: &mut Battle, map: &Map, unit: usize, left: bool) -> Result<(), Refused> {
    if battle.units[unit].state == UnitState::Routed {
        return Err(Refused::Routed);
    }
    let u = &battle.units[unit];
    let free = u.turned && MoveClass::of_type(u.type_id) == Some(MoveClass::Skirmisher);
    if !free && u.mp < 1 {
        return Err(Refused::NoMovementPoints);
    }
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
    let u = &mut battle.units[unit];
    if !free {
        u.mp -= 1;
    }
    if MoveClass::of_type(u.type_id) == Some(MoveClass::Chariot) {
        u.hits += 2;
    }
    u.turned = true;
    u.facing = facing;
    u.hex = main;
    Ok(())
}

/// Move `unit` to `to` along the cheapest legal path (see [`reachable`]),
/// one hex at a time. Each hex entered next to an enemy missile unit that
/// has missiles and the hex in its arc draws a reaction volley (range 1);
/// the move stops if the unit routs. Returns the reaction volleys as
/// `(shooter, hits)`.
pub fn move_to(
    battle: &mut Battle,
    map: &Map,
    unit: usize,
    to: Hex,
    rng: &mut BorlandRand,
) -> Result<Vec<(usize, i32)>, Refused> {
    if battle.units[unit].state == UnitState::Routed {
        return Err(Refused::Routed);
    }
    let mp = battle.units[unit].mp;
    let path = path_to(battle, map, unit, mp, to).ok_or(Refused::Unreachable)?;
    // moving again after momentum: one cohesion hit, once a turn
    let u = &mut battle.units[unit];
    if u.reordered && !u.reorder_hit {
        u.reorder_hit = true;
        u.hits += 1;
    }
    let costs = reachable(battle, map, unit, mp);
    let mut fire = Vec::new();
    for h in path {
        battle.units[unit].mp = mp - costs[&h];
        battle.units[unit].hex = h;
        for s in reacting(battle, unit) {
            let class = MoveClass::of_type(battle.units[unit].type_id).unwrap_or(MoveClass::HeavyInfantry);
            let Some((weapon, left)) = battle.units[s].missile else { continue };
            battle.units[s].missile = Some((weapon, left - 1));
            let cat = matches!(battle.units[unit].type_id, 41 | 44);
            let r = Volley::reaction(weapon, class, cat).resolve(rng.next() % 10);
            battle.units[unit].hits += r.hits;
            fire.push((s, r.hits));
        }
        let u = &battle.units[unit];
        if u.hits >= u.tq as i32 {
            battle.units[unit].state = UnitState::Routed;
            break;
        }
        for e in withdrawers(battle, unit) {
            withdraw(battle, map, e, unit, rng);
        }
    }
    Ok(fire)
}

/// Enemy units next to `mover` that may withdraw from it (the manual):
/// cavalry from non-cavalry, leaders from anyone, skirmishers from heavy
/// infantry and phalanxes.
fn withdrawers(battle: &Battle, mover: usize) -> Vec<usize> {
    use MoveClass::*;
    let m = &battle.units[mover];
    let m_class = MoveClass::of_type(m.type_id);
    let m_horse = matches!(m_class, Some(HeavyCavalry | Lancer | LightCavalry));
    (0..battle.units.len())
        .filter(|&e| {
            let o = &battle.units[e];
            if o.side == m.side || o.state != UnitState::Fighting || o.hex.distance(m.hex) != 1 {
                return false;
            }
            match MoveClass::of_type(o.type_id) {
                None => o.leader.is_some(),
                Some(HeavyCavalry | Lancer | LightCavalry) => !m_horse,
                Some(Skirmisher) => matches!(m_class, Some(HeavyInfantry | Phalanx)),
                _ => false,
            }
        })
        .collect()
}

/// Orderly withdrawal of `unit` from `enemy` (the manual; 🟡 the
/// original's code is not traced): a TQ check (`rand() % 10 ≤ TQ − hits`);
/// on success one cohesion hit if approached from the flank or rear (only
/// the rear for skirmishers), then up to two hexes straight back, keeping
/// its facing, never into an enemy zone of control, rough terrain or an
/// occupied hex.
pub fn withdraw(battle: &mut Battle, map: &Map, unit: usize, enemy: usize, rng: &mut BorlandRand) -> bool {
    let u = &battle.units[unit];
    if (rng.next() % 10) as i32 > u.tq as i32 - u.hits {
        return false;
    }
    let from = battle.units[enemy].hex;
    let angle = crate::shock::angle(u.hex, u.facing, from);
    let skirmisher = MoveClass::of_type(u.type_id) == Some(MoveClass::Skirmisher);
    let back: Vec<usize> = crate::movement::forward(u.facing)
        .map(|f| f.map(|d| (d + 3) % 6).to_vec())
        .unwrap_or_else(|| (0..6).collect());
    let side = u.side;
    if angle == 2 || (angle == 1 && !skirmisher) {
        battle.units[unit].hits += 1;
    }
    for _ in 0..2 {
        let here = battle.units[unit].hex;
        let next = back.iter().map(|&d| here.neighbours()[d]).find(|&h| {
            crate::movement::on_map(map, h)
                && !map.hex_features(h.x as usize, h.y as usize).iter().any(|&c| (1..=5).contains(&c))
                && !crate::movement::in_enemy_zoc(battle, side, h)
                && battle.units.iter().all(|o| o.state == UnitState::Gone || o.hex != h)
                && h.distance(from) > here.distance(from)
        });
        match next {
            Some(h) => battle.units[unit].hex = h,
            None => break,
        }
    }
    true
}

/// Enemy missile units that react to `unit` standing where it is.
fn reacting(battle: &Battle, unit: usize) -> Vec<usize> {
    let u = &battle.units[unit];
    battle
        .units
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.side != u.side
                && s.state == UnitState::Fighting
                && s.missile.is_some_and(|(_, n)| n > 0)
                && arc(s.hex, s.facing, 1).contains(&u.hex)
        })
        .map(|(i, _)| i)
        .collect()
}

/// Order `shooter` to volley at `target`: once per leader's orders, needs a
/// missile and the target in its arc within the weapon's range.
pub fn volley(battle: &mut Battle, shooter: usize, target: usize, rng: &mut BorlandRand) -> Result<i32, Refused> {
    let s = &battle.units[shooter];
    let t = &battle.units[target];
    if s.state == UnitState::Routed {
        return Err(Refused::Routed);
    }
    let Some((weapon, left)) = s.missile else { return Err(Refused::Unreachable) };
    let range = s.hex.distance(t.hex);
    if s.fired || left == 0 || !arc(s.hex, s.facing, weapon.range()).contains(&t.hex) {
        return Err(Refused::Unreachable);
    }
    let class = MoveClass::of_type(t.type_id).unwrap_or(MoveClass::HeavyInfantry);
    let v = Volley {
        weapon,
        range,
        target: class,
        from_front: arc(t.hex, t.facing, 1).contains(&s.hex),
        moved: s.mp < s.max_mp,
        target_cataphract: matches!(t.type_id, 41 | 44),
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
pub fn recover(battle: &mut Battle, unit: usize, initiative: u16, rng: &mut BorlandRand) -> i32 {
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
    if crate::movement::in_enemy_zoc(battle, u.side, u.hex) {
        roll -= 40;
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

/// Resupply at a leader's activation (the manual; 🟡 the original's code is
/// not traced): each unit given that is out of missiles gains `rand() %
/// (half + 1)` of its supply, rounded up. Returns the units resupplied
/// (they may not be ordered this activation).
pub fn resupply(battle: &mut Battle, units: &[usize], rng: &mut BorlandRand) -> Vec<usize> {
    let mut out = Vec::new();
    for &u in units {
        if let Some((w, 0)) = battle.units[u].missile {
            let half = w.supply().div_ceil(2);
            battle.units[u].missile = Some((w, (rng.next() % (half as u32 + 1)) as u16));
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
        let own = battle.units[unit].group == battle.units[leader].group;
        self.used = (self.used + if own { 1 } else { 2 }).min(self.max);
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
        let mut b = Battle { units: vec![unit(31, Hex::new(5, 5), 3, 0, None), unit(2, Hex::new(8, 8), 3, 0, None)] };
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
        let mut b = Battle { units: vec![unit(17, Hex::new(5, 5), 3, 0, None)] };
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
        let mut b = Battle {
            units: vec![unit(10, Hex::new(2, 4), 3, 0, None), unit(31, Hex::new(4, 4), 7, 0, None)],
        };
        b.units[1].side = 1;
        b.units[0].missile = Some((Weapon::Bow, 10));
        let mut rng = BorlandRand::new(3);
        assert!(volley(&mut b, 0, 1, &mut rng).is_ok());
        assert_eq!(b.units[0].missile, Some((Weapon::Bow, 9)));
        // once per leader's orders
        assert!(volley(&mut b, 0, 1, &mut rng).is_err());
        // not behind
        b.units[0].fired = false;
        b.units[0].facing = 7;
        assert!(volley(&mut b, 0, 1, &mut rng).is_err());
    }

    #[test]
    fn moving_next_to_javelins_draws_reaction_fire() {
        use crate::missile::Weapon;
        let m = map();
        // the mover faces 3 and walks east past a javelin unit facing it
        let mut b = Battle {
            units: vec![unit(31, Hex::new(2, 5), 3, 0, None), unit(2, Hex::new(5, 5), 7, 0, None)],
        };
        b.units[1].side = 1;
        b.units[1].missile = Some((Weapon::Javelin, 3));
        let target = Hex::new(4, 5);
        let fire = move_to(&mut b, &m, 0, target, &mut BorlandRand::new(1)).unwrap();
        assert!(!fire.is_empty());
        assert!(b.units[1].missile.unwrap().1 < 3);
    }

    #[test]
    fn resupply_only_empty_units() {
        use crate::missile::Weapon;
        let mut b = Battle { units: vec![unit(2, Hex::new(2, 2), 3, 0, None), unit(2, Hex::new(3, 3), 3, 0, None)] };
        b.units[0].missile = Some((Weapon::Javelin, 0));
        b.units[1].missile = Some((Weapon::Javelin, 2));
        let r = resupply(&mut b, &[0, 1], &mut BorlandRand::new(5));
        assert_eq!(r, vec![0]);
        assert!(b.units[0].missile.unwrap().1 <= 2);
        assert_eq!(b.units[1].missile.unwrap().1, 2);
    }

    #[test]
    fn cavalry_withdraws_from_infantry() {
        let m = map();
        // infantry walks up to cavalry facing it; the cavalry (TQ 9, 0 hits)
        // always passes and backs away
        let mut b = Battle {
            units: vec![unit(31, Hex::new(2, 5), 3, 0, None), unit(8, Hex::new(5, 5), 7, 0, None)],
        };
        b.units[1].side = 1;
        b.units[1].tq = 9;
        move_to(&mut b, &m, 0, Hex::new(4, 5), &mut BorlandRand::new(1)).unwrap();
        assert!(b.units[1].hex.distance(b.units[0].hex) > 1);
    }

    #[test]
    fn recovery_chances_follow_the_leader() {
        let mut b = Battle { units: vec![unit(31, Hex::new(2, 2), 3, 0, None)] };
        let mut removed = [0; 4];
        let mut rng = BorlandRand::new(9);
        for _ in 0..4000 {
            b.units[0].hits = 3;
            removed[recover(&mut b, 0, 7, &mut rng) as usize] += 1;
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
            units: vec![
                unit(4, Hex::new(1, 1), 3, 1, Some(ratings)),
                unit(31, Hex::new(2, 1), 3, 1, None),
                unit(31, Hex::new(3, 1), 3, 2, None),
            ],
        };
        let mut c = CommandPoints::new(&b, 0);
        c.unit_done(&b, 0, 1);
        c.unit_done(&b, 0, 1);
        assert_eq!(c.left(), 2);
        c.unit_done(&b, 0, 2);
        assert!(c.finished());
    }
}
