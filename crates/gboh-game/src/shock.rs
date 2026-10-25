//! A whole shock combat on the battle state: pre-contact checks, the
//! table, sharing hits and collapse (see `docs/formats.md`, "Shock
//! combat").
//!
//! Status 🟡: the steps follow the original's combat function; leader
//! casualties, advances and routs are not applied yet.

use crate::battle::Battle;
use crate::combat::{LeaderCasualty, Share, Shock, leader_casualty, checks_before_shock, defending_class, share_hits, side_size, tq_check_unit};
use crate::hex::Hex;
use crate::movement::{MoveClass, forward};
use gboh_formats::crt::BorlandRand;

/// Hannibal's Carthaginian nation (scenario byte).
const CARTHAGINIAN: u8 = 7;

/// Angle of an attack from `from` on a unit at `at` with scenario facing
/// `facing`: 0 front, 1 flank, 2 rear.
pub fn angle(at: Hex, facing: u8, from: Hex) -> u8 {
    let Some(i) = at.neighbours().iter().position(|&n| n == from) else { return 0 };
    let Some(front) = forward(facing) else { return 0 };
    let behind = front.map(|d| (d + 3) % 6);
    if front.contains(&i) {
        0
    } else if behind.contains(&i) {
        2
    } else {
        1
    }
}

/// The leading attacker and its angle against the primary target, as the
/// original picks them. Each attacker is scored by its frontal base column
/// against the target's class. One standing in the target's flank or rear
/// (either of its hexes, for two-hex units) takes the lead if its score is
/// at least the best so far; a frontal attacker takes it only while no
/// flanker has, with the same test. Ties go to the later unit. So a flanker
/// scoring lower than an earlier frontal attacker is not used.
pub fn leading_attacker(battle: &Battle, attackers: &[usize], target: usize) -> (usize, u8) {
    let t = &battle.units[target];
    let t_class = class(battle, target);
    let mut best = 0i32;
    let mut lead = (attackers[0], 0u8);
    let mut flanked = false;
    for &a in attackers {
        let score = crate::combat::base_column(class(battle, a), t_class, 0);
        let ang = battle.hexes(a).iter().map(|&h| angle(t.hex, t.facing, h)).max().unwrap_or(0);
        if ang > 0 && score >= best {
            best = score;
            flanked = true;
            lead = (a, ang);
        } else if !flanked && best <= score {
            best = score;
            lead = (a, 0);
        }
    }
    lead
}

/// What happened to the units in one shock.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Hits each unit took, by unit index.
    pub hits: Vec<(usize, i32)>,
    /// Units that routed.
    pub routed: Vec<usize>,
    /// Routed defenders eliminated on being attacked.
    pub eliminated: Vec<usize>,
    /// The leader who lost the casualty roll, and what happened to him
    /// (not applied to the battle: leaders do not track wounds yet).
    pub leader: Option<(usize, LeaderCasualty)>,
}

/// The leader of a side involved in a shock: among leaders stacked with any
/// of `units`, the one with the highest first rating.
fn involved_leader(battle: &Battle, units: &[usize]) -> Option<usize> {
    let hexes: Vec<Hex> = units.iter().flat_map(|&u| battle.hexes(u)).collect();
    battle
        .leaders()
        .filter(|&l| hexes.contains(&battle.units[l].hex))
        .max_by_key(|&l| battle.units[l].leader.map_or(0, |r| r.charisma))
}

fn class(battle: &Battle, u: usize) -> MoveClass {
    MoveClass::of_type(battle.units[u].type_id).unwrap_or(MoveClass::HeavyInfantry)
}

fn routed(battle: &Battle, u: usize) -> bool {
    battle.units[u].hits >= battle.units[u].tq as i32
}

