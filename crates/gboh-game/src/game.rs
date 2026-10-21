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
use std::collections::HashMap;
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
    /// Whether the active leader has moved (once per activation).
    pub leader_moved: bool,
    /// Battle index (see `gboh_formats::scn::battle_index`), for the
    /// battles' own victory rules.
    pub battle_index: Option<usize>,
    /// Pelium: points of Macedonian units out across the Apsos (1000 more
    /// once a leader follows them).
    pub exited: u16,
    /// The active leader's group move, once granted: how far it may go.
    pub group_move: Option<GroupMove>,
    /// Per leader: whether he tried a group move in his last activation,
    /// and how many activations running he has (the original's `+0xf6` and
    /// `+0xee`).
    group_tried: HashMap<usize, bool>,
    group_streak: HashMap<usize, u8>,
    /// Shocks designated by the active leader.
    pub shocks: Vec<Designation>,
}

/// The result of a group move attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupMove {
    /// As far as the slowest unit allows.
    Full,
    /// One hex only.
    OneHex,
    Failed,
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
            leader_moved: false,
            battle_index: battle_index(&scn.map_file),
            exited: 0,
            group_move: None,
            group_tried: HashMap::new(),
            group_streak: HashMap::new(),
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
        self.leader_moved = false;
        self.group_move = None;
        if let Some(l) = leader {
            // consecutive group moves get easier (`FUN_00565fa0`)
            let streak = if self.group_tried.insert(l, false).unwrap_or(false) {
                self.group_streak.get(&l).copied().unwrap_or(0).saturating_add(1)
            } else {
                0
            };
            self.group_streak.insert(l, streak);
        }
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

    /// Rally a routed unit with the active leader (the order must already
    /// be paid for): on success its hits drop and it fights again; on
    /// failure it routs terminally (out of the battle). Returns success.
    pub fn rally(&mut self, unit: usize) -> bool {
        let Some(l) = self.activation.active() else { return false };
        let u = &self.battle.units[unit];
        if u.state != UnitState::Routed {
            return false;
        }
        let (tq, hits) = (u.tq, u.hits);
        let ini = self.battle.units[l].leader.map_or(0, |r| r.initiative);
        let rng = &mut self.rng;
        match crate::rout::rally(tq, hits, ini, || rng.next() % 10) {
            Some(h) => {
                let u = &mut self.battle.units[unit];
                u.hits = h;
                u.state = UnitState::Fighting;
                self.spend(unit, true);
                true
            }
            None => {
                self.battle.units[unit].state = UnitState::Gone;
                false
            }
        }
    }

    /// Recover cohesion for `unit` with the active leader (the order must
    /// already be paid for); returns the hits removed.
    pub fn recover(&mut self, unit: usize) -> i32 {
        let Some(l) = self.activation.active() else { return 0 };
        let ini = self.battle.units[l].leader.map_or(0, |r| r.initiative);
        let n = orders::recover(&mut self.battle, unit, ini, &mut self.rng);
        self.spend(unit, true);
        n
    }

    /// The active leader's group: the units sharing his group number that
    /// are still in the battle (in command range or not).
    pub fn group(&self, leader: usize) -> Vec<usize> {
        let l = &self.battle.units[leader];
        (0..self.battle.units.len())
            .filter(|&u| {
                let o = &self.battle.units[u];
                o.leader.is_none() && o.side == l.side && o.group == l.group && o.on_field()
            })
            .collect()
    }

    /// Why the active leader cannot group move now, if he cannot (the
    /// manual's conditions): he has given orders, or a unit of his group is
    /// routed, faces another way, is out of his command or within two hexes
    /// of an enemy.
    pub fn group_move_refused(&self) -> Option<&'static str> {
        let l = self.activation.active()?;
        if self.points.as_ref().is_some_and(|p| p.used > 0) || self.leader_moved {
            return Some("orders already given");
        }
        let group = self.group(l);
        let Some(&first) = group.first() else { return Some("no group") };
        let facing = self.battle.units[first].facing;
        let orderable = self.orderable();
        for &u in &group {
            let o = &self.battle.units[u];
            if o.state != UnitState::Fighting {
                return Some("a unit is routed");
            }
            if o.facing != facing {
                return Some("units face different ways");
            }
            if !orderable.contains(&u) {
                return Some("a unit is out of command");
            }
            let near = self.battle.units.iter().any(|e| {
                e.side != o.side && e.on_field() && self.battle.hexes(u).iter().any(|h| h.distance(e.hex) <= 2)
            });
            if near {
                return Some("a unit is near the enemy");
            }
        }
        None
    }

    /// Try a group move (the original's `FUN_00564572`): `rand() % 100`
    /// below `(initiative − 2) × 20 + 30` grants a full group move, below
    /// `(initiative − 2) × 20 + 50` a one-hex one (both capped at 100). It
    /// uses all the leader's orders and rules out momentum. A leader who
    /// tried one in his previous activation too rolls 30 lower; after two or
    /// more such activations running he cannot fail.
    pub fn try_group_move(&mut self) -> Option<GroupMove> {
        let l = self.activation.active()?;
        if self.group_move_refused().is_some() {
            return None;
        }
        let ini = self.battle.units[l].leader.map_or(0, |r| r.initiative) as i32;
        let full = ((ini - 2) * 20 + 30).min(100);
        let one = ((ini - 2) * 20 + 50).min(100);
        let mut roll = (self.rng.next() % 100) as i32;
        match self.group_streak.get(&l).copied().unwrap_or(0) {
            0 => {}
            1 => roll = (roll - 30).max(0),
            _ => roll = 0,
        }
        self.group_tried.insert(l, true);
        let result = if roll < full {
            GroupMove::Full
        } else if roll < one {
            GroupMove::OneHex
        } else {
            GroupMove::Failed
        };
        if let Some(p) = &mut self.points {
            p.used = p.max;
        }
        self.group_move = Some(result);
        Some(result)
    }

    /// Where a granted group move can take the leader: steps through the
    /// two front sides of the group's facing, every unit making each step
    /// within its movement and never into an enemy zone of control or a
    /// hex held by a unit outside the group.
    pub fn group_move_options(&self) -> Vec<crate::hex::Hex> {
        self.group_move_paths().into_keys().collect()
    }

    fn group_move_paths(&self) -> HashMap<crate::hex::Hex, Vec<usize>> {
        use crate::movement::{MoveClass, forward, in_enemy_zoc, step_cost};
        let mut out = HashMap::new();
        let (Some(l), Some(mode)) = (self.activation.active(), self.group_move) else { return out };
        if mode == GroupMove::Failed || self.leader_moved {
            return out;
        }
        let group = self.group(l);
        let Some(&first) = group.first() else { return out };
        let Some(dirs) = forward(self.battle.units[first].facing) else { return out };
        let max_steps = if mode == GroupMove::OneHex { 1 } else { 99 };
        let start = self.battle.units[l].hex;
        // breadth first over step sequences, tracking each unit's spending
        let mut frontier = vec![(start, Vec::<usize>::new(), group.iter().map(|&u| (u, self.battle.units[u].hex, 0)).collect::<Vec<_>>())];
        while let Some((lh, path, units)) = frontier.pop() {
            if path.len() >= max_steps {
                continue;
            }
            'dir: for &d in &dirs {
                let nl = lh.neighbours()[d];
                if !crate::movement::on_map(&self.map, nl) || out.contains_key(&nl) || nl == start {
                    continue;
                }
                let mut moved = Vec::new();
                for &(u, h, spent) in &units {
                    let o = &self.battle.units[u];
                    let class = MoveClass::of_type(o.type_id).unwrap_or(MoveClass::HeavyInfantry);
                    let Some(c) = step_cost(&self.map, class, h, d) else { continue 'dir };
                    let to = h.neighbours()[d];
                    let blocked = self.battle.units.iter().enumerate().any(|(i, x)| {
                        x.on_field() && x.hex == to && !group.contains(&i) && i != l
                    });
                    if spent + c > o.max_mp || blocked || in_enemy_zoc(&self.battle, o.side, to) {
                        continue 'dir;
                    }
                    moved.push((u, to, spent + c));
                }
                let mut p = path.clone();
                p.push(d);
                out.insert(nl, p.clone());
                frontier.insert(0, (nl, p, moved));
            }
        }
        out
    }

    /// Carry out a granted group move to `to` (one of
    /// `group_move_options`): the leader and every unit of his group take
    /// the same steps.
    pub fn do_group_move(&mut self, to: crate::hex::Hex) -> bool {
        let Some(l) = self.activation.active() else { return false };
        let Some(path) = self.group_move_paths().remove(&to) else { return false };
        let group = self.group(l);
        for &d in &path {
            for &u in &group {
                let h = self.battle.units[u].hex;
                self.battle.units[u].hex = h.neighbours()[d];
            }
        }
        for &u in &group {
            self.battle.units[u].mp = 0;
        }
        self.battle.units[l].hex = to;
        self.leader_moved = true;
        true
    }

    /// Where the active leader can move, if he has not yet.
    pub fn leader_options(&self) -> Vec<crate::hex::Hex> {
        match self.activation.active() {
            Some(l) if !self.leader_moved => {
                crate::movement::leader_moves(&self.battle, &self.map, l, self.battle.units[l].mp)
            }
            _ => Vec::new(),
        }
    }

    /// Move the active leader to `to` (one of `leader_options`, the order
    /// already paid for); false if he cannot.
    pub fn move_leader(&mut self, to: crate::hex::Hex) -> bool {
        let Some(l) = self.activation.active() else { return false };
        if !self.leader_options().contains(&to) || to == self.battle.units[l].hex {
            return false;
        }
        self.battle.units[l].hex = to;
        self.leader_moved = true;
        true
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
                o.side != u.side && o.leader.is_none() && o.on_field()
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
        // a leader may try for momentum only if he gave an order this
        // activation (the original's `CanAttemptMomentum`, `FUN_00564374`;
        // nor after a group move)
        self.pelium_exits();
        let gave_orders = self.points.as_ref().is_some_and(|p| p.used > 0) && self.group_move.is_none();
        let next = self.activation.next(&self.battle, momentum && gave_orders, &mut self.rng);
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
        for _ in 0..40 {
            let before = self.snapshot();
            let mut host = crate::ai_host::AiHost::new(self);
            let ran = machine.orders(&name, &mut host)?;
            let finished = host.finished;
            log.append(&mut host.log);
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
                    .filter(|&u| u != elephant && self.battle.units[u].on_field() && self.battle.hexes(u).contains(&next))
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
        // the Macedonian army is the original's first side
        let mac = self.armies.iter().position(|a| a.0 == 2).unwrap_or(0);
        let enemy = 1 - mac;
        if self.exited >= 1000 {
            return Verdict::Won(mac);
        }
        let over = |a: usize| self.rout_points(a) as i32 - self.armies[a].1 as i32;
        match (over(mac) >= 0, over(enemy) >= 0) {
            (true, false) => return Verdict::Won(enemy),
            (false, true) => return Verdict::Won(mac),
            // both broken: the one less far past its level wins; a tie goes
            // against the Macedonians (the code; the manual says for them)
            (true, true) => return Verdict::Won(if over(mac) < over(enemy) { mac } else { enemy }),
            (false, false) => {}
        }
        // a killed Alexander loses the battle, a killed Darius too
        let killed = |name: &str| {
            self.battle.units.iter().any(|u| u.leader.is_some() && u.state == UnitState::Gone && u.name.contains(name))
        };
        if killed("Alexander") {
            return Verdict::Won(enemy);
        }
        if killed("Darius") {
            return Verdict::Won(mac);
        }
        // turn limits: the Macedonians lose if no army has broken by then
        if self.turn_limit().is_some_and(|n| self.activation.turn >= n) {
            return Verdict::Won(enemy);
        }
        Verdict::Continue
    }

    /// The battle's turn limit (the original's `FUN_0054d45b`): ten turns
    /// at Chaeronea, Lyginus, Pelium, Granicus, the Jaxartes, Arigaeum and
    /// Hydaspes, five at Samarkand, seven at Paraitacene; none at Issus and
    /// Gaugamela.
    pub fn turn_limit(&self) -> Option<u16> {
        match self.battle_index? {
            0 | 1 | 2 | 3 | 7 | 8 | 9 => Some(10),
            6 => Some(5),
            10 => Some(7),
            _ => None,
        }
    }

    /// Pelium (`FUN_0054d85c`): Macedonian units standing unrouted on the
    /// west edge (column 0, rows 5–20) leave across the Apsos, scoring
    /// their TQ; a leader leaves only once 50 points have, scoring 1000
    /// (the Macedonian win).
    fn pelium_exits(&mut self) {
        if self.battle_index != Some(2) {
            return;
        }
        for u in 0..self.battle.units.len() {
            let o = &self.battle.units[u];
            let at_exit = self.battle.hexes(u).iter().any(|h| h.x == 0 && (5..=20).contains(&h.y));
            if o.nation != 2 || o.state != UnitState::Fighting || !at_exit {
                continue;
            }
            if o.leader.is_some() {
                if self.exited < 50 {
                    continue;
                }
                self.exited += 1000;
            } else {
                self.exited += o.tq;
            }
            self.battle.units[u].state = UnitState::Exited;
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
            battle_index: None,
            exited: 0,
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
            leader_moved: false,
            group_move: None,
            group_tried: HashMap::new(),
            group_streak: HashMap::new(),
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
    fn group_moves_carry_the_group() {
        let r = LeaderRatings { charisma: 1, command_range: 6, initiative: 7, personal_combat: 1 };
        let mut g = game(vec![
            unit(4, 2, Hex::new(2, 6), Some(r)),
            unit(31, 2, Hex::new(3, 5), None),
            unit(31, 2, Hex::new(3, 7), None),
        ]);
        g.activation = Activation::new(&g.battle, None);
        g.begin_turn();
        assert_eq!(g.group_move_refused(), None);
        // initiative 7: a full group move is certain
        assert_eq!(g.try_group_move(), Some(GroupMove::Full));
        let options = g.group_move_options();
        let to = *options.iter().max_by_key(|h| h.x).unwrap();
        assert!(to.x > 2);
        let before: Vec<Hex> = g.battle.units.iter().map(|u| u.hex).collect();
        assert!(g.do_group_move(to));
        // everyone moved, keeping the formation's spacing
        let dx = g.battle.units[1].hex.x - before[1].x;
        assert!(dx > 0 && g.battle.units[2].hex.x - before[2].x == dx);
        assert_eq!(g.points.as_ref().unwrap().left(), 0);
    }

    #[test]
    fn killed_alexander_and_turn_limits() {
        let r = LeaderRatings { charisma: 1, command_range: 6, initiative: 7, personal_combat: 1 };
        let mut g = game(vec![unit(0, 2, Hex::new(2, 2), Some(r)), unit(31, 1, Hex::new(9, 9), None)]);
        g.battle.units[0].name = "Alexander III".into();
        assert_eq!(g.verdict(), Verdict::Continue);
        g.battle_index = Some(6);
        g.activation.turn = 5;
        assert_eq!(g.verdict(), Verdict::Won(1));
        g.activation.turn = 0;
        g.battle.units[0].state = UnitState::Gone;
        assert_eq!(g.verdict(), Verdict::Won(1));
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
