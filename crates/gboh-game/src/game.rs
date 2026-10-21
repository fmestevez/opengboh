//! The turn loop: activation, orders, shock, routs and victory (see
//! `docs/formats.md`, "Turn sequence").
//!
//! Status 🟡: wires the ported rules together in the manual's order. Missile
//! resupply, advancing after combat, orderly withdrawal and leaders'
//! wounds are not applied yet.

use crate::activation::Activation;
use crate::battle::{Battle, UnitState};
use crate::combat::{LeaderCasualty, rout_points};
use crate::command::commanded;
use crate::orders::{self, CommandPoints, resupply};
use crate::rout::{Flight, flee};
use crate::shock::{self, Outcome};
use gboh_formats::crt::BorlandRand;
use gboh_formats::map::Map;
use gboh_formats::scn::{Scenario, battle_index};

/// Unit positions, facings, hits and states; shocks; orders used.
type Snapshot = (Vec<(crate::hex::Hex, u8, i32, UnitState)>, usize, u16);

/// A designated shock: attackers (with whether each moved) and defenders.
pub type Designation = (Vec<(usize, bool)>, Vec<usize>);

/// A battle being played.
pub struct Game {
    pub battle: Battle,
    pub map: Map,
    pub activation: Activation,
    pub rng: BorlandRand,
    /// `(nation, rout level)` for the home and visitor armies.
    pub armies: [(u8, u16); 2],
    /// The active leader's command points.
    pub points: Option<CommandPoints>,
    /// Units resupplied at this activation (they may not be ordered).
    pub resupplied: Vec<usize>,
    /// Units the active leader may still order, fixed at activation.
    orderable_now: Vec<usize>,
    /// Shocks designated by the active leader.
    pub shocks: Vec<Designation>,
}

/// How a battle stands at the end of a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Continue,
    /// The army (index into [`Game::armies`]) that won.
    Won(usize),
}

impl Game {
    pub fn new(scn: &Scenario, map: Map, seed: u32) -> Self {
        let battle = Battle::from_scenario(scn, &map);
        let activation = Activation::new(&battle, battle_index(&scn.map_file));
        Self {
            battle,
            map,
            activation,
            rng: BorlandRand::new(seed),
            armies: [
                (scn.home.nation, scn.home.rout_level as u16),
                (scn.visitor.nation, scn.visitor.rout_level as u16),
            ],
            points: None,
            resupplied: Vec::new(),
            orderable_now: Vec::new(),
            shocks: Vec::new(),
        }
    }

    /// Resume the current turn after the leaders `done` have been active
    /// (e.g. when starting from a saved game); returns the next leader.
    pub fn resume_turn(&mut self, done: &[usize]) -> Option<usize> {
        let l = self.activation.resume(&self.battle, done, &mut self.rng);
        self.activate(l)
    }

    /// Start the next turn; returns its first leader.
    pub fn begin_turn(&mut self) -> Option<usize> {
        let l = self.activation.begin_turn(&self.battle, &mut self.rng);
        self.activate(l)
    }

    fn activate(&mut self, leader: Option<usize>) -> Option<usize> {
        self.points = leader.map(|l| CommandPoints::new(&self.battle, l));
        self.shocks.clear();
        for u in &mut self.battle.units {
            u.fired = false;
        }
        self.resupplied = match leader {
            Some(l) => {
                let units = commanded(&self.battle, l);
                resupply(&mut self.battle, &units, &mut self.rng)
            }
            None => Vec::new(),
        };
        // who may take orders is fixed now (the original's FUN_005235b8):
        // units in command that have not moved this turn and are not done
        self.orderable_now = leader.map_or_else(Vec::new, |l| commanded(&self.battle, l));
        let b = &self.battle;
        let resupplied = &self.resupplied;
        self.orderable_now.retain(|&u| b.units[u].mp == b.units[u].max_mp && !b.units[u].done && !resupplied.contains(&u));
        leader
    }

