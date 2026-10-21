//! Command: which units an activated leader can order.
//!
//! The manual: "A leader can order any friendly combat units within his
//! command range"; units of his own group cost one order, others two (see
//! `orders::CommandPoints`). The game also flags the units of each friendly
//! leader within his range, as far as that leader's own range reaches: at
//! Hydaspes, turn 1, the Attalus phalanx (11 hexes from Alexander, range 9)
//! is flagged through Antigenes. With both rules the flags match the game.
//! 🟡 A leader who starts in an enemy zone of control cannot give orders
//! (manual; not applied yet).

use crate::activation::Activation;
use crate::battle::Battle;
use gboh_formats::crt::BorlandRand;
use gboh_formats::scn::battle_index;

/// Units (not leaders) the leader at `leader` can order, by index.
pub fn commanded(battle: &Battle, leader: usize) -> Vec<usize> {
    let l = &battle.units[leader];
    let Some(ratings) = l.leader else { return Vec::new() };
    let in_range = |u: &crate::battle::Unit, from: &crate::battle::Unit, range: u16| u.hex.distance(from.hex) <= range as i32;
    // friendly leaders within his range
    let subordinates: Vec<usize> = battle
        .leaders()
        .filter(|&s| s != leader && battle.units[s].side == l.side && in_range(&battle.units[s], l, ratings.command_range))
        .collect();
    (0..battle.units.len())
        .filter(|&i| {
            let u = &battle.units[i];
            u.leader.is_none()
                && u.state == crate::battle::UnitState::Fighting
                && u.side == l.side
                && (in_range(u, l, ratings.command_range)
                    || subordinates.iter().any(|&s| {
                        let su = &battle.units[s];
                        u.group == su.group && in_range(u, su, su.leader.map_or(0, |r| r.command_range))
                    }))
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
    activation.begin_turn(&battle, &mut rng).map_or_else(Vec::new, |l| {
        commanded(&battle, l).into_iter().map(|i| battle.units[i].source).collect()
    })
}