/// Resolve one shock of `attackers` (with whether each moved this phase)
/// against `defenders`, updating hits on `battle`.
pub fn resolve(battle: &mut Battle, attackers: &[(usize, bool)], defenders: &[usize], rng: &mut BorlandRand) -> Outcome {
    let mut out = Outcome::default();
    let hit = |out: &mut Outcome, battle: &mut Battle, u: usize, h: i32| {
        battle.units[u].hits += h;
        out.hits.push((u, h));
    };

    // routed defenders are eliminated outright
    let mut standing = Vec::new();
    for &d in defenders {
        if routed(battle, d) { out.eliminated.push(d) } else { standing.push(d) }
    }
    if standing.is_empty() {
        return out;
    }

    // pre-contact TQ checks for marked attackers
    let mut fighting = Vec::new();
    let (mut checked, mut all_light, mut chariot) = (false, true, false);
    let mut foreign_elephant = false;
    for &(a, moved) in attackers {
        let d = standing[0];
        let frontal = angle(battle.units[d].hex, battle.units[d].facing, battle.units[a].hex) == 0;
        if checks_before_shock(class(battle, a), moved, class(battle, d), frontal) {
            checked = true;
            all_light &= class(battle, a) == MoveClass::LightInfantry;
            chariot |= class(battle, a) == MoveClass::Chariot;
            // Hannibal's: charging Celts shake the defenders as chariots do
            // (the unit's name has the word); and elephants other than the
            // Carthaginians' shake Carthaginian elephants (🟡)
            if battle.hannibal() {
                chariot |= battle.units[a].name.contains("Celts");
                foreign_elephant |= class(battle, a) == MoveClass::Elephant && battle.units[a].nation != CARTHAGINIAN;
            }
            let u = &battle.units[a];
            let (h, rout) = tq_check_unit(u.hits, u.tq as i32, rng.next() % 10);
            let before = u.hits;
            hit(&mut out, battle, a, h - before);
            if rout {
                out.routed.push(a);
                continue;
            }
        }
        fighting.push(a);
    }
    // then the defenders check (the original's combat function): all of
    // them once any attacker checked, except phalanxes and heavy infantry
    // when every checking attacker is light infantry; a charging chariot
    // adds 1 to their roll
    if checked {
        let mut still = Vec::new();
        for &d in &standing {
            let c = class(battle, d);
            if all_light && matches!(c, MoveClass::Phalanx | MoveClass::HeavyInfantry) {
                still.push(d);
                continue;
            }
            let u = &battle.units[d];
            let elephants = foreign_elephant && c == MoveClass::Elephant && battle.units[d].nation == CARTHAGINIAN;
            let roll = rng.next() % 10 + (chariot || elephants) as u32;
            let (h, rout) = tq_check_unit(u.hits, u.tq as i32, roll);
            let before = u.hits;
            hit(&mut out, battle, d, h - before);
            if rout {
                out.routed.push(d);
            } else {
                still.push(d);
            }
        }
        standing = still;
    }
    if fighting.is_empty() || standing.is_empty() {
        return out;
    }

    // leading attacker and angle, as the original picks them
    let def_classes: Vec<MoveClass> = standing.iter().map(|&d| class(battle, d)).collect();
    let (lead, attack_angle) = leading_attacker(battle, &fighting, standing[0]);
    let a_class = class(battle, lead);
    let shock = Shock {
        attacker: a_class,
        defender: defending_class(a_class, &def_classes).unwrap_or(def_classes[0]),
        angle: attack_angle,
        attacker_size: side_size(&fighting.iter().map(|&a| (class(battle, a), battle.units[a].size as i32)).collect::<Vec<_>>()),
        defender_size: side_size(&standing.iter().map(|&d| (class(battle, d), battle.units[d].size as i32)).collect::<Vec<_>>()),
        attacker_cataphract: gboh_formats::units::cataphract(battle.units[lead].type_id),
        defender_cataphract: standing.iter().any(|&d| gboh_formats::units::cataphract(battle.units[d].type_id)),
        defender_missile: false,
        no_superiority: false,
        column_shift: 0,
        hannibal: battle.hannibal(),
        attacker_nation: battle.units[lead].nation,
        defender_nation: battle.units[standing[0]].nation,
    };
    let r = shock.resolve(rng.next() % 10);

    // share out and collapse
    let contact = true;
    let shares = |units: &[usize], b: &Battle| -> Vec<Share> {
        units.iter().map(|&u| Share { hits: b.units[u].hits, tq: b.units[u].tq as i32, enemy_contact: contact }).collect()
    };
    let a_shares = shares(&fighting, battle);
    let d_shares = shares(&standing, battle);
    let lead_i = fighting.iter().position(|&a| a == lead).unwrap_or(0);
    let mut roll = || rng.next() % 10;
    let a_hits = share_hits(r.attacker_hits, &a_shares, lead_i, r.defender_hits, standing.len(), &mut roll);
    let d_hits = share_hits(r.defender_hits, &d_shares, 0, r.attacker_hits, fighting.len(), &mut roll);
    for (&u, &h) in fighting.iter().zip(&a_hits).chain(standing.iter().zip(&d_hits)) {
        hit(&mut out, battle, u, h);
        if routed(battle, u) {
            out.routed.push(u);
        }
    }

    // leader casualties (attacking side rolls first)
    if let (Some(la), Some(ld)) = (involved_leader(battle, &fighting), involved_leader(battle, &standing)) {
        let fourth = |l: usize| battle.units[l].leader.map_or(0, |r| r.personal_combat);
        let ra = rng.next() % 10;
        let rd = rng.next() % 10;
        if let Some((att_lost, effect)) = leader_casualty((fourth(la), ra), (fourth(ld), rd), |_| false) {
            out.leader = Some((if att_lost { la } else { ld }, effect));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::Unit;

    fn unit(type_id: usize, side: u8, hex: Hex, facing: u8, size: u16, tq: u16) -> Unit {
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
            max_mp: 5,
            mp: 5,
            turned: false,
            size,
            tq,
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
            done: false,
        }
    }

    #[test]
    fn flankers_lead_unless_outscored_first() {
        let t = Hex::new(5, 5);
        let n = t.neighbours();
        // target faces 3 (front n[1], n[2]); flank n[0], rear n[4]
        let mk = |ty, hex| unit(ty, 2, hex, 6, 6, 6);
        let mut b = Battle { units: vec![unit(31, 1, t, 3, 6, 6), mk(31, n[1]), mk(2, n[0])] };
        // a skirmisher on the flank scores below the frontal hoplites: not used
        assert_eq!(leading_attacker(&b, &[1, 2], 0), (1, 0));
        // listed first it leads, and a frontal unit cannot take over
        assert_eq!(leading_attacker(&b, &[2, 1], 0), (2, 1));
        b.units[2].type_id = 31;
        b.units[2].hex = n[4];
        assert_eq!(leading_attacker(&b, &[1, 2], 0), (2, 2));
    }

    #[test]
    fn angles() {
        let at = Hex::new(5, 5);
        let n = at.neighbours();
        // facing 3 looks NE and SE
        assert_eq!(angle(at, 3, n[1]), 0);
        assert_eq!(angle(at, 3, n[2]), 0);
        assert_eq!(angle(at, 3, n[4]), 2);
        assert_eq!(angle(at, 3, n[0]), 1);
    }

    #[test]
    fn routed_defenders_are_eliminated() {
        let mut b = Battle { units: vec![unit(31, 2, Hex::new(5, 5), 3, 6, 6), unit(31, 1, Hex::new(6, 5), 7, 6, 6)] };
        b.units[1].hits = 6;
        let out = resolve(&mut b, &[(0, true)], &[1], &mut BorlandRand::new(1));
        assert_eq!(out.eliminated, vec![1]);
    }

    #[test]
    fn a_shock_deals_hits_to_both_sides() {
        let mut b = Battle { units: vec![unit(31, 2, Hex::new(5, 5), 3, 6, 9), unit(31, 1, Hex::new(6, 5), 7, 6, 9)] };
        let out = resolve(&mut b, &[(0, false)], &[1], &mut BorlandRand::new(1));
        assert!(b.units[0].hits > 0 && b.units[1].hits > 0, "{out:?}");
    }
}