    /// `unit` may take no more orders this activation; `done` also keeps it
    /// from being ordered again this turn (after a shock or a recovery).
    pub fn spend(&mut self, unit: usize, done: bool) {
        self.orderable_now.retain(|&u| u != unit);
        if done {
            self.battle.units[unit].done = true;
        }
    }

    /// Units the active leader can order.
    pub fn orderable(&self) -> Vec<usize> {
        self.orderable_now.clone()
    }

    /// Designate a shock for resolution when the leader finishes.
    pub fn designate(&mut self, attackers: Vec<(usize, bool)>, defenders: Vec<usize>) {
        self.shocks.push((attackers, defenders));
    }

    /// Enemy units in `unit`'s front hexes (its zone of control), either
    /// hex for two-hex units.
    pub fn in_front(&self, unit: usize) -> Vec<usize> {
        let u = &self.battle.units[unit];
        let Some(dirs) = crate::movement::forward(u.facing) else { return Vec::new() };
        let front: Vec<crate::hex::Hex> =
            self.battle.hexes(unit).iter().flat_map(|h| dirs.map(|d| h.neighbours()[d])).collect();
        (0..self.battle.units.len())
            .filter(|&e| {
                let o = &self.battle.units[e];
                o.side != u.side && o.leader.is_none() && o.state != UnitState::Gone
                    && self.battle.hexes(e).iter().any(|h| front.contains(h))
            })
            .collect()
    }

    /// Designate `unit` to shock `target` (the manual's rules): the target
    /// must be in its front; it also attacks every other enemy there not
    /// already attacked this phase, and joins a shock already on its
    /// target. Skirmishers cannot shock (the Agrianians can, and so can
    /// anyone against chariots or routed units). Returns false if refused.
    pub fn designate_shock(&mut self, unit: usize, target: usize) -> bool {
        let u = &self.battle.units[unit];
        let front = self.in_front(unit);
        let t = &self.battle.units[target];
        if u.state == UnitState::Routed {
            return false;
        }
        let skirmisher = crate::movement::MoveClass::of_type(u.type_id) == Some(crate::movement::MoveClass::Skirmisher);
        let soft = matches!(t.type_id, 22 | 23) || t.state == UnitState::Routed;
        if !front.contains(&target) || (skirmisher && u.type_id != 42 && !soft) {
            return false;
        }
        let moved = u.mp < u.max_mp;
        if let Some(s) = self.shocks.iter_mut().find(|(_, d)| d.contains(&target)) {
            s.0.push((unit, moved));
            return true;
        }
        let taken: Vec<usize> = self.shocks.iter().flat_map(|(_, d)| d.clone()).collect();
        let mut defenders = vec![target];
        defenders.extend(front.into_iter().filter(|e| *e != target && !taken.contains(e)));
        self.shocks.push((vec![(unit, moved)], defenders));
        true
    }

    /// Finish the active leader: resolve his shocks, rout the losers two
    /// hexes, and activate the next leader (`None` when the turn is over).
    pub fn finish_leader(&mut self, momentum: bool) -> (Vec<Outcome>, Option<usize>) {
        let mut outcomes = Vec::new();
        for (attackers, defenders) in std::mem::take(&mut self.shocks) {
            let target_hex = defenders.first().map(|&d| self.battle.units[d].hex);
            let out = shock::resolve(&mut self.battle, &attackers, &defenders, &mut self.rng);
            for &u in &out.eliminated {
                self.battle.units[u].state = UnitState::Gone;
            }
            for &u in &out.routed {
                self.rout(u, 2);
            }
            if let Some((l, effect)) = out.leader {
                self.leader_casualty(l, effect);
            }
            if let Some(h) = target_hex {
                self.advance(&attackers, &defenders, h);
            }
            outcomes.push(out);
        }
        let before = self.activation.active();
        let next = self.activation.next(&self.battle, momentum, &mut self.rng);
        if let (Some(l), true) = (next, next == before) {
            self.momentum_refresh(l);
        }
        (outcomes, self.activate(next))
    }

