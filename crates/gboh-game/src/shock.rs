//! A whole shock combat on the battle state: pre-contact checks, the
//! table, sharing hits and collapse (see `docs/formats.md`, "Shock
//! combat").
//!
//! Status 🟡: the steps follow the original's combat function; leader
//! casualties, advances and routs are not applied yet.

use crate::battle::Battle;
use crate::combat::{LeaderCasualty, Share, Shock, checks_before_shock, defending_class, leader_casualty, share_hits, side_size, tq_check_unit};
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
    // the most advantageous angle any attacker has, as the manual says
    // (the original let a flanker lead only if it scored at least as well
    // as those before it, so a weaker flanker lost the flank: fixed), and
    // among those at it the best score, ties to the later
    let angle_of = |a: usize| battle.hexes(a).iter().map(|&h| angle(t.hex, t.facing, h)).max().unwrap_or(0);
    let best_angle = attackers.iter().map(|&a| angle_of(a)).max().unwrap_or(0);
    let mut best = i32::MIN;
    let mut lead = (attackers[0], best_angle);
    for &a in attackers.iter().filter(|&&a| angle_of(a) == best_angle) {
        let score = crate::combat::base_column(class(battle, a), t_class, 0);
        if score >= best {
            best = score;
            lead = (a, best_angle);
        }
    }
    lead
}

/// The manual's terrain adjustment (Alexander's manual, p. 85), the best
/// for the defender: a column left if all defenders stand in a river,
/// marsh or woods, or any stands higher than the attackers or any attacker
/// attacks across a steep-banked stream; two if all do; else a column
/// right if all defenders stand lower than the attacker, none in a river
/// or marsh.
fn manual_terrain_shift(battle: &Battle, map: &gboh_formats::map::Map, attackers: &[usize], defenders: &[usize]) -> i32 {
    let on = |h: Hex| h.x >= 0 && h.y >= 0 && (h.x as usize) < map.width && (h.y as usize) < map.height;
    let codes = |h: Hex| if on(h) { map.hex_features(h.x as usize, h.y as usize).to_vec() } else { Vec::new() };
    let level = |h: Hex| if on(h) { map.elevation_at(h.x as usize, h.y as usize) as i32 } else { 1 };
    let wet = |h: Hex| level(h) == 0 || codes(h).iter().any(|c| matches!(c, 3 | 4));
    let rough = |h: Hex| wet(h) || codes(h).contains(&1);
    let d_hexes: Vec<Hex> = defenders.iter().flat_map(|&d| battle.hexes(d)).collect();
    let a_hexes: Vec<Hex> = attackers.iter().flat_map(|&a| battle.hexes(a)).collect();
    let top = a_hexes.iter().map(|&h| level(h)).max().unwrap_or(1);
    let higher: Vec<bool> = d_hexes.iter().map(|&h| level(h) > top).collect();
    // an attacker across a steep-banked stream (side code 8) from a defender
    let across = |a: usize| {
        battle.hexes(a).iter().any(|&h| {
            gboh_formats::map::Side::ALL.iter().enumerate().any(|(k, &s)| {
                let n = h.neighbours()[k];
                d_hexes.contains(&n) && on(h) && map.side_features(h.x as usize, h.y as usize, s).contains(&8)
            })
        })
    };
    let streams: Vec<bool> = attackers.iter().map(|&a| across(a)).collect();
    let mut shift = 0;
    if !d_hexes.is_empty() && d_hexes.iter().all(|&h| rough(h)) {
        shift = shift.min(-1);
    }
    if higher.iter().any(|&b| b) || streams.iter().any(|&b| b) {
        shift = shift.min(-1);
    }
    if (!higher.is_empty() && higher.iter().all(|&b| b)) || (!streams.is_empty() && streams.iter().all(|&b| b)) {
        shift = shift.min(-2);
    }
    let bottom = a_hexes.iter().map(|&h| level(h)).min().unwrap_or(1);
    if shift == 0 && !d_hexes.is_empty() && d_hexes.iter().all(|&h| level(h) < bottom && !wet(h)) {
        shift = 1;
    }
    shift
}

