//! Command: which units an activated leader can order.
//!
//! The manual: "A leader can order any friendly combat units within his
//! command range"; units of his own group cost one order, others two (see
//! `orders::CommandPoints`). The game also flags the units of each friendly
//! leader within his range, as far as that leader's own range reaches: at
//! Hydaspes, turn 1, the Attalus phalanx (11 hexes from Alexander, range 9)
//! is flagged through Antigenes. With both rules the flags match the game.
//! Range is traced as a path from the leader (`FUN_004b37a8` with
//! `FUN_00565687`): one point a hex, two for a level up or down, none past
//! a steeper climb, enemy units, or an empty hex in an enemy zone of
//! control (it is reached, but the trace goes no further). The manual's
//! rule that a leader in an enemy zone cannot give orders is not in the
//! original (a rule option: see `Game::held`).

use crate::activation::Activation;
use crate::battle::Battle;
use crate::hex::Hex;
use gboh_formats::crt::BorlandRand;
use gboh_formats::map::Map;
use gboh_formats::scn::battle_index;

/// The hexes within `leader`'s command range (the original's command set,
/// built at activation by `FUN_00562ce7`).
pub fn command_set(battle: &Battle, map: &Map, leader: usize) -> std::collections::HashMap<Hex, i32> {
    let l = &battle.units[leader];
    let range = l.leader.map_or(0, |r| r.command_range) as i32;
    let enemy_at = |h: Hex| battle.units.iter().enumerate().any(|(i, u)| u.side != l.side && u.on_field() && battle.hexes(i).contains(&h));
    let friend_at = |h: Hex| battle.units.iter().enumerate().any(|(i, u)| u.side == l.side && u.on_field() && battle.hexes(i).contains(&h));
    let mut best = std::collections::HashMap::from([(l.hex, 0)]);
    let mut heap = std::collections::BinaryHeap::from([(0i32, l.hex.x, l.hex.y)]);
    while let Some((neg, x, y)) = heap.pop() {
        let (spent, here) = (-neg, Hex::new(x, y));
        if best.get(&here).is_some_and(|&b| b < spent) {
            continue;
        }
        // no further from an empty hex in an enemy zone
        if here != l.hex && !friend_at(here) && crate::movement::in_enemy_zoc(battle, map, l.side, here) {
            continue;
        }
        for d in 0..6 {
            let Some(c) = crate::movement::leader_step_cost(map, here, d) else { continue };
            let to = here.neighbours()[d];
            let total = spent + c;
            if total > range || enemy_at(to) || best.get(&to).is_some_and(|&b| b <= total) {
                continue;
            }
            best.insert(to, total);
            heap.push((-total, to.x, to.y));
        }
    }
    best
}

/// Units (not leaders) the leader at `leader` can order, by index: those
/// with a hex in his command set, and those of the group of a friendly
/// leader in it that are in that leader's own set. Routed units count (the
/// only order they take is a rally).
pub fn commanded(battle: &Battle, map: &Map, leader: usize) -> Vec<usize> {
    let l = &battle.units[leader];
    if l.leader.is_none() {
        return Vec::new();
    }
    let set = command_set(battle, map, leader);
    let in_set = |u: usize, set: &std::collections::HashMap<Hex, i32>| battle.hexes(u).iter().any(|h| set.contains_key(h));
    let subordinates: Vec<(usize, std::collections::HashMap<Hex, i32>)> =
        battle.leaders().filter(|&s| s != leader && battle.units[s].side == l.side && in_set(s, &set)).map(|s| (s, command_set(battle, map, s))).collect();
    (0..battle.units.len())
        .filter(|&i| {
            let u = &battle.units[i];
            u.leader.is_none()
                && u.on_field()
                && u.side == l.side
                && (in_set(i, &set) || subordinates.iter().any(|(s, ss)| u.group == battle.units[*s].group && in_set(i, ss)))
        })
        .collect()
}

/// Scenario indices of the units flagged when `scn` starts: those the
/// first leader of turn 1 commands (`seed` drives the activation draw where
/// elite initiative does not decide it).
pub fn opening_flags(scn: &gboh_formats::scn::Scenario, map: &gboh_formats::map::Map, seed: u32) -> Vec<usize> {
    let battle = Battle::from_scenario(scn, map);
    let mut activation = Activation::new(&battle, battle_index(&scn.map_file));
    let mut rng = BorlandRand::new(seed);
    activation.begin_turn(&battle, &mut rng).map_or_else(Vec::new, |l| commanded(&battle, map, l).into_iter().map(|i| battle.units[i].source).collect())
}