    /// A successful momentum: the leader's units get their movement back
    /// and may be ordered again; one that has already moved this turn takes
    /// a cohesion hit the first time it moves again ("ordered again"). 🟡
    /// Modelled on the original's commentary at Hydaspes; the original
    /// restores movement and clears "done" in `FUN_0052b2e2`, but the
    /// message that triggers it is not traced.
    fn momentum_refresh(&mut self, leader: usize) {
        for u in commanded(&self.battle, leader) {
            let unit = &mut self.battle.units[u];
            if unit.mp < unit.max_mp {
                unit.reordered = true;
            }
            unit.mp = unit.max_mp;
            unit.turned = false;
            unit.done = false;
        }
    }

    /// Apply a leader casualty: finished for the turn; wounded (command
    /// range and fourth rating down 2 — 🟡 the amount the original's wound
    /// message carries; a second wound kills); killed.
    fn leader_casualty(&mut self, leader: usize, effect: LeaderCasualty) {
        let u = &mut self.battle.units[leader];
        match effect {
            LeaderCasualty::Finished => self.activation.finish(leader),
            LeaderCasualty::Wounded if !u.wounded => {
                u.wounded = true;
                if let Some(r) = &mut u.leader {
                    r.command_range = r.command_range.saturating_sub(2);
                    r.personal_combat = r.personal_combat.saturating_sub(2);
                }
            }
            _ => u.state = UnitState::Gone,
        }
    }

    /// After a shock that cleared the defenders, one attacker advances into
    /// the primary defender's hex if it is empty and next to no other
    /// enemy: the highest TQ among those still fighting (🟡 the manual: the
    /// superior attacker first; the original's rule is not traced).
    fn advance(&mut self, attackers: &[(usize, bool)], defenders: &[usize], hex: crate::hex::Hex) {
        let cleared = defenders.iter().all(|&d| self.battle.units[d].state != UnitState::Fighting || self.battle.units[d].hex != hex);
        let empty = self.battle.units.iter().all(|u| u.state == UnitState::Gone || u.hex != hex);
        if !cleared || !empty {
            return;
        }
        let side = attackers.first().map(|&(a, _)| self.battle.units[a].side);
        let threatened = hex.neighbours().iter().any(|n| {
            self.battle.units.iter().any(|u| Some(u.side) != side && u.state == UnitState::Fighting && u.hex == *n)
        });
        if threatened {
            return;
        }
        let mover = attackers
            .iter()
            .map(|&(a, _)| a)
            .filter(|&a| self.battle.units[a].state == UnitState::Fighting && !self.battle.units[a].two_hex())
            .max_by_key(|&a| self.battle.units[a].tq);
        if let Some(a) = mover {
            self.battle.units[a].hex = hex;
        }
    }

    /// Let a script give the active leader's orders, as the original does:
    /// one `Orders for` call per order, repeated until the script calls
    /// `FinishLeader`, the leader has no points left, or a call changes
    /// nothing. Returns the script's debug output; the leader still has to
    /// be finished with [`Game::finish_leader`].
    pub fn ai_orders(&mut self, machine: &mut crate::ai::Machine) -> Result<Vec<String>, crate::ai::Error> {
        let Some(l) = self.activation.active() else { return Ok(Vec::new()) };
        let name = self.battle.units[l].name.clone();
        let mut log = Vec::new();
        let mut leader_moved = false;
        for _ in 0..40 {
            let before = self.snapshot();
            let mut host = crate::ai_host::AiHost::new(self);
            host.leader_moved = leader_moved;
            let ran = machine.orders(&name, &mut host)?;
            let (finished, moved) = (host.finished, host.leader_moved);
            log.append(&mut host.log);
            leader_moved = moved;
            let spent = self.points.as_ref().is_none_or(|p| p.finished());
            if !ran || finished || spent || self.snapshot() == before {
                break;
            }
        }
        Ok(log)
    }