/// Caesar's manual's terrain adjustments (pp. 86–87), cumulative: a column
/// left each if all defenders stand in a river, marsh or woods; if the
/// defending unit is infantry in rough ground (woods, rocky ground,
/// marsh); if any defender stands higher than the attackers, and another
/// if all do; if any attacker attacks across a river (a stream side or
/// into the lowest ground), two if all do; a column right if all defenders
/// stand lower.
fn caesar_terrain_shift(battle: &Battle, map: &gboh_formats::map::Map, attackers: &[usize], defenders: &[usize]) -> i32 {
    let on = |h: Hex| h.x >= 0 && h.y >= 0 && (h.x as usize) < map.width && (h.y as usize) < map.height;
    let codes = |h: Hex| if on(h) { map.hex_features(h.x as usize, h.y as usize).to_vec() } else { Vec::new() };
    let level = |h: Hex| if on(h) { map.elevation_at(h.x as usize, h.y as usize) as i32 } else { 1 };
    let wet = |h: Hex| level(h) == 0 || codes(h).contains(&3);
    let rough = |h: Hex| wet(h) || codes(h).iter().any(|c| matches!(c, 1 | 2));
    let d_hexes: Vec<Hex> = defenders.iter().flat_map(|&d| battle.hexes(d)).collect();
    let a_hexes: Vec<Hex> = attackers.iter().flat_map(|&a| battle.hexes(a)).collect();
    let (top, bottom) = (a_hexes.iter().map(|&h| level(h)).max().unwrap_or(1), a_hexes.iter().map(|&h| level(h)).min().unwrap_or(1));
    let across = |a: usize| {
        battle.hexes(a).iter().any(|&h| {
            gboh_formats::map::Side::ALL.iter().enumerate().any(|(k, &s)| {
                let n = h.neighbours()[k];
                d_hexes.contains(&n) && on(h) && (map.side_features(h.x as usize, h.y as usize, s).contains(&6) || level(n) == 0)
            })
        })
    };
    let infantry = |d: usize| {
        !matches!(
            class(battle, d),
            MoveClass::HeavyCavalry
                | MoveClass::RomanCavalry
                | MoveClass::BarbarianCavalry
                | MoveClass::LightCavalry
                | MoveClass::Lancer
                | MoveClass::Elephant
                | MoveClass::Chariot
        )
    };
    let mut shift = 0;
    if !d_hexes.is_empty() && d_hexes.iter().all(|&h| rough(h)) {
        shift -= 1;
    }
    if defenders.first().is_some_and(|&d| infantry(d) && battle.hexes(d).iter().any(|&h| rough(h))) {
        shift -= 1;
    }
    if d_hexes.iter().any(|&h| level(h) > top) {
        shift -= 1;
    }
    if !d_hexes.is_empty() && d_hexes.iter().all(|&h| level(h) > top) {
        shift -= 1;
    }
    let streams: Vec<bool> = attackers.iter().map(|&a| across(a)).collect();
    if streams.iter().any(|&b| b) {
        shift -= if streams.iter().all(|&b| b) { 2 } else { 1 };
    }
    if !d_hexes.is_empty() && d_hexes.iter().all(|&h| level(h) < bottom) {
        shift += 1;
    }
    shift
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
    battle.leaders().filter(|&l| hexes.contains(&battle.units[l].hex)).max_by_key(|&l| battle.units[l].leader.map_or(0, |r| r.charisma))
}

fn class(battle: &Battle, u: usize) -> MoveClass {
    let o = &battle.units[u];
    // Caesar's cohorts fight by their type (`FUN_0048b4d9`): veterans as
    // legionaries, recruits as heavy infantry, conscripts as light
    if o.type_id >= gboh_formats::units::CAESAR
        && let Some(t) = o.legion
    {
        return match t {
            1 => MoveClass::Legion,
            2 => MoveClass::HeavyInfantry,
            _ => MoveClass::LightInfantry,
        };
    }
    MoveClass::of_type(o.type_id).unwrap_or(MoveClass::HeavyInfantry)
}

fn routed(battle: &Battle, u: usize) -> bool {
    battle.units[u].hits >= battle.units[u].tq as i32
}

