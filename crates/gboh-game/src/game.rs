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
use std::collections::HashMap;

/// Unit positions, facings, hits and states; shocks; orders used.
type Snapshot = (Vec<(crate::hex::Hex, u8, i32, UnitState)>, usize, u16);

/// A designated shock: attackers (with whether each moved) and defenders.
pub type Designation = (Vec<(usize, bool)>, Vec<usize>);

/// A battle being played.
/// Hannibal's Roman nation (scenario byte).
const ROMAN: u8 = 6;

/// Hannibal's battles in its table's order, by map file: Metaurus,
/// Bagradas, Cannae, Cynoscephalae, Zama, Trebbia, Baecula, Ilipa,
/// Beneventum, the Great Plains, Magnesia, Trasimene, Dertosa.
const HANNIBAL_BATTLES: [&str; 13] =
    ["METAURUS", "BAGRADAS", "CANNAE", "CYNO", "ZAMA", "TREBBIA", "BAECULA", "ILIPA", "BENEVENT", "GPLAINS", "MAGNESIA", "TRASIMEN", "DERTOSA"];

/// Per Hannibal battle: the (0-based) turn that ends it when turn limits
/// are on, and the army (0 home, 1 visitor) it then goes to: the original's
/// result 1 is the visitor's, 0x10001 the home army's, as the manual's
/// victory conditions show for every battle (the Romans, the visitor,
/// holding out at Bagradas and Trasimene; the home army elsewhere).
const HANNIBAL_TURN_LIMITS: [(u16, usize); 13] = [(14, 0), (7, 1), (9, 0), (7, 0), (7, 0), (7, 0), (14, 0), (11, 0), (5, 0), (5, 0), (9, 0), (2, 1), (7, 1)];

/// The facing half round from `facing` (2 ↔ 6, 3 ↔ 7, 4 ↔ 8).
fn opposite(facing: u8) -> u8 {
    match facing {
        2 => 6,
        3 => 7,
        4 => 8,
        6 => 2,
        7 => 3,
        8 => 4,
        f => f,
    }
}

/// Rout points a unit is worth ([`rout_points`]); with the manuals'
/// figures as an option: Alexander's Agrianians 2, and "Alexander (CH)" at
/// Chaeronea (the young Alexander leading the cavalry) a leader's × 5;
/// Hannibal × 10; Caesar's artillery its TQ.
fn unit_rout_points(options: &crate::options::Options, u: &crate::battle::Unit) -> u16 {
    let initiative = u.leader.map_or(0, |r| r.initiative);
    if options.manual_victory {
        use gboh_formats::units::{CAESAR, HANNIBAL};
        if u.type_id == 42 {
            return 2;
        }
        if u.type_id == 0 && u.name.contains("(CH)") {
            return initiative * 5;
        }
        if (HANNIBAL..CAESAR).contains(&u.type_id) && u.leader.is_some() && u.name.contains("Hannibal") {
            return initiative * 10;
        }
        if gboh_formats::units::hannibal_class(u.type_id) == Some(gboh_formats::units::Class::Artillery) {
            return u.tq;
        }
    }
    rout_points(u.type_id, u.tq, initiative)
}

/// Caesar's battles in its table's order, by map file: Dyrrhachium,
/// Pharsalus, Ruspina, Thapsus, Munda, Vercellae, Chaeronea, Bibracte,
/// Sabis, Rhine.
const CAESAR_BATTLES: [&str; 10] = ["DYRRHACH", "PHARSAL", "RUSPINA", "THAPSUS", "MUNDA", "VERCELL", "CHAERON", "BIBRAC", "SABIS", "RHINE"];

/// Per Caesar battle: the (0-based) turn that ends it when turn limits are
/// on, for the visitor (`FUN_00555709`'s result 1, as the manual's victory
/// conditions show: the Caesarians must win by then); none at Dyrrhachium
/// and Ruspina.
const CAESAR_TURN_LIMITS: [Option<u16>; 10] = [None, Some(7), None, Some(5), Some(11), Some(5), Some(5), Some(5), Some(5), Some(4)];

/// The battle number of a Caesar map file.
fn caesar_battle(map_file: &str) -> Option<usize> {
    let stem = map_file.split('.').next().unwrap_or("").to_ascii_uppercase();
    CAESAR_BATTLES.iter().position(|b| *b == stem)
}

/// The battle number of a Hannibal map file.
fn hannibal_battle(map_file: &str) -> Option<usize> {
    let stem = map_file.split('.').next().unwrap_or("").to_ascii_uppercase();
    HANNIBAL_BATTLES.iter().position(|b| *b == stem)
}

/// Whose rules a battle is fought by: Alexander's, or Hannibal's (its
/// legions, pursuit, group orders, elite initiative...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rules {
    #[default]
    Alexander,
    Hannibal,
    /// Caesar's: Hannibal's engine without its nations' and battles' own
    /// rules, with its own classes, terrain and battles.
    Caesar,
}

impl Rules {
    /// Hannibal's engine (Hannibal's or Caesar's rules).
    pub fn newer(self) -> bool {
        self != Rules::Alexander
    }
}

#[derive(Clone)]
pub struct Game {
    pub battle: Battle,
    pub rules: Rules,
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
    /// What happened since a front end last took it.
    pub log: Vec<Happened>,
    /// Battle index (see `gboh_formats::scn::battle_index`), for the
    /// battles' own victory rules.
    pub battle_index: Option<usize>,
    /// Hannibal's battle number (its table's order, from the map file),
    /// and whether its turn limits apply (an option of its set-up).
    pub hannibal_battle: Option<usize>,
    pub turn_limits: bool,
    /// Caesar's battle number (its table's order).
    pub caesar_battle: Option<usize>,
    /// A *Collector's Edition* scenario's own turn limit: turns, and the
    /// army that must win by then (the other takes it).
    pub scenario_limit: Option<(u16, u8)>,
    /// Hannibal's Triarii tactical doctrine option.
    pub triarii_doctrine: bool,
    /// Hannibal's deployment phase, while on: its line (see
    /// [`Game::deployment_line`]).
    pub deployment: Option<(bool, i32)>,
    /// Hannibal's Cannae: whether the wind blows this turn.
    pub wind: bool,
    /// Hannibal's Metaurus: unreliable units (Ligurians, Gauls) that have
    /// proved themselves.
    reliable: Vec<usize>,
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
    /// Rout points of leaders killed and replaced from the ranks (they
    /// stay on the map), by nation.
    pub fallen_leaders: Vec<(u8, u16)>,
}

/// Something that happened on the battlefield, for a front end to show
/// (moves to animate, volleys to sound).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Happened {
    /// A unit (or leader) walked through these hexes, from where it was;
    /// `running` when routed.
    Moved {
        unit: usize,
        path: Vec<crate::hex::Hex>,
        running: bool,
    },
    Volley {
        shooter: usize,
        target: usize,
        weapon: crate::missile::Weapon,
        hit: bool,
    },
    /// A unit lost men: `count` cohesion hits taken on `hex`, of `kind`,
    /// facing `facing`, `hits` in all after them.
    Casualties {
        unit: usize,
        hex: crate::hex::Hex,
        facing: u8,
        kind: Casualty,
        count: i32,
        hits: i32,
    },
    /// A unit was destroyed where it stood (its men all fall).
    Eliminated {
        unit: usize,
        hex: crate::hex::Hex,
        facing: u8,
    },
    /// A unit broke and routs (its men throw down their arms).
    Routed {
        unit: usize,
    },
    /// A shock combat is fought (its casualties follow).
    Shock {
        attackers: Vec<usize>,
        defenders: Vec<usize>,
    },
}