    /// What an order can change, to notice when the AI stops making
    /// progress.
    fn snapshot(&self) -> Snapshot {
        (
            self.battle.units.iter().map(|u| (u.hex, u.facing, u.hits, u.state)).collect(),
            self.shocks.len(),
            self.points.as_ref().map_or(0, |p| p.used),
        )
    }

    /// A routed elephant rampages, then is out of the battle (the original's
    /// `FUN_00544b65`): up to four legs; each rolls `rand() % 10`, 7–9
    /// ending the rampage, else charging three hexes N (0, 1), NE (2), SE
    /// (3), S (4), SW (5) or NW (6), until it runs off the map. Every unit
    /// in a hex it enters takes 1 cohesion hit if the elephant comes in
    /// through its front, 2 otherwise (cavalry, lancers and chariots always
    /// 2; routed units, chariots and oxybeles none); cavalry and leaders
    /// may withdraw first, and a leader who cannot is killed. Units it
    /// breaks rout. Returns the units it hit.
    pub fn rampage(&mut self, elephant: usize) -> Vec<usize> {
        use crate::movement::MoveClass::*;
        self.battle.units[elephant].state = UnitState::Routed;
        let mut here = self.battle.units[elephant].hex;
        let mut hit = Vec::new();
        'legs: for _ in 0..4 {
            let side = match self.rng.next() % 10 {
                0 | 1 => 1,
                2 => 2,
                3 => 4,
                4 => 5,
                5 => 6,
                6 => 8,
                _ => break,
            };
            for _ in 0..3 {
                let Some(next) = crate::battle::neighbour(here, side).filter(|&h| crate::movement::on_map(&self.map, h)) else {
                    break 'legs;
                };
                let at: Vec<usize> = (0..self.battle.units.len())
                    .filter(|&u| u != elephant && self.battle.units[u].state != UnitState::Gone && self.battle.hexes(u).contains(&next))
                    .collect();
                for u in at {
                    let o = self.battle.units[u].clone();
                    let class = crate::movement::MoveClass::of_type(o.type_id);
                    if o.leader.is_some() {
                        if !orders::withdraw(&mut self.battle, &self.map, u, elephant, &mut self.rng) {
                            self.battle.units[u].state = UnitState::Gone;
                        }
                        continue;
                    }
                    let horse = matches!(class, Some(HeavyCavalry | LightCavalry | Lancer));
                    if horse && orders::withdraw(&mut self.battle, &self.map, u, elephant, &mut self.rng) {
                        continue;
                    }
                    if o.state == UnitState::Routed || matches!(class, Some(Chariot | Oxybeles)) {
                        continue;
                    }
                    let frontal = crate::shock::angle(o.hex, o.facing, here) == 0;
                    let n = if frontal && !horse { 1 } else { 2 };
                    self.battle.units[u].hits += n;
                    hit.push(u);
                    let o = &self.battle.units[u];
                    if o.hits >= o.tq as i32 {
                        self.rout(u, 2);
                    }
                }
                here = next;
            }
        }
        self.battle.units[elephant].state = UnitState::Gone;
        hit
    }

    /// Rout a unit: flee `steps` hexes, or leave the battle.
    pub fn rout(&mut self, unit: usize, steps: u32) {
        if crate::movement::MoveClass::of_type(self.battle.units[unit].type_id) == Some(crate::movement::MoveClass::Elephant) {
            self.rampage(unit);
            return;
        }
        self.battle.units[unit].state = UnitState::Routed;
        let (path, gone) = match flee(&self.battle, &self.map, unit, steps) {
            Flight::Moved(p) => (p, false),
            Flight::Gone(p) => (p, true),
        };
        // friendly units routed through take a cohesion hit (manual)
        let side = self.battle.units[unit].side;
        for o in 0..self.battle.units.len() {
            let u = &self.battle.units[o];
            if o != unit && u.side == side && u.state == UnitState::Fighting && path.contains(&u.hex) {
                self.battle.units[o].hits += 1;
            }
        }
        if gone {
            self.battle.units[unit].state = UnitState::Gone;
        } else if let Some(&h) = path.last() {
            self.battle.units[unit].hex = h;
        }
    }

    /// End the turn: routed units flee their full movement allowance, then
    /// the armies' rout points are compared with their levels.
    pub fn end_turn(&mut self) -> Verdict {
        for u in 0..self.battle.units.len() {
            if self.battle.units[u].state == UnitState::Routed {
                let mp = self.battle.units[u].max_mp.max(0) as u32;
                self.rout(u, mp);
            }
        }
        for u in &mut self.battle.units {
            u.mp = u.max_mp;
            u.turned = false;
            u.reordered = false;
            u.reorder_hit = false;
            u.done = false;
        }
        self.activation.turn += 1;
        self.verdict()
    }

    /// Rout points an army has lost (units out of the battle).
    pub fn rout_points(&self, army: usize) -> u16 {
        let nation = self.armies[army].0;
        self.battle
            .units
            .iter()
            .filter(|u| u.side == nation && u.state == UnitState::Gone)
            .map(|u| rout_points(u.type_id, u.tq, u.leader.map_or(0, |r| r.initiative)))
            .sum()
    }

    /// An army that has reached its rout level withdraws. If both have, the
    /// one less far past its level wins; on a tie the Macedonians (manual).
    pub fn verdict(&self) -> Verdict {
        let over = |a: usize| self.rout_points(a) as i32 - self.armies[a].1 as i32;
        match (over(0) >= 0, over(1) >= 0) {
            (false, false) => Verdict::Continue,
            (true, false) => Verdict::Won(1),
            (false, true) => Verdict::Won(0),
            (true, true) => {
                let (a, b) = (over(0), over(1));
                if a < b || (a == b && self.armies[0].0 == 2) { Verdict::Won(0) } else { Verdict::Won(1) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::{LeaderRatings, Unit};
    use crate::hex::Hex;

    fn unit(type_id: usize, side: u8, hex: Hex, leader: Option<LeaderRatings>) -> Unit {
        Unit {
            source: 0,
            name: String::new(),
            type_id,
            nation: side,
            side,
            group: 0,
            hex,
            facing: 3,
            leader,
            retreat: if side == 2 { 7 } else { 3 },
            max_mp: 5,
            mp: 5,
            turned: false,
            size: 6,
            tq: 6,
            hits: 0,
            state: UnitState::Fighting,
            missile: None,
            fired: false,
            wounded: false,
            reordered: false,
            reorder_hit: false,
            done: false,
        }
    }

    fn game(units: Vec<Unit>) -> Game {
        let battle = Battle { units };
        let activation = Activation::new(&battle, None);
        Game {
            battle,
            map: Map {
                name: String::new(),
                width: 12,
                height: 12,
                hex_id_origin: 1000,
                unk_b: 1,
                elevation: vec![1; 144],
                cells: vec![Vec::new(); (2 * 12 + 2) * (4 * 12 + 1)],
            },
            activation,
            rng: BorlandRand::new(1),
            armies: [(2, 10), (1, 6)],
            points: None,
            resupplied: Vec::new(),
            orderable_now: Vec::new(),
            shocks: Vec::new(),
        }
    }

    #[test]
    fn a_turn_runs_through_every_leader() {
        let r = LeaderRatings { charisma: 1, command_range: 4, initiative: 3, personal_combat: 1 };
        let mut g = game(vec![
            unit(4, 2, Hex::new(2, 2), Some(r)),
            unit(5, 1, Hex::new(9, 9), Some(r)),
            unit(31, 2, Hex::new(3, 2), None),
        ]);
        let mut leaders = vec![g.begin_turn().unwrap()];
        while let (_, Some(l)) = g.finish_leader(false) {
            leaders.push(l);
        }
        leaders.sort();
        assert_eq!(leaders, vec![0, 1]);
        assert_eq!(g.end_turn(), Verdict::Continue);
        assert_eq!(g.activation.turn, 1);
    }

    #[test]
    fn losing_rout_points_ends_the_battle() {
        let mut g = game(vec![unit(31, 1, Hex::new(5, 5), None), unit(31, 2, Hex::new(6, 5), None)]);
        // the visitor's hoplites (TQ 6) reach its level of 6
        g.battle.units[0].state = UnitState::Gone;
        assert_eq!(g.verdict(), Verdict::Won(0));
    }

    #[test]
    fn winners_advance_into_the_vacated_hex() {
        let mut g = game(vec![unit(31, 2, Hex::new(5, 5), None), unit(31, 1, Hex::new(6, 5), None)]);
        g.battle.units[1].state = UnitState::Gone;
        g.advance(&[(0, true)], &[1], Hex::new(6, 5));
        assert_eq!(g.battle.units[0].hex, Hex::new(6, 5));
    }

    #[test]
    fn shocks_take_in_everyone_in_front() {
        // attacker at (4,4) facing 3: front hexes NE and SE
        let n = Hex::new(4, 4).neighbours();
        let mut g = game(vec![
            unit(31, 2, Hex::new(4, 4), None),
            unit(31, 1, n[1], None),
            unit(31, 1, n[2], None),
            unit(31, 1, n[4], None),
        ]);
        assert!(g.designate_shock(0, 1));
        assert_eq!(g.shocks[0].1, vec![1, 2]);
        // not behind
        assert!(!g.designate_shock(0, 3));
    }

    #[test]
    fn momentum_lets_moved_units_go_again_at_a_cost() {
        let r = LeaderRatings { charisma: 1, command_range: 9, initiative: 3, personal_combat: 1 };
        let mut g = game(vec![unit(4, 2, Hex::new(2, 2), Some(r)), unit(31, 2, Hex::new(4, 4), None)]);
        g.battle.units[1].mp = 1;
        g.momentum_refresh(0);
        assert_eq!(g.battle.units[1].mp, 5);
        let m = g.map.clone();
        crate::orders::move_to(&mut g.battle, &m, 1, Hex::new(5, 4), &mut BorlandRand::new(1)).unwrap();
        assert_eq!(g.battle.units[1].hits, 1);
        // only once a turn
        crate::orders::move_to(&mut g.battle, &m, 1, Hex::new(6, 4), &mut BorlandRand::new(1)).unwrap();
        assert_eq!(g.battle.units[1].hits, 1);
    }

    #[test]
    fn routed_elephants_rampage_and_leave() {
        let mut g = game(vec![unit(23, 1, Hex::new(6, 6), None)]);
        // surround it so any direction hits someone
        for n in Hex::new(6, 6).neighbours() {
            g.battle.units.push(unit(31, 2, n, None));
        }
        g.battle.units[0].tq = 6;
        let mut any = false;
        for seed in 1..20 {
            let mut h = g.battle.clone();
            std::mem::swap(&mut h, &mut g.battle);
            g.rng = BorlandRand::new(seed);
            any |= !g.rampage(0).is_empty();
            assert_eq!(g.battle.units[0].state, UnitState::Gone);
            std::mem::swap(&mut h, &mut g.battle);
        }
        assert!(any);
    }

    #[test]
    fn wounds_and_deaths() {
        let r = LeaderRatings { charisma: 1, command_range: 5, initiative: 3, personal_combat: 3 };
        let mut g = game(vec![unit(4, 2, Hex::new(2, 2), Some(r))]);
        g.leader_casualty(0, LeaderCasualty::Wounded);
        assert_eq!(g.battle.units[0].leader.unwrap().command_range, 3);
        g.leader_casualty(0, LeaderCasualty::Wounded);
        assert_eq!(g.battle.units[0].state, UnitState::Gone);
    }

    #[test]
    fn routed_units_flee_at_the_end_of_the_turn() {
        let mut g = game(vec![unit(31, 2, Hex::new(6, 5), None)]);
        g.battle.units[0].state = UnitState::Routed;
        g.end_turn();
        assert!(g.battle.units[0].hex.x < 6 || g.battle.units[0].state == UnitState::Gone);
    }
}