/// Resolve one shock of `attackers` (with whether each moved this phase)
/// against `defenders`, updating hits on `battle`.
/// `map`: the battlefield, for Caesar's berms (Dyrrhachium's) and the
/// manual's terrain shifts (an option).
pub fn resolve(battle: &mut Battle, attackers: &[(usize, bool)], defenders: &[usize], map: Option<&gboh_formats::map::Map>, rng: &mut BorlandRand) -> Outcome {
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
    let manual = battle.options.manual_shock;
    // (the manual's charge: only the classes it lists check, and none
    // attacking skirmishers)
    let (hannibal, caesar) = (battle.hannibal(), battle.caesar());
    let manual_checks = move |c: MoveClass| match c {
        MoveClass::HeavyInfantry | MoveClass::MediumInfantry | MoveClass::Phalanx | MoveClass::Lancer | MoveClass::Elephant => true,
        MoveClass::Chariot => !hannibal,
        MoveClass::Legion => caesar,
        _ => false,
    };
    let at_skirmishers = standing.iter().all(|&d| class(battle, d) == MoveClass::Skirmisher);
    let mut all_frontal_light = true;
    for &(a, moved) in attackers {
        let d = standing[0];
        let frontal = angle(battle.units[d].hex, battle.units[d].facing, battle.units[a].hex) == 0;
        let checks = if manual {
            moved && manual_checks(class(battle, a)) && !at_skirmishers
        } else {
            checks_before_shock(class(battle, a), moved, class(battle, d), frontal)
        };
        if checks {
            all_frontal_light &= frontal && class(battle, a) == MoveClass::LightInfantry;
            // (the manual: a charging elephant shakes them too, Hannibal's
            // and Caesar's)
            if manual && battle.hannibal() && class(battle, a) == MoveClass::Elephant {
                foreign_elephant = true;
            }
            checked = true;
            all_light &= class(battle, a) == MoveClass::LightInfantry;
            chariot |= class(battle, a) == MoveClass::Chariot;
            // Hannibal's: charging Celts shake the defenders as chariots do
            // (the unit's name has the word); and elephants other than the
            // Carthaginians' shake Carthaginian elephants (🟡)
            if battle.hannibal() && !battle.caesar() {
                chariot |= battle.units[a].name.contains("Celts");
                foreign_elephant |= class(battle, a) == MoveClass::Elephant && battle.units[a].nation != CARTHAGINIAN;
            }
            let u = &battle.units[a];
            let (h, rout) = tq_check_unit(u.hits, u.tq as i32, rng.next() % 10 + battle.tq_burden(a));
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
            // (the manual: phalanx and heavy infantry, Caesar's cohorts too,
            // charged frontally by light infantry only)
            let steady = matches!(c, MoveClass::Phalanx | MoveClass::HeavyInfantry) || (manual && battle.caesar() && battle.units[d].legion.is_some());
            if steady && if manual { all_frontal_light } else { all_light } {
                still.push(d);
                continue;
            }
            let u = &battle.units[d];
            let elephants = foreign_elephant && (manual || (c == MoveClass::Elephant && battle.units[d].nation == CARTHAGINIAN));
            let roll = rng.next() % 10 + (chariot || elephants) as u32;
            let (h, rout) = tq_check_unit(u.hits, u.tq as i32, roll + battle.tq_burden(d));
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
    let berm = |h: Hex| map.is_some_and(|m| m.caesar && h.x >= 0 && h.y >= 0 && m.hex_features(h.x as usize, h.y as usize).contains(&4));
    // the manual's rules (an option): no size ratio where elephants or
    // chariots fight; the terrain shift best for the defender
    let beasts = fighting.iter().chain(&standing).any(|&u| matches!(class(battle, u), MoveClass::Elephant | MoveClass::Chariot));
    let terrain = match map.filter(|_| manual) {
        Some(m) if battle.caesar() => caesar_terrain_shift(battle, m, &fighting, &standing),
        Some(m) => manual_terrain_shift(battle, m, &fighting, &standing),
        None => 0,
    };
    let d_class = defending_class(a_class, &def_classes).unwrap_or(def_classes[0]);
    // the defender missile-capable (the original's argument for its
    // superiority exceptions); an attacker held in the zone of another enemy
    // than those it attacks gains no superiority (the manual's rule, read as
    // the original's flag 🟡)
    let defender_missile = standing.iter().any(|&d| class(battle, d) == d_class && battle.units[d].missile.is_some());
    let held = map.is_some_and(|m| {
        fighting.iter().any(|&a| {
            let side = battle.units[a].side;
            battle.hexes(a).iter().any(|&h| {
                (0..battle.units.len()).any(|e| {
                    let o = &battle.units[e];
                    o.side != side
                        && o.state == crate::battle::UnitState::Fighting
                        && !standing.contains(&e)
                        && class(battle, e) != MoveClass::Skirmisher
                        && crate::movement::zoc_of(battle, m, e).contains(&h)
                })
            })
        })
    });
    let shock = Shock {
        attacker: a_class,
        defender: d_class,
        angle: attack_angle,
        attacker_size: side_size(&fighting.iter().map(|&a| (class(battle, a), battle.units[a].size as i32)).collect::<Vec<_>>()),
        defender_size: side_size(&standing.iter().map(|&d| (class(battle, d), battle.units[d].size as i32)).collect::<Vec<_>>()),
        attacker_cataphract: gboh_formats::units::cataphract(battle.units[lead].type_id),
        defender_cataphract: standing.iter().any(|&d| gboh_formats::units::cataphract(battle.units[d].type_id)),
        defender_missile,
        no_superiority: held,
        // (Caesar's berms, `FUN_004aee30`: the attacker on one and the
        // defender not −2, the other way round −3)
        column_shift: terrain + match (berm(battle.units[lead].hex), berm(battle.units[standing[0]].hex)) {
            (true, false) => -2,
            (false, true) => -3,
            _ => 0,
        }
            // a rallied attacker a column worse, a rallied defender a column
            // better for the attacker (Caesar's code tests the attacker
            // twice, so they cancel: fixed)
            - battle.units[lead].rallied as i32
            + battle.units[standing[0]].rallied as i32,
        // (Caesar's: Alexander's superiority order, no nations' rules)
        hannibal: battle.hannibal() && !battle.caesar(),
        caesar: battle.caesar(),
        stack_superiority: battle.hannibal() && !battle.caesar() && !battle.options.no_stacked_superiority,
        attacker_nation: battle.units[lead].nation,
        defender_nation: battle.units[standing[0]].nation,
        manual,
        moved: attackers.iter().any(|&(a, m)| m && fighting.contains(&a)),
        all_phalanx: fighting.iter().all(|&a| class(battle, a) == MoveClass::Phalanx),
        // (Alexander's and Hannibal's manuals: any defending heavy cavalry
        // 2L; Caesar's: cataphracts only)
        defender_heavy_cavalry: !battle.caesar() && standing.iter().any(|&d| matches!(class(battle, d), MoveClass::HeavyCavalry | MoveClass::RomanCavalry)),
    };
    let shock = if manual && beasts { Shock { attacker_size: 1, defender_size: 1, ..shock } } else { shock };
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
    // (the manual: with a skirmisher on either side, the others' hits halve)
    let skirmish = manual && fighting.iter().chain(&standing).any(|&u| class(battle, u) == MoveClass::Skirmisher);
    let halved = |u: usize, h: i32| if skirmish && class(battle, u) != MoveClass::Skirmisher { h / 2 } else { h };
    let a_hits: Vec<i32> = fighting.iter().zip(&a_hits).map(|(&u, &h)| halved(u, h)).collect();
    let d_hits: Vec<i32> = standing.iter().zip(&d_hits).map(|(&u, &h)| halved(u, h)).collect();
    for (&u, &h) in fighting.iter().zip(&a_hits).chain(standing.iter().zip(&d_hits)) {
        hit(&mut out, battle, u, h);
        if routed(battle, u) {
            out.routed.push(u);
        }
    }

    // leader casualties (attacking side rolls first; the manual: "a leader
    // is never involved in shock combat")
    if manual {
        return out;
    }
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
            rallied: false,
            done: false,
        }
    }

    #[test]
    fn the_best_angle_leads() {
        let t = Hex::new(5, 5);
        let n = t.neighbours();
        // target faces 3 (front n[1], n[2]); flank n[0], rear n[4]
        let mk = |ty, hex| unit(ty, 2, hex, 6, 6, 6);
        let mut b = Battle { units: vec![unit(31, 1, t, 3, 6, 6), mk(31, n[1]), mk(2, n[0])], ..Default::default() };
        // a skirmisher on the flank leads, whatever the order: the flank
        assert_eq!(leading_attacker(&b, &[1, 2], 0), (2, 1));
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
        let mut b = Battle { units: vec![unit(31, 2, Hex::new(5, 5), 3, 6, 6), unit(31, 1, Hex::new(6, 5), 7, 6, 6)], ..Default::default() };
        b.units[1].hits = 6;
        let out = resolve(&mut b, &[(0, true)], &[1], None, &mut BorlandRand::new(1));
        assert_eq!(out.eliminated, vec![1]);
    }

    #[test]
    fn a_shock_deals_hits_to_both_sides() {
        let mut b = Battle { units: vec![unit(31, 2, Hex::new(5, 5), 3, 6, 9), unit(31, 1, Hex::new(6, 5), 7, 6, 9)], ..Default::default() };
        let out = resolve(&mut b, &[(0, false)], &[1], None, &mut BorlandRand::new(1));
        assert!(b.units[0].hits > 0 && b.units[1].hits > 0, "{out:?}");
    }
}