/// How men fell (the unit art's corpse strips).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Casualty {
    Shock,
    Arrow,
    Trample,
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
        let mut activation = Activation::new(&battle, battle_index(&scn.map_file));
        activation.romans_idle_first_turn = scn.map_file.to_ascii_uppercase().starts_with("TRASIMEN");
        activation.caesarians_idle_first_turn = scn.layout == gboh_formats::scn::Layout::Caesar && caesar_battle(&scn.map_file) == Some(8);
        let mut map = map;
        map.caesar = matches!(scn.layout, gboh_formats::scn::Layout::Caesar | gboh_formats::scn::Layout::Editor);
        let rules = match scn.layout {
            gboh_formats::scn::Layout::Alexander => Rules::Alexander,
            gboh_formats::scn::Layout::Hannibal => Rules::Hannibal,
            // (the Collector's Edition's, by its engine, Caesar's)
            gboh_formats::scn::Layout::Caesar | gboh_formats::scn::Layout::Editor => Rules::Caesar,
        };
        Self {
            battle,
            rules,
            map,
            activation,
            rng: BorlandRand::new(seed),
            armies: [(scn.home.nation, scn.home.rout_level), (scn.visitor.nation, scn.visitor.rout_level)],
            points: None,
            resupplied: Vec::new(),
            orderable_now: Vec::new(),
            leader_moved: false,
            log: Vec::new(),
            battle_index: battle_index(&scn.map_file),
            hannibal_battle: hannibal_battle(&scn.map_file),
            caesar_battle: if scn.layout == gboh_formats::scn::Layout::Caesar { caesar_battle(&scn.map_file) } else { None },
            scenario_limit: scn.editor.as_ref().and_then(|e| e.turn_limit),
            turn_limits: false,
            triarii_doctrine: true,
            deployment: None,
            wind: false,
            reliable: Vec::new(),
            exited: 0,
            group_move: None,
            group_tried: HashMap::new(),
            group_streak: HashMap::new(),
            shocks: Vec::new(),
            fallen_leaders: Vec::new(),
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
        // Caesar's Vercellae (its manual, an option): the Gauls tire, a
        // point worse on TQ checks from turn 3, two from turn 5
        self.battle.heat = if self.battle.options.manual_victory && self.caesar_battle == Some(5) {
            match self.activation.turn {
                0 | 1 => 0,
                2 | 3 => 1,
                _ => 2,
            }
        } else {
            0
        };
        // Hannibal's Cannae: wind on seven turns in ten (from direction 2)
        if self.rules == Rules::Hannibal && self.hannibal_battle == Some(2) {
            self.wind = self.rng.random() % 10 < 7;
            self.battle.wind = self.wind;
        }
        let l = self.activation.begin_turn(&self.battle, &mut self.rng);
        self.activate(l)
    }

    /// Answer the elite initiative offered to a player's leader: taken,
    /// he stays active; declined, the turn's first leader is drawn as
    /// usual. Returns the active leader.
    pub fn answer_elite(&mut self, accept: bool) -> Option<usize> {
        if !self.activation.offered {
            return self.activation.active();
        }
        if accept {
            self.activation.offered = false;
            return self.activation.active();
        }
        let l = self.activation.decline(&self.battle, &mut self.rng);
        self.activate(l)
    }

    fn activate(&mut self, leader: Option<usize>) -> Option<usize> {
        // the free units' entry drawn first: they act now
        if std::mem::take(&mut self.activation.free_drawn) {
            self.free_units();
        }
        self.points = leader.map(|l| CommandPoints::new(&self.battle, l));
        self.leader_moved = false;
        self.group_move = None;
        if let Some(l) = leader {
            // consecutive group moves get easier (`FUN_00565fa0`)
            let streak = if self.group_tried.insert(l, false).unwrap_or(false) { self.group_streak.get(&l).copied().unwrap_or(0).saturating_add(1) } else { 0 };
            self.group_streak.insert(l, streak);
        }
        self.shocks.clear();
        // every activation, a successful momentum too (`FUN_0054b4eb`
        // broadcasting `BeginLeaderOrdersPhase`): every unit gets its
        // movement back and may take orders again (`FUN_0052b2e2`,
        // `FUN_00536b1a`); one that spent movement in an earlier order has
        // been ordered this turn, and pays for being ordered again
        for u in &mut self.battle.units {
            if u.mp < u.max_mp || u.fired {
                u.reordered = true;
            }
            u.mp = u.max_mp;
            u.done = false;
            u.turned = false;
            u.reorder_hit = false;
            u.fired = false;
        }
        // (Hannibal's Cannae wind is taken from the moves' search budget
        // and the missiles' range: see `Battle::into_wind`)
        self.resupplied = match leader {
            Some(l) => {
                let units = commanded(&self.battle, &self.map, l);
                let r = resupply(&mut self.battle, &units, &mut self.rng);
                // more than two cost the leader a command point
                if r.len() > 2
                    && let Some(p) = &mut self.points
                {
                    p.used += 1;
                }
                r
            }
            None => Vec::new(),
        };
        // who may take orders is fixed now (the original's FUN_005235b8):
        // units in command that have not moved this turn and are not done
        self.orderable_now = leader.map_or_else(Vec::new, |l| commanded(&self.battle, &self.map, l));
        let b = &self.battle;
        let resupplied = &self.resupplied;
        self.orderable_now.retain(|&u| b.units[u].mp == b.units[u].max_mp && !b.units[u].done && !resupplied.contains(&u));
        leader
    }

    /// Move `unit` to `to` (see `orders::move_to`), logging the hexes it
    /// walked through (up to where reaction fire may have stopped it).
    /// Hannibal's deployment line (`FUN_004aa5d7`): halfway between the two
    /// armies, across the axis of the retreat edges (rows if one army
    /// retreats north, else columns); `None` if they're within a hex of each
    /// other (no free deployment then). Returns whether it's a row, and its
    /// coordinate.
    pub fn deployment_line(&self) -> Option<(bool, i32)> {
        let edge = |nation: u8| self.battle.units.iter().find(|u| u.side == nation).map_or(0, |u| u.retreat);
        let (a, b) = (self.armies[0].0, self.armies[1].0);
        // the "low" army retreats north or west
        let (low, high) = if matches!(edge(a), 1 | 7) { (a, b) } else { (b, a) };
        let rows = edge(low) == 1 || edge(high) == 1;
        let coord = |u: &crate::battle::Unit| if rows { u.hex.y } else { u.hex.x };
        let placed = |n: u8| self.battle.units.iter().filter(move |u| u.side == n && u.on_field());
        let lo = placed(low).map(coord).max()?;
        let hi = placed(high).map(coord).min()?;
        (hi - lo > 1).then_some((rows, lo + (hi - lo) / 2))
    }

    /// Whether `unit` may be deployed on `hex` (`FUN_0053075f`): on its
    /// army's side of the deployment line fixed when the phase began
    /// (towards its retreat edge), not impassable, and empty.
    pub fn can_deploy(&self, unit: usize, hex: crate::hex::Hex) -> bool {
        let Some((_, line)) = self.deployment else { return false };
        let u = &self.battle.units[unit];
        let side_ok = match u.retreat {
            1 => hex.y < line,
            3 => hex.x > line,
            5 => hex.y > line,
            _ => hex.x < line,
        };
        let hexes: Vec<crate::hex::Hex> = std::iter::once(hex).chain(u.two_hex().then(|| crate::battle::flank(hex, u.facing)).flatten()).collect();
        let fits = hexes.len() == if u.two_hex() { 2 } else { 1 }
            && hexes.iter().all(|&h| {
                crate::movement::on_map(&self.map, h)
                    && !crate::movement::impassable(&self.map, h)
                    && (0..self.battle.units.len()).all(|o| o == unit || !self.battle.units[o].on_field() || !self.battle.hexes(o).contains(&h))
            });
        side_ok && fits
    }

    /// Put `unit` on `hex` during the deployment phase (see
    /// [`Game::can_deploy`]).
    pub fn deploy(&mut self, unit: usize, hex: crate::hex::Hex) -> bool {
        if !self.can_deploy(unit, hex) {
            return false;
        }
        self.battle.units[unit].hex = hex;
        true
    }

    /// Hannibal's backward movement (`FUN_00528241`): a unit steps back
    /// through its rear, two movement points at most, keeping its facing.
    /// The hexes it can reach that way.
    pub fn backward_options(&self, unit: usize) -> Vec<crate::hex::Hex> {
        let u = &self.battle.units[unit];
        if !(self.rules.newer() || self.battle.options.backward) || u.state != UnitState::Fighting || u.leader.is_some() || u.mp <= 0 {
            return Vec::new();
        }
        // (the wind by the unit's own facing: the original tested the
        // reversed one, fixed)
        let wind = self.battle.into_wind(unit) as i32;
        let mut b = self.battle.clone();
        b.units[unit].facing = opposite(u.facing);
        b.wind = false;
        crate::movement::highlighted(&b, &self.map, unit, u.mp.min(2) - wind)
    }

    /// Step `unit` back to `to` (see [`Game::backward_options`]).
    pub fn order_backward(&mut self, unit: usize, to: crate::hex::Hex) -> Result<Vec<(usize, i32)>, orders::Refused> {
        if !self.backward_options(unit).contains(&to) {
            return Err(orders::Refused::Unreachable);
        }
        let facing = self.battle.units[unit].facing;
        let mp = self.battle.units[unit].mp;
        let wind = std::mem::take(&mut self.battle.wind);
        let penalty = (wind && self.battle.units[unit].leader.is_none() && matches!(facing, 6 | 7)) as i32;
        self.battle.units[unit].facing = opposite(facing);
        self.battle.units[unit].mp = mp.min(2) - penalty;
        let r = self.order_move(unit, to);
        self.battle.wind = wind;
        self.battle.units[unit].facing = facing;
        if r.is_err() {
            self.battle.units[unit].mp = mp;
        }
        r
    }

    /// Hannibal's Metaurus (`FUN_0053c518`): its Ligurians and Gauls take an
    /// order only with a Roman unit within seven hexes, and the first time
    /// pass a TQ check or rout. Returns whether the unit may act.
    pub fn reliable(&mut self, unit: usize) -> Result<(), orders::Refused> {
        let u = &self.battle.units[unit];
        let doubtful = self.rules == Rules::Hannibal && self.hannibal_battle == Some(0) && (u.name.contains("Liguria") || u.name.contains("Gaul"));
        if !doubtful || self.reliable.contains(&unit) {
            return Ok(());
        }
        let here = u.hex;
        let romans = self.battle.units.iter().any(|o| o.nation == ROMAN && o.leader.is_none() && o.state == UnitState::Fighting && o.hex.distance(here) <= 7);
        if !romans {
            return Err(orders::Refused::Unreachable);
        }
        if self.rng.next() % 10 <= self.battle.units[unit].tq as u32 {
            self.reliable.push(unit);
            Ok(())
        } else {
            self.rout(unit, self.battle.units[unit].max_mp.max(0) as u32);
            Err(orders::Refused::Routed)
        }
    }

    pub fn order_move(&mut self, unit: usize, to: crate::hex::Hex) -> Result<Vec<(usize, i32)>, orders::Refused> {
        self.reliable(unit)?;
        let start = self.battle.units[unit].hex;
        let mp = self.battle.units[unit].mp;
        let path = crate::movement::path_to(&self.battle, &self.map, unit, mp, to).unwrap_or_default();
        let map = self.map.clone();
        let positions: Vec<crate::hex::Hex> = self.battle.units.iter().map(|u| u.hex).collect();
        let moved = orders::move_to(&mut self.battle, &map, unit, to, &mut self.rng)?;
        let fire = moved.fire;
        // units that withdrew leave any shock designated against them, and
        // are seen backing off
        for &w in &moved.withdrew {
            for (_, defenders) in self.shocks.iter_mut() {
                defenders.retain(|&d| d != w);
            }
            for (attackers, _) in self.shocks.iter_mut() {
                attackers.retain(|&(a, _)| a != w);
            }
            self.log.push(Happened::Moved { unit: w, path: vec![positions[w], self.battle.units[w].hex], running: false });
        }
        self.shocks.retain(|(a, d)| !a.is_empty() && !d.is_empty());
        // leaders hurt stepping away roll for a casualty (`FUN_00566549`:
        // finished, then wounded on d100 under 50, else killed)
        for &l in &moved.leaders_hit {
            self.log.push(Happened::Moved { unit: l, path: vec![positions[l], self.battle.units[l].hex], running: false });
            self.leader_casualty(l, LeaderCasualty::Finished, 0);
            let roll = self.rng.next() % 100;
            let effect = if roll < 50 { LeaderCasualty::Wounded } else { LeaderCasualty::Killed };
            self.leader_casualty(l, effect, 1);
        }
        let end = self.battle.units[unit].hex;
        let hits: i32 = fire.iter().map(|&(_, h)| h).sum();
        if hits > 0 {
            let (facing, total) = (self.battle.units[unit].facing, self.battle.units[unit].hits);
            self.log.push(Happened::Casualties { unit, hex: end, facing, kind: Casualty::Arrow, count: hits, hits: total });
        }
        let mut walked = vec![start];
        for h in path {
            walked.push(h);
            if h == end {
                break;
            }
        }
        self.log.push(Happened::Moved { unit, path: walked, running: false });
        Ok(fire)
    }

    /// `shooter` volleys at `target` (see `orders::volley`), logged.
    pub fn order_volley(&mut self, shooter: usize, target: usize) -> Result<i32, orders::Refused> {
        let weapon = self.battle.units[shooter].missile.map(|m| m.0);
        let cover = self.cover(target);
        let hits = orders::volley(&mut self.battle, shooter, target, cover, &mut self.rng)?;
        if let Some(weapon) = weapon {
            self.log.push(Happened::Volley { shooter, target, weapon, hit: hits > 0 });
        }
        if hits > 0 {
            let o = &self.battle.units[target];
            self.log.push(Happened::Casualties { unit: target, hex: o.hex, facing: o.facing, kind: Casualty::Arrow, count: hits, hits: o.hits });
            // (Caesar's manual: a routed unit hit by missiles is cut down;
            // an option)
            let o = &self.battle.units[target];
            if self.rules == Rules::Caesar && self.battle.options.manual_missiles && o.state == UnitState::Routed {
                let (hex, facing) = (o.hex, o.facing);
                self.battle.units[target].state = UnitState::Gone;
                self.log.push(Happened::Eliminated { unit: target, hex, facing });
            }
        }
        self.missile_leader_hit(target);
        Ok(hits)
    }

    /// The manual's woods (an option): shooting into them a point harder.
    fn cover(&self, target: usize) -> i32 {
        let h = self.battle.units[target].hex;
        let woods =
            self.battle.options.manual_missiles && crate::movement::on_map(&self.map, h) && self.map.hex_features(h.x as usize, h.y as usize).contains(&1);
        woods as i32
    }

    /// A leader standing with the target may be hit (the original's 16 in
    /// 100, the manuals' 15 as an option): then startled (finished) on 0..25
    /// of a hundred, wounded to 70, else killed; Alexander and Darius
    /// startled to 40, wounded to 85.
    fn missile_leader_hit(&mut self, target: usize) {
        let h = self.battle.units[target].hex;
        let side = self.battle.units[target].side;
        let Some(l) = self.battle.leaders().find(|&l| self.battle.units[l].hex == h && self.battle.units[l].side == side) else { return };
        let chance = if self.battle.options.manual_missiles { 15 } else { 16 };
        if self.rng.random() % 100 >= chance {
            return;
        }
        let king = {
            let n = &self.battle.units[l].name;
            n.contains("Alexander") || n.contains("Darius")
        };
        let (startled, wounded) = if king { (40, 85) } else { (25, 70) };
        match self.rng.random() % 100 {
            r if r < startled => self.leader_casualty(l, LeaderCasualty::Finished, 0),
            r if r < wounded => self.leader_casualty(l, LeaderCasualty::Wounded, 1),
            _ => self.leader_casualty(l, LeaderCasualty::Killed, 1),
        }
    }

    /// Whether group retreat, attack and restore cohesion may be given
    /// (Hannibal's and Caesar's, or Alexander's by option).
    pub fn group_orders_allowed(&self) -> bool {
        self.rules.newer() || self.battle.options.group_orders
    }

    /// Set the rule options the battle is fought by (see
    /// [`crate::options`]).
    pub fn set_options(&mut self, options: crate::options::Options) {
        self.battle.options = options;
        self.map.fast_roads = self.map.caesar || options.roads;
        self.map.manual_slopes = options.manual_movement.then_some(self.rules.newer());
    }

    /// Whether the active leader may try to rally `unit`: any he commands,
    /// but by Caesar's manual (a rule option) a Gallic leader only within
    /// two hexes of him.
    pub fn may_rally(&self, unit: usize) -> bool {
        let Some(l) = self.activation.active() else { return false };
        let (l, u) = (&self.battle.units[l], &self.battle.units[unit]);
        !(self.battle.options.manual_command && self.rules == Rules::Caesar && l.nation == 11 && l.hex.distance(u.hex) > 2)
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
        let (tq, hits, rallied) = (u.tq, u.hits, u.rallied);
        let ini = self.battle.units[l].leader.map_or(0, |r| r.initiative);
        let rng = &mut self.rng;
        match crate::rout::rally(tq, hits, ini, rallied, || rng.next() % 10) {
            Some(h) => {
                let caesar = self.rules == Rules::Caesar || self.battle.options.rally_mark;
                let u = &mut self.battle.units[unit];
                u.hits = h;
                u.state = UnitState::Fighting;
                // (Caesar's: marked for good, but its first and tenth
                // cohorts, `FUN_0058f6ce`)
                let exempt = u.legion.is_some() && u.name.contains('1');
                if caesar && !exempt {
                    u.rallied = true;
                }
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
        let n = orders::recover(&mut self.battle, &self.map, unit, ini, &mut self.rng);
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
        // (Hannibal's: no command test; refused only with an enemy beside)
        let hannibal = self.rules.newer();
        let reach = if hannibal { 1 } else { 2 };
        for &u in &group {
            let o = &self.battle.units[u];
            if o.state != UnitState::Fighting {
                return Some("a unit is routed");
            }
            if o.facing != facing {
                return Some("units face different ways");
            }
            if !hannibal && !orderable.contains(&u) {
                return Some("a unit is out of command");
            }
            let near = self.battle.units.iter().any(|e| e.side != o.side && e.on_field() && self.battle.hexes(u).iter().any(|h| h.distance(e.hex) <= reach));
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
        let manual = self.battle.options.manual_command;
        // (the manuals: at most 90 % on a first try, 20 % easier after a
        // failure; an option)
        let cap = if manual { 90 } else { 100 };
        let full = ((ini - 2) * 20 + 30).min(cap);
        let one = ((ini - 2) * 20 + 50).min(cap);
        // (Hannibal's roll on `random()`; its Roman leaders never fail)
        let hannibal = self.rules.newer();
        let mut roll = (if hannibal { self.rng.random() } else { self.rng.next() } % 100) as i32;
        match self.group_streak.get(&l).copied().unwrap_or(0) {
            0 => {}
            1 => roll = (roll - if manual { 20 } else { 30 }).max(0),
            _ => roll = 0,
        }
        // (Caesar's Romans, Caesarians and Pompeians alike; Caesar's manual:
        // every leader's group move succeeds, an option)
        let n = self.battle.units[l].nation;
        if (self.rules == Rules::Hannibal && n == ROMAN) || (self.rules == Rules::Caesar && (manual || matches!(n, ROMAN | 9 | 10))) {
            roll = 0;
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

    /// Hannibal's roll for a group order (`FUN_0056a817`), for the retreat,
    /// attack and restore cohesion orders as for the group move: success
    /// takes all the leader's orders and charges nothing for the units;
    /// failure ends his orders and counts towards the next roll's bonus.
    fn group_order_roll(&mut self) -> bool {
        let Some(l) = self.activation.active() else { return false };
        if self.points.as_ref().is_some_and(|p| p.used > 0) || self.leader_moved {
            return false;
        }
        let ini = self.battle.units[l].leader.map_or(0, |r| r.initiative) as i32;
        // (the group move's roll, whose "one hex" counts as success for
        // these: below base + 50)
        let pass = ((ini - 2) * 20 + 50).min(100);
        let mut roll = (self.rng.random() % 100) as i32;
        match self.group_streak.get(&l).copied().unwrap_or(0) {
            0 => {}
            1 => roll = (roll - 30).max(0),
            _ => roll = 0,
        }
        // (Romans never fail: Caesar's Caesarians and Pompeians too)
        let n = self.battle.units[l].nation;
        if (self.rules == Rules::Hannibal && n == ROMAN) || (self.rules == Rules::Caesar && matches!(n, ROMAN | 9 | 10)) {
            roll = 0;
        }
        if let Some(p) = &mut self.points {
            p.used = p.max;
        }
        if roll < pass {
            self.group_streak.insert(l, 0);
            true
        } else {
            *self.group_streak.entry(l).or_insert(0) += 1;
            false
        }
    }

    /// Each unit of the leader's group moves its full allowance as far as
    /// it can towards `toward(unit)` (the original's per-unit move).
    fn group_step(&mut self, units: &[usize], toward: impl Fn(&Self, usize) -> Option<crate::hex::Hex>) {
        for &u in units {
            if self.battle.units[u].state != UnitState::Fighting {
                continue;
            }
            let Some(target) = toward(self, u) else { continue };
            let here = self.battle.units[u].hex;
            let mp = self.battle.units[u].mp;
            let best = crate::movement::reachable(&self.battle, &self.map, u, mp)
                .into_iter()
                .filter(|(h, _)| h.distance(target) < here.distance(target))
                .min_by_key(|(h, c)| (h.distance(target), *c, h.x, h.y))
                .map(|(h, _)| h);
            if let Some(to) = best {
                let _ = self.order_move(u, to);
            }
        }
    }

    /// Hannibal's group retreat (`FUN_005704f0`): on a good roll every unit
    /// of the leader's group makes for his army's retreat edge.
    pub fn group_retreat(&mut self) -> bool {
        let Some(l) = self.activation.active() else { return false };
        if !self.group_order_roll() {
            return false;
        }
        let edge = self.battle.units[l].retreat;
        let (w, h) = (self.map.width as i32, self.map.height as i32);
        let group = self.group(l);
        self.group_step(&group, |g, u| {
            let at = g.battle.units[u].hex;
            Some(match edge {
                1 => crate::hex::Hex::new(at.x, 0),
                3 => crate::hex::Hex::new(w - 1, at.y),
                5 => crate::hex::Hex::new(at.x, h - 1),
                _ => crate::hex::Hex::new(0, at.y),
            })
        });
        for u in group {
            self.spend(u, false);
        }
        true
    }

    /// Hannibal's group attack (`FUN_00570bca`): on a good roll every unit
    /// of the group closes on its nearest enemy, and those that reach one
    /// attack it (🟡 the original also fires missiles where it can't).
    pub fn group_attack(&mut self) -> bool {
        let Some(l) = self.activation.active() else { return false };
        if !self.group_order_roll() {
            return false;
        }
        let group = self.group(l);
        self.group_step(&group, |g, u| {
            let (here, side) = (g.battle.units[u].hex, g.battle.units[u].side);
            g.battle.units.iter().filter(|o| o.side != side && o.leader.is_none() && o.on_field()).map(|o| o.hex).min_by_key(|h| (h.distance(here), h.x, h.y))
        });
        for &u in &group {
            let targets = self.in_front(u);
            if let Some(&t) = targets.first() {
                let _ = self.designate_shock(u, t);
            }
            self.spend(u, false);
        }
        true
    }

    /// Hannibal's group restore cohesion (`FUN_005715f6`): on a good roll
    /// every unit of the group with hits tries to recover.
    pub fn group_restore(&mut self) -> bool {
        let Some(l) = self.activation.active() else { return false };
        if !self.group_order_roll() {
            return false;
        }
        let ini = self.battle.units[l].leader.map_or(0, |r| r.initiative);
        for u in self.group(l) {
            if self.battle.units[u].hits > 0 && self.battle.units[u].state == UnitState::Fighting {
                orders::recover(&mut self.battle, &self.map, u, ini, &mut self.rng);
                self.spend(u, true);
            }
        }
        true
    }

    /// Whether the active leader may order a manipular line extension: a
    /// Roman (in Hannibal's rules) with legionaries in his group, before any
    /// other order.
    pub fn can_extend_line(&self) -> bool {
        let Some(l) = self.activation.active() else { return false };
        // (Caesar's: any Roman's, Caesarian's or Pompeian's)
        let romans: &[u8] = if self.rules == Rules::Caesar { &[ROMAN, 9, 10] } else { &[ROMAN] };
        self.rules.newer()
            && romans.contains(&self.battle.units[l].nation)
            && self.points.as_ref().is_none_or(|p| p.used == 0)
            && !self.leader_moved
            && self.group(l).iter().any(|&u| self.battle.units[u].legion.is_some())
    }

    /// Manipular line extension (Hannibal's `FUN_0056fe45`, Caesar's
    /// `FUN_0057130c`): no roll, the leader's whole activation. In the
    /// group's order, a maniple (a cohort) whose right-front hex holds one
    /// of the same nation, group and line (cohort type) steps into its
    /// left-front hex by the leader's facing if that is empty: a rear
    /// maniple standing behind-left of a front one comes up on its left, so
    /// the line extends to the left. Facings are kept; each step is made
    /// before the next unit is looked at. Returns how many stepped.
    pub fn extend_line(&mut self) -> usize {
        if !self.can_extend_line() {
            return 0;
        }
        let Some(l) = self.activation.active() else { return 0 };
        let group = self.group(l);
        let (nation, leader_facing) = (self.battle.units[l].nation, self.battle.units[l].facing);
        // the sides by facing: right-front of a unit, left-front of the leader
        let right_front = |f: u8| match f {
            2 => 2,
            3 => 4,
            4 => 5,
            6 => 6,
            7 => 8,
            _ => 1,
        };
        let left_front = match leader_facing {
            2 => 1,
            3 => 2,
            4 => 4,
            6 => 5,
            7 => 6,
            _ => 8,
        };
        let legion = |u: &crate::battle::Unit| crate::movement::MoveClass::of_type(u.type_id) == Some(crate::movement::MoveClass::Legion);
        let mut stepped = 0;
        for &u in &group {
            let o = &self.battle.units[u];
            // (the original skipped no routed unit: fixed)
            if !legion(o) || o.state != UnitState::Fighting {
                continue;
            }
            let (here, line, grp) = (o.hex, o.legion, o.group);
            let Some(h1) = crate::battle::neighbour(here, right_front(o.facing)) else { continue };
            let partner = self.battle.units.iter().find(|x| x.on_field() && x.hex == h1);
            if !partner.is_some_and(|v| v.leader.is_none() && v.nation == nation && legion(v) && v.legion == line && v.group == grp) {
                continue;
            }
            let Some(to) = crate::battle::neighbour(here, left_front) else { continue };
            // (nor into an enemy's zone, as the manual says: the original
            // didn't test it, fixed)
            let empty = crate::movement::on_map(&self.map, to)
                && (0..self.battle.units.len()).all(|x| !self.battle.units[x].on_field() || !self.battle.hexes(x).contains(&to))
                && !crate::movement::in_enemy_zoc(&self.battle, &self.map, self.battle.units[u].side, to);
            if empty {
                self.log.push(Happened::Moved { unit: u, path: vec![here, to], running: false });
                self.battle.units[u].hex = to;
                stepped += 1;
            }
        }
        if let Some(p) = &mut self.points {
            p.used = p.max;
        }
        for u in group {
            self.spend(u, false);
        }
        stepped
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
                    let blocked = self.battle.units.iter().enumerate().any(|(i, x)| x.on_field() && x.hex == to && !group.contains(&i) && i != l);
                    if spent + c > o.max_mp || blocked || in_enemy_zoc(&self.battle, &self.map, o.side, to) {
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
        let from = self.battle.units[l].hex;
        self.battle.units[l].hex = to;
        self.leader_moved = true;
        self.log.push(Happened::Moved { unit: l, path: vec![from, to], running: false });
        true
    }

    /// Where the active leader can move, if he has not yet.
    pub fn leader_options(&self) -> Vec<crate::hex::Hex> {
        match self.activation.active() {
            Some(l) if !self.leader_moved => crate::movement::leader_moves(&self.battle, &self.map, l, self.battle.units[l].mp),
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
        let from = self.battle.units[l].hex;
        self.battle.units[l].hex = to;
        self.leader_moved = true;
        self.log.push(Happened::Moved { unit: l, path: vec![from, to], running: false });
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

    /// Units the active leader can order (Hannibal's Triarii held back
    /// by their doctrine left out).
    pub fn orderable(&self) -> Vec<usize> {
        self.orderable_now.iter().copied().filter(|&u| !self.triarii_held(u)).collect()
    }

    /// Hannibal's Triarii tactical doctrine (an option, `FUN_0048c8cd`):
    /// Triarii stay put while a Roman legion unit stands within 6 hexes of
    /// them, unless an enemy other than elephants comes within 3.
    pub fn triarii_held(&self, unit: usize) -> bool {
        if self.rules != Rules::Hannibal || !self.triarii_doctrine {
            return false;
        }
        let u = &self.battle.units[unit];
        if u.nation != ROMAN || !gboh_formats::units::title(u.type_id).is_some_and(|t| t.contains("Triarii")) {
            return false;
        }
        let legion_near =
            self.battle.units.iter().any(|o| o.side == u.side && o.state == UnitState::Fighting && o.legion.is_some() && o.hex.distance(u.hex) <= 6);
        let threat = self.battle.units.iter().any(|o| {
            o.side != u.side
                && o.on_field()
                && o.leader.is_none()
                && crate::movement::MoveClass::of_type(o.type_id) != Some(crate::movement::MoveClass::Elephant)
                && o.hex.distance(u.hex) <= 3
        });
        legion_near && !threat
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
        let front: Vec<crate::hex::Hex> = self.battle.hexes(unit).iter().flat_map(|h| dirs.map(|d| h.neighbours()[d])).collect();
        (0..self.battle.units.len())
            .filter(|&e| {
                let o = &self.battle.units[e];
                o.side != u.side && o.leader.is_none() && o.on_field() && self.battle.hexes(e).iter().any(|h| front.contains(h))
            })
            .collect()
    }

    /// Whether `unit` may shock `target`: in its front, and not a
    /// skirmisher attacking anything but chariots or routed units.
    pub fn can_shock(&self, unit: usize, target: usize) -> bool {
        let u = &self.battle.units[unit];
        let t = &self.battle.units[target];
        if u.state == UnitState::Routed || u.side == t.side {
            return false;
        }
        let skirmisher = crate::movement::MoveClass::of_type(u.type_id) == Some(crate::movement::MoveClass::Skirmisher);
        let soft = gboh_formats::units::chariot(t.type_id) || t.state == UnitState::Routed;
        self.in_front(unit).contains(&target) && !(skirmisher && u.type_id != 42 && !soft)
    }

    /// Whether `shooter` may volley at `target` now (nothing is changed).
    pub fn can_volley(&self, shooter: usize, target: usize) -> bool {
        orders::volley(&mut self.battle.clone(), shooter, target, 0, &mut self.rng.clone()).is_ok()
    }

    /// Designate `unit` to shock `target` (the manual's rules): the target
    /// must be in its front; it also attacks every other enemy there not
    /// already attacked this phase, and joins a shock already on its
    /// target. Skirmishers cannot shock (the Agrianians can, and so can
    /// anyone against chariots or routed units). Returns false if refused.
    pub fn designate_shock(&mut self, unit: usize, target: usize) -> bool {
        if !self.can_shock(unit, target) {
            return false;
        }
        let front = self.in_front(unit);
        let u = &self.battle.units[unit];
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
            self.log.push(Happened::Shock { attackers: attackers.iter().map(|a| a.0).collect(), defenders: defenders.clone() });
            let out = shock::resolve(&mut self.battle, &attackers, &defenders, Some(&self.map), &mut self.rng);
            for &(u, h) in &out.hits {
                if h > 0 {
                    let o = &self.battle.units[u];
                    self.log.push(Happened::Casualties { unit: u, hex: o.hex, facing: o.facing, kind: Casualty::Shock, count: h, hits: o.hits });
                }
            }
            for &u in &out.eliminated {
                let o = &mut self.battle.units[u];
                o.state = UnitState::Gone;
                self.log.push(Happened::Eliminated { unit: u, hex: o.hex, facing: o.facing });
            }
            // (where the routed stood, for a pursuer that fails its roll)
            let stood: Vec<(usize, crate::hex::Hex)> = out.routed.iter().map(|&u| (u, self.battle.units[u].hex)).collect();
            for &u in &out.routed {
                self.rout(u, 2);
            }
            if let Some((l, effect)) = out.leader {
                self.leader_casualty(l, effect, 2);
            }
            if self.rules.newer() || self.battle.options.pursuit {
                self.pursue_or_advance(&attackers, &defenders, &stood, target_hex);
            } else if let Some(h) = target_hex {
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
        let _ = before;
        (outcomes, self.activate(next))
    }

    /// Whether the active leader may try for momentum when he finishes: he
    /// gave an order this activation, not a group move, and has tried fewer
    /// than twice (the original's `CanAttemptMomentum`).
    pub fn can_attempt_momentum(&self) -> bool {
        self.points.as_ref().is_some_and(|p| p.used > 0) && self.group_move.is_none() && self.activation.momentum_left()
    }

    /// Apply a leader casualty: finished for the turn; wounded
    /// (`FUN_00566251`: charisma, initiative, fourth rating and command
    /// range all down by `amount`, 2 from a shock and 1 stepping away; a
    /// second wound kills); killed (`FUN_00565b3d`: his rout points count
    /// for the enemy and a soldier from the ranks replaces him, range 4,
    /// initiative 2, finished this turn; Alexander and Darius are not
    /// replaced, their fall ends the battle). 🟡 The replacement keeps the
    /// leader's name (the original gives him one from its data).
    fn leader_casualty(&mut self, leader: usize, effect: LeaderCasualty, amount: u16) {
        let u = &mut self.battle.units[leader];
        match effect {
            LeaderCasualty::Finished => self.activation.finish(leader),
            LeaderCasualty::Wounded if !u.wounded => {
                u.wounded = true;
                if let Some(r) = &mut u.leader {
                    r.charisma = r.charisma.saturating_sub(amount);
                    r.initiative = r.initiative.saturating_sub(amount);
                    r.personal_combat = r.personal_combat.saturating_sub(amount);
                    r.command_range = r.command_range.saturating_sub(amount);
                }
            }
            _ => {
                let king = u.name.contains("Alexander") || u.name.contains("Darius");
                let points = unit_rout_points(&self.battle.options, u);
                if king {
                    u.state = UnitState::Gone;
                    let (hex, facing) = (u.hex, u.facing);
                    self.log.push(Happened::Eliminated { unit: leader, hex, facing });
                } else {
                    self.fallen_leaders.push((u.side, points));
                    if let Some(r) = &mut u.leader {
                        r.command_range = 4;
                        r.initiative = 2;
                    }
                    self.activation.finish(leader);
                }
            }
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
        let threatened =
            hex.neighbours().iter().any(|n| self.battle.units.iter().any(|u| Some(u.side) != side && u.state == UnitState::Fighting && u.hex == *n));
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

    /// Hannibal's after a shock (`FUN_004af008`'s end): for each defender
    /// that broke (not an elephant), the first attacker of horse that held
    /// pursues it; failing that, one attacker advances, once a shock.
    fn pursue_or_advance(&mut self, attackers: &[(usize, bool)], defenders: &[usize], stood: &[(usize, crate::hex::Hex)], target_hex: Option<crate::hex::Hex>) {
        use crate::movement::MoveClass;
        let mut used = Vec::new();
        let mut advanced = false;
        for &(d, at) in stood {
            let elephant = MoveClass::of_type(self.battle.units[d].type_id) == Some(MoveClass::Elephant);
            let pursuer = (!elephant)
                .then(|| {
                    attackers.iter().map(|&(a, _)| a).find(|&a| {
                        let o = &self.battle.units[a];
                        !used.contains(&a) && o.state == UnitState::Fighting && MoveClass::of_type(o.type_id).is_some_and(|c| c.horse())
                    })
                })
                .flatten();
            match pursuer {
                Some(p) => {
                    used.push(p);
                    if !self.pursue(p, d) {
                        // a failed roll: into the hex the routed held
                        let empty = self.battle.units.iter().all(|u| !u.on_field() || u.hex != at);
                        if empty && !self.battle.units[p].two_hex() {
                            self.battle.units[p].hex = at;
                        }
                    }
                }
                None if !advanced => {
                    if let Some(h) = target_hex {
                        self.advance(attackers, defenders, h);
                    }
                    advanced = true;
                }
                None => break,
            }
        }
        if stood.is_empty()
            && let Some(h) = target_hex
        {
            self.advance(attackers, defenders, h);
        }
    }

    /// Cavalry pursuit (Hannibal's `FUN_0057ef41`, `FUN_0057f86f`; Caesar's
    /// alike): a roll of `rand() % 10` (+2 against foot) of at least 4
    /// (heavy and barbarian cavalry), 6 (Roman cavalry, lancers) or 8 (light
    /// cavalry); not from a hex in an enemy's zone. The pursuer rides the
    /// original's hex line at the routed unit, stopping before an occupied
    /// hex and after one in an enemy's zone, its facing kept; it catches it
    /// on a roll of no more than its TQ if the unit is in its front then,
    /// with no other foe there. A caught unit checks at TQ 1: failing, foot
    /// are cut down, horse, chariots and elephants flee again with the
    /// pursuer after them (🟡 off the map it is out, not back as a free
    /// unit). Uncaught, or after a chase, enemy leaders beside the pursuer
    /// step away. Returns whether it pursued (false: it may advance).
    fn pursue(&mut self, p: usize, target: usize) -> bool {
        use crate::movement::MoveClass::*;
        let side = self.battle.units[p].side;
        if self.battle.units[target].state == UnitState::Gone || crate::movement::in_enemy_zoc(&self.battle, &self.map, side, self.battle.units[p].hex) {
            return true;
        }
        let threshold = match crate::movement::MoveClass::of_type(self.battle.units[p].type_id) {
            Some(HeavyCavalry | BarbarianCavalry) => 4,
            Some(RomanCavalry | Lancer) => 6,
            _ => 8,
        };
        let t_class = crate::movement::MoveClass::of_type(self.battle.units[target].type_id);
        let foot = !t_class.is_some_and(|c| c.horse() || matches!(c, Chariot | Elephant));
        let roll = (self.rng.next() % 10) as i32 + if foot { 2 } else { 0 };
        if roll < threshold {
            return false;
        }
        let occupied =
            |g: &Self, h: crate::hex::Hex| (0..g.battle.units.len()).any(|u| u != p && g.battle.units[u].on_field() && g.battle.hexes(u).contains(&h));
        // the hex line to the routed unit
        let goal = self.battle.units[target].hex;
        let from = self.battle.units[p].hex;
        let mut path = vec![from];
        for h in crate::movement::hex_line(&self.map, from, goal).into_iter().skip(1) {
            if occupied(self, h) {
                break;
            }
            path.push(h);
            if crate::movement::in_enemy_zoc(&self.battle, &self.map, side, h) {
                break;
            }
        }
        let end = *path.last().unwrap_or(&from);
        let facing = self.battle.units[p].facing;
        let front: Vec<crate::hex::Hex> = crate::movement::forward(facing).map_or(Vec::new(), |d| d.iter().map(|&k| end.neighbours()[k]).collect());
        let tq = self.battle.units[p].tq as u32;
        let caught = self.rng.next() % 10 <= tq
            && front.contains(&goal)
            && !front.iter().any(|&h| h != goal && self.battle.units.iter().any(|u| u.on_field() && u.nation != self.battle.units[p].nation && u.hex == h));
        if path.len() > 1 {
            self.log.push(Happened::Moved { unit: p, path: path.clone(), running: false });
            self.battle.units[p].hex = end;
        }
        if caught {
            // the caught unit's check at TQ 1 (fails on 2..9)
            if (self.rng.next() % 10).saturating_sub(1) == 0 {
                return true;
            }
            if foot {
                let o = &mut self.battle.units[target];
                o.state = UnitState::Gone;
                let (hex, facing) = (o.hex, o.facing);
                self.log.push(Happened::Eliminated { unit: target, hex, facing });
            } else {
                // it flees again, the pursuer after it
                let before = self.battle.units[target].hex;
                let mp = self.battle.units[target].max_mp.max(0) as u32;
                let mut flight = vec![before];
                if let Some(Happened::Moved { path, .. }) = {
                    self.rout(target, mp);
                    self.log.iter().rev().find(|e| matches!(e, Happened::Moved { unit, .. } if *unit == target))
                } {
                    flight = path.clone();
                }
                let there = self.battle.units[target].on_field().then(|| self.battle.units[target].hex);
                let here = self.battle.units[p].hex;
                let mut chase = Vec::new();
                for &h in &flight {
                    if h == here {
                        continue;
                    }
                    if (occupied(self, h) && Some(h) != there) || crate::movement::in_enemy_zoc(&self.battle, &self.map, side, h) {
                        break;
                    }
                    chase.push(h);
                }
                if let Some(&last) = chase.last() {
                    self.log.push(Happened::Moved { unit: p, path: std::iter::once(here).chain(chase.iter().copied()).collect(), running: false });
                    self.battle.units[p].hex = last;
                    // (off the map's edge it rides out, leaderless, to come
                    // back from that edge, `FUN_004b91d7`)
                    if last.neighbours().iter().any(|&n| !crate::movement::on_map(&self.map, n)) {
                        let edge = self.battle.units[target].retreat;
                        let o = &mut self.battle.units[p];
                        (o.state, o.group, o.retreat) = (UnitState::Away, 0xffff, edge);
                    }
                }
            }
        }
        // enemy leaders beside the pursuer step away
        if !self.battle.units[p].on_field() {
            return true;
        }
        let here = self.battle.units[p].hex;
        let leaders: Vec<usize> =
            self.battle.leaders().filter(|&l| self.battle.units[l].side != side && self.battle.units[l].hex.distance(here) <= 1).collect();
        for l in leaders {
            // (at risk: finished, then wounded on d100 25..70, killed above)
            if crate::orders::leader_withdraw(&mut self.battle, &self.map, l, p, &mut self.rng) {
                self.leader_casualty(l, LeaderCasualty::Finished, 0);
                match self.rng.random() % 100 {
                    25..70 => self.leader_casualty(l, LeaderCasualty::Wounded, 1),
                    70.. => self.leader_casualty(l, LeaderCasualty::Killed, 1),
                    _ => {}
                }
            }
        }
        true
    }

    /// Let a script give the active leader's orders, as the original does:
    /// one `Orders for` call per order, repeated until the script calls
    /// `FinishLeader`, the leader has no points left, or a call changes
    /// nothing. Returns the script's debug output; the leader still has to
    /// be finished with [`Game::finish_leader`].
    pub fn ai_orders(&mut self, machine: &mut crate::ai::Machine) -> Result<Vec<String>, crate::ai::Error> {
        let Some(l) = self.activation.active() else { return Ok(Vec::new()) };
        let name = self.battle.units[l].name.clone();
        machine.define_groups(self.battle.units.iter().filter(|u| u.leader.is_some()).map(|u| (u.name.clone(), u.group as i32)));
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
        (self.battle.units.iter().map(|u| (u.hex, u.facing, u.hits, u.state)).collect(), self.shocks.len(), self.points.as_ref().map_or(0, |p| p.used))
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
        let mut trail = vec![here];
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
                // (Caesar's: an impassable side ends it, `FUN_0054be4a`)
                let map_side = gboh_formats::map::Side::ALL[[1, 2, 4, 5, 6, 8].iter().position(|&d| d == side).unwrap_or(0)];
                if self.map.caesar && self.map.side_features(here.x as usize, here.y as usize, map_side).contains(&20) {
                    break 'legs;
                }
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
                    let horse = class.is_some_and(|c| c.horse());
                    if horse && orders::withdraw(&mut self.battle, &self.map, u, elephant, &mut self.rng) {
                        continue;
                    }
                    // (Hannibal's: routed units in the way are trampled out)
                    if o.state == UnitState::Routed && self.rules.newer() {
                        let o = &mut self.battle.units[u];
                        o.state = UnitState::Gone;
                        let (hex, facing) = (o.hex, o.facing);
                        self.log.push(Happened::Eliminated { unit: u, hex, facing });
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
                    self.log.push(Happened::Casualties { unit: u, hex: o.hex, facing: o.facing, kind: Casualty::Trample, count: n, hits: o.hits });
                    let o = &self.battle.units[u];
                    if o.hits >= o.tq as i32 {
                        self.rout(u, 2);
                    }
                }
                here = next;
                trail.push(here);
            }
        }
        self.log.push(Happened::Moved { unit: elephant, path: trail, running: true });
        let o = &mut self.battle.units[elephant];
        o.state = UnitState::Gone;
        self.log.push(Happened::Eliminated { unit: elephant, hex: o.hex, facing: o.facing });
        hit
    }

    /// Rout a unit: flee `steps` hexes, or leave the battle.
    pub fn rout(&mut self, unit: usize, steps: u32) {
        if crate::movement::MoveClass::of_type(self.battle.units[unit].type_id) == Some(crate::movement::MoveClass::Elephant) {
            self.rampage(unit);
            return;
        }
        if self.battle.units[unit].state == UnitState::Fighting {
            self.log.push(Happened::Routed { unit });
            // foot archers, slingers and javelinists throw down their
            // missiles (`FUN_0056bcb0`; mounted ones and oxybeles keep them)
            use crate::missile::Weapon;
            if let Some((w @ (Weapon::Bow | Weapon::Sling | Weapon::Javelin), _)) = self.battle.units[unit].missile {
                self.battle.units[unit].missile = Some((w, 0));
            }
        }
        self.battle.units[unit].state = UnitState::Routed;
        let start = self.battle.hexes(unit);
        let (path, gone) = match flee(&self.battle, &self.map, unit, steps) {
            Flight::Moved(p) => (p, false),
            Flight::Gone(p) => (p, true),
        };
        // friendly units it routs through (the end of `FUN_0057ae96`): one
        // cohesion hit each, which may break them; routed ones are destroyed
        let side = self.battle.units[unit].side;
        let mut through: Vec<crate::hex::Hex> = start;
        through.extend(path.iter().copied());
        for o in 0..self.battle.units.len() {
            let u = &self.battle.units[o];
            if o == unit || u.side != side || u.leader.is_some() || !self.battle.hexes(o).iter().any(|h| through.contains(h)) {
                continue;
            }
            match u.state {
                UnitState::Fighting => {
                    let u = &mut self.battle.units[o];
                    u.hits += 1;
                    if u.hits >= u.tq as i32 {
                        u.state = UnitState::Routed;
                        self.log.push(Happened::Routed { unit: o });
                    }
                }
                UnitState::Routed => {
                    let u = &mut self.battle.units[o];
                    u.state = UnitState::Gone;
                    let (hex, facing) = (u.hex, u.facing);
                    self.log.push(Happened::Eliminated { unit: o, hex, facing });
                }
                _ => {}
            }
        }
        let mut trail = vec![self.battle.units[unit].hex];
        trail.extend(path.iter().copied());
        self.log.push(Happened::Moved { unit, path: trail, running: true });
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
        self.arrivals();
        self.verdict()
    }

    /// Hannibal's free units (`FUN_004b9302`): units with no leader (group
    /// 0xffff) act on their own, as the computer's do, when their entry is
    /// drawn among the leaders': each closes on its nearest enemy and
    /// attacks it if it can. (🟡 Its off-map ones, entering at random,
    /// aren't ported.)
    fn free_units(&mut self) {
        // those off the map come back on 40 %, at a free hex of their edge
        // with no enemy leader beside it, facing about (`FUN_004b9302`)
        for u in 0..self.battle.units.len() {
            if self.battle.units[u].state != UnitState::Away || self.rng.random() % 100 >= 40 {
                continue;
            }
            let (w, h) = (self.map.width as i32, self.map.height as i32);
            let mut hexes: Vec<crate::hex::Hex> = match self.battle.units[u].retreat {
                1 => (1..w).map(|c| crate::hex::Hex::new(c, 0)).collect(),
                3 => (0..h).map(|r| crate::hex::Hex::new(w - 1, r)).collect(),
                5 => (0..w).map(|c| crate::hex::Hex::new(c, if c & 1 == 1 { h - 2 } else { h - 1 })).collect(),
                _ => (0..h).map(|r| crate::hex::Hex::new(0, r)).collect(),
            };
            let side = self.battle.units[u].side;
            while !hexes.is_empty() {
                let k = self.rng.random() as usize % hexes.len();
                let at = hexes.remove(k);
                let b = &self.battle;
                let held = (0..b.units.len()).any(|x| b.units[x].on_field() && b.hexes(x).contains(&at));
                let leader = b.units.iter().any(|x| x.on_field() && x.leader.is_some() && x.side != side && at.neighbours().contains(&x.hex));
                if !held && !leader {
                    let o = &mut self.battle.units[u];
                    (o.hex, o.facing, o.state) = (at, opposite(o.facing), UnitState::Fighting);
                    break;
                }
            }
        }
        let free: Vec<usize> = (0..self.battle.units.len())
            .filter(|&u| {
                let o = &self.battle.units[u];
                o.group == 0xffff && o.leader.is_none() && o.state == UnitState::Fighting
            })
            .collect();
        if free.is_empty() {
            return;
        }
        let saved = std::mem::take(&mut self.shocks);
        self.group_step(&free, |g, u| {
            let (here, side) = (g.battle.units[u].hex, g.battle.units[u].side);
            g.battle.units.iter().filter(|o| o.side != side && o.leader.is_none() && o.on_field()).map(|o| o.hex).min_by_key(|h| (h.distance(here), h.x, h.y))
        });
        for &u in &free {
            if let Some(&t) = self.in_front(u).first() {
                self.designate_shock(u, t);
            }
        }
        for (attackers, defenders) in std::mem::take(&mut self.shocks) {
            let out = shock::resolve(&mut self.battle, &attackers, &defenders, Some(&self.map), &mut self.rng);
            for &u in &out.eliminated {
                self.battle.units[u].state = UnitState::Gone;
            }
            for &u in &out.routed {
                self.rout(u, 2);
            }
        }
        self.shocks = saved;
    }

    /// Units due this turn (the counter from 0) take their places on the
    /// field if they're free.
    pub fn arrivals(&mut self) {
        let turn = self.activation.turn;
        for u in 0..self.battle.units.len() {
            let o = &self.battle.units[u];
            if o.state != UnitState::Waiting || o.arrival > turn {
                continue;
            }
            // (`FUN_00526f1e`: its hexes empty of anyone and with no enemy
            // leader beside them, else it tries again next turn)
            let side = o.side;
            let hexes = self.battle.hexes(u);
            let b = &self.battle;
            let held = |h: crate::hex::Hex| (0..b.units.len()).any(|x| x != u && b.units[x].on_field() && b.hexes(x).contains(&h));
            let enemy_leader =
                |h: crate::hex::Hex| b.units.iter().any(|x| x.on_field() && x.leader.is_some() && x.side != side && h.neighbours().contains(&x.hex));
            let ok =
                (!o.two_hex() || o.second_hex().is_some_and(|h| crate::movement::on_map(&self.map, h))) && hexes.iter().all(|&h| !held(h) && !enemy_leader(h));
            if ok {
                self.battle.units[u].state = UnitState::Fighting;
            }
        }
    }

    /// Rout points an army has lost (units out of the battle).
    pub fn rout_points(&self, army: usize) -> u16 {
        let nation = self.armies[army].0;
        self.battle.units.iter().filter(|u| u.side == nation && u.state == UnitState::Gone).map(|u| unit_rout_points(&self.battle.options, u)).sum::<u16>()
            + self.fallen_leaders.iter().filter(|(n, _)| *n == nation).map(|(_, p)| p).sum::<u16>()
    }

    /// An army that has reached its rout level withdraws. If both have, the
    /// one less far past its level wins; on a tie the Macedonians, as the
    /// manual says (the original gave it to the enemy: fixed).
    pub fn verdict(&self) -> Verdict {
        if self.rules.newer() {
            return self.hannibal_verdict();
        }
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
            // both broken: the one less far past its level wins; a tie to
            // the Macedonians
            (true, true) => return Verdict::Won(if over(mac) <= over(enemy) { mac } else { enemy }),
            (false, false) => {}
        }
        // a killed Alexander loses the battle, a killed Darius too
        let killed = |name: &str| self.battle.units.iter().any(|u| u.leader.is_some() && u.state == UnitState::Gone && u.name.contains(name));
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

    /// Hannibal's (`FUN_00554a2f`): the rout rule, as Alexander's (the army
    /// less far past its level wins); Hannibal's death loses his army the
    /// battle; and, if the option is on, a battle's last turn gives it to
    /// the army its table names (🟡 its sides taken as home and visitor).
    fn hannibal_verdict(&self) -> Verdict {
        let over = |a: usize| self.rout_points(a) as i32 - self.armies[a].1 as i32;
        match (over(0) >= 0, over(1) >= 0) {
            (true, false) => return Verdict::Won(1),
            (false, true) => return Verdict::Won(0),
            (true, true) => return Verdict::Won(if over(0) < over(1) { 0 } else { 1 }),
            (false, false) => {}
        }
        // (not Caesar's: no leader's death ends its battles)
        // (nor by Hannibal's manual: an option)
        let hannibal = self.battle.units.iter().find(|u| {
            self.rules == Rules::Hannibal
                && !self.battle.options.manual_victory
                && u.leader.is_some()
                && u.state == UnitState::Gone
                && u.name.contains("Hannibal")
        });
        if let Some(h) = hannibal {
            let his = self.armies.iter().position(|a| a.0 == h.side).unwrap_or(0);
            return Verdict::Won(1 - his);
        }
        if self.turn_limits
            && let Some(b) = self.hannibal_battle
            && let Some(&(last, winner)) = HANNIBAL_TURN_LIMITS.get(b)
            // (Cynoscephalae's manual: turn 10, an option; the code 8)
            && self.activation.turn >= if b == 3 && self.battle.options.manual_victory { 9 } else { last }
        {
            return Verdict::Won(winner);
        }
        if self.turn_limits
            && let Some((turns, side)) = self.scenario_limit
            && self.activation.turn + 1 >= turns
        {
            return Verdict::Won(1 - (side as usize).min(1));
        }
        if self.turn_limits
            && let Some(Some(last)) = self.caesar_battle.map(|b| CAESAR_TURN_LIMITS[b])
            && self.activation.turn >= last
        {
            return Verdict::Won(1);
        }
        // Caesar's Ruspina (`FUN_005558b5`): no Caesarian unit left on the
        // field (they broke out), the Caesarians win, as the manual says
        if self.caesar_battle == Some(2) && !self.battle.units.iter().any(|u| u.nation == 9 && u.leader.is_none() && u.on_field()) {
            let caesarians = self.armies.iter().position(|a| a.0 == 9).unwrap_or(0);
            return Verdict::Won(caesarians);
        }
        Verdict::Continue
    }

    /// The battle's turn limit (the original's `FUN_0054d45b`): ten turns
    /// at Chaeronea, Lyginus, Pelium, Granicus, the Jaxartes, Arigaeum and
    /// Hydaspes, five at Samarkand, seven at Paraitacene; none at Issus and
    /// Gaugamela.
    pub fn turn_limit(&self) -> Option<u16> {
        match self.battle_index? {
            // (Alexander's manual gives Hydaspes none: an option)
            9 if self.battle.options.manual_victory => None,
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

    fn game(units: Vec<Unit>) -> Game {
        let battle = Battle { units, ..Default::default() };
        let activation = Activation::new(&battle, None);
        Game {
            triarii_doctrine: false,
            deployment: None,
            wind: false,
            reliable: Vec::new(),
            hannibal_battle: None,
            caesar_battle: None,
            scenario_limit: None,
            turn_limits: false,
            rules: Rules::Alexander,
            battle,
            battle_index: None,
            log: Vec::new(),
            exited: 0,
            map: Map {
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
            fallen_leaders: Vec::new(),
        }
    }

    #[test]
    fn a_turn_runs_through_every_leader() {
        let r = LeaderRatings { charisma: 1, command_range: 4, initiative: 3, personal_combat: 1 };
        let mut g = game(vec![unit(4, 2, Hex::new(2, 2), Some(r)), unit(5, 1, Hex::new(9, 9), Some(r)), unit(31, 2, Hex::new(3, 2), None)]);
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

    /// Hannibal's backward movement: two points at most, behind the unit,
    /// its facing kept.
    #[test]
    fn hannibal_units_step_back() {
        let hastati = gboh_formats::units::HANNIBAL + 35;
        let mut g = game(vec![unit(hastati, 6, Hex::new(5, 5), None)]);
        g.rules = Rules::Hannibal;
        let back = g.backward_options(0);
        assert!(!back.is_empty());
        assert!(back.iter().all(|h| h.distance(Hex::new(5, 5)) <= 2));
        // facing 3 (east): the hexes behind are to the west
        assert!(back.iter().all(|h| h.x <= 5), "{back:?}");
        let to = back[0];
        g.order_backward(0, to).unwrap();
        assert_eq!((g.battle.units[0].hex, g.battle.units[0].facing), (to, 3));
    }

    /// Caesar's manual: Gallic leaders rally only within two hexes (an
    /// option).
    #[test]
    fn gallic_leaders_rally_near() {
        let r = LeaderRatings { charisma: 1, command_range: 6, initiative: 3, personal_combat: 1 };
        let mut g = game(vec![unit(4, 11, Hex::new(2, 2), Some(r)), unit(31, 11, Hex::new(2, 6), None), unit(31, 11, Hex::new(2, 4), None)]);
        g.rules = Rules::Caesar;
        assert_eq!(g.begin_turn(), Some(0));
        assert!(g.may_rally(1) && g.may_rally(2));
        g.battle.options.manual_command = true;
        assert!(!g.may_rally(1) && g.may_rally(2));
    }

    /// Hannibal's Roman leaders never fail a group order.
    #[test]
    fn roman_group_orders_succeed() {
        let ratings = LeaderRatings { charisma: 0, command_range: 4, initiative: 1, personal_combat: 0 };
        let hastati = gboh_formats::units::HANNIBAL + 35;
        let mut g = game(vec![unit(hastati - 30, 6, Hex::new(2, 2), Some(ratings)), unit(hastati, 6, Hex::new(2, 3), None)]);
        g.rules = Rules::Hannibal;
        for _ in 0..20 {
            assert_eq!(g.begin_turn(), Some(0));
            assert!(g.group_order_roll());
        }
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
        let mut g = game(vec![unit(31, 2, Hex::new(4, 4), None), unit(31, 1, n[1], None), unit(31, 1, n[2], None), unit(31, 1, n[4], None)]);
        assert!(g.designate_shock(0, 1));
        assert_eq!(g.shocks[0].1, vec![1, 2]);
        // not behind
        assert!(!g.designate_shock(0, 3));
    }

    #[test]
    fn activations_let_moved_units_go_again_at_a_cost() {
        let r = LeaderRatings { charisma: 1, command_range: 9, initiative: 3, personal_combat: 1 };
        let mut g = game(vec![unit(4, 2, Hex::new(2, 2), Some(r)), unit(31, 2, Hex::new(4, 4), None)]);
        g.battle.units[1].mp = 1;
        g.activate(Some(0));
        assert_eq!(g.battle.units[1].mp, 5);
        let m = g.map.clone();
        crate::orders::move_to(&mut g.battle, &m, 1, Hex::new(5, 4), &mut BorlandRand::new(1)).unwrap();
        assert_eq!(g.battle.units[1].hits, 1);
        // only once an activation
        crate::orders::move_to(&mut g.battle, &m, 1, Hex::new(6, 4), &mut BorlandRand::new(1)).unwrap();
        assert_eq!(g.battle.units[1].hits, 1);
        // the next activation, again
        g.activate(Some(0));
        crate::orders::move_to(&mut g.battle, &m, 1, Hex::new(7, 4), &mut BorlandRand::new(1)).unwrap();
        assert_eq!(g.battle.units[1].hits, 2);
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
        let mut g = game(vec![unit(4, 2, Hex::new(2, 6), Some(r)), unit(31, 2, Hex::new(3, 5), None), unit(31, 2, Hex::new(3, 7), None)]);
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
        g.leader_casualty(0, LeaderCasualty::Wounded, 2);
        let r = g.battle.units[0].leader.unwrap();
        assert_eq!((r.command_range, r.initiative, r.personal_combat), (3, 1, 1));
        // a second wound kills: a soldier from the ranks takes over
        g.leader_casualty(0, LeaderCasualty::Wounded, 1);
        assert_eq!(g.battle.units[0].state, UnitState::Fighting);
        assert_eq!((g.battle.units[0].leader.unwrap().command_range, g.battle.units[0].leader.unwrap().initiative), (4, 2));
        assert!(g.rout_points(0) > 0);
    }

    #[test]
    fn routed_units_flee_at_the_end_of_the_turn() {
        let mut g = game(vec![unit(31, 2, Hex::new(6, 5), None)]);
        g.battle.units[0].state = UnitState::Routed;
        g.end_turn();
        assert!(g.battle.units[0].hex.x < 6 || g.battle.units[0].state == UnitState::Gone);
    }
}
