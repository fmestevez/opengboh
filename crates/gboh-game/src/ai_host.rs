//! The AI scripts' built-in functions, played on a [`Game`] (see
//! `docs/formats.md`, "AI scripts").
//!
//! Script values: units are battle indices, hexes are `x * 100 + y` (map
//! coordinates, no origin), directions are 1 N … 8 NW. Facings are hex
//! vertices (NE, E, SE, SW, W, NW); `AdjacentHex` takes hex sides (N, NE,
//! SE, S, SW, NW).
//!
//! Status: `Query` (every attribute), the group list, `GetLeader`,
//! `HexOccupancy`, `IsLeader`, `IsEnemy`, `InCommandRange`, `CanBeOrdered`,
//! `AssignList`, `Random`, `GetTurnNum`, `Recover`, the order count (one
//! per new unit), `Direction`, `IsInGeneralDir`, `Rotate`, `TurnToAttack`,
//! `AnyoneToCombat`, `CanMove`, `FindClosestEnemyToAttack`, `Attack`,
//! `Fire`, `EnemyWithinRange`, `FindHurtingUnit` and `MoveToward`'s choice
//! of hex follow the original's code (ties between
//! equally good hexes are broken by position here; the original's order is
//! its internal set order). 🟡
//! Leader movement follows the original's code too.

use crate::ai::Host;
use crate::battle::{UnitState, neighbour};
use crate::game::Game;
use crate::hex::Hex;
use crate::movement::{MoveClass, arc, forward, on_map, reachable};
use crate::orders;

/// Script nation numbers for the scenario's nation bytes (the original's
/// remap on load): 0 Greek → 4, 1 Persian → 2, 2 Macedonian → 1, 3 Indian →
/// 3, 4 Danubian → 5, 5 Scythian → 6; the later games' nations one up
/// (Hannibal's 6 Roman → 7, 7 Carthaginian → 8; Caesar's 9 Caesarian → 10,
/// 10 Pompeian → 11, 11 barbarians → 12).
pub fn script_nation(file: u8) -> i32 {
    match file {
        0 => 4,
        1 => 2,
        2 => 1,
        3 => 3,
        b @ 4..=11 => b as i32 + 1,
        _ => 0,
    }
}

/// A script hex number for a map hex.
pub fn hex_id(h: Hex) -> i32 {
    h.x * 100 + h.y
}

fn hex_of(id: i32) -> Option<Hex> {
    (id >= 0).then(|| Hex::new(id / 100, id % 100))
}

/// The angle from `a` to `b` in steps of 15° clockwise from north (0..24),
/// or `None` for the same hex (the original's table lookup: hexes become
/// grid points `(2x + 2, 4y + 2 + 2·(x odd))`, and the ratio of the
/// distances across and down picks the angle).
pub fn angle(a: Hex, b: Hex) -> Option<i32> {
    // the original's grid point (`FUN_004b1c4a`): column `id / 50 + 2`, one
    // more than `2x + 2` from y = 50 on (see `Map::hex_cell`)
    let grid = |h: Hex| ((100 * h.x + h.y).div_euclid(50) + 2, 4 * h.y + 2 * (h.x & 1) + 2);
    let ((ax, ay), (bx, by)) = (grid(a), grid(b));
    let (dx, dy) = (bx - ax, by - ay);
    if dx == 0 && dy == 0 {
        return None;
    }
    let (adx, ady) = (dx.abs(), dy.abs());
    let class = if dx == 0 {
        0
    } else if dy == 0 {
        1
    } else if ady == adx {
        2
    } else if ady < adx {
        3
    } else if ady == 3 * adx {
        4
    } else if ady < 3 * adx {
        5
    } else {
        6
    };
    const TABLE: [[i32; 7]; 4] = [
        [0, 6, 4, 5, 2, 3, 1],         // right and up
        [0, 18, 20, 19, 22, 21, 23],   // left and up
        [0, 0, 8, 7, 10, 9, 11],       // right and down
        [12, 18, 16, 17, 14, 15, 13],  // left and down
    ];
    let quadrant = match (dx > 0, dy > 0) {
        (true, false) => 0,
        (false, false) => 1,
        (true, true) => 2,
        (false, true) => 3,
    };
    Some(TABLE[quadrant][class])
}

/// The facing (NE, E, SE, SW, W, NW) whose 60° sector holds the direction
/// from `a` to `b` (`Direction` in the scripts; never N or S), or 0.
pub fn direction(a: Hex, b: Hex) -> i32 {
    angle(a, b).map_or(0, |t| [2, 3, 4, 6, 7, 8][(t / 4) as usize])
}

/// Whether the direction from `a` to `b` lies in the cone of `facing`:
/// the five angle steps from its left front hex side to its right one.
pub fn in_cone(a: Hex, b: Hex, facing: u8) -> bool {
    let first = match facing {
        2 => 0,
        3 => 4,
        4 => 8,
        6 => 12,
        7 => 16,
        8 => 20,
        _ => return false,
    };
    angle(a, b).is_some_and(|t| (0..5).any(|k| (first + k) % 24 == t))
}

/// The AI's view of the game while it gives one leader's orders.
pub struct AiHost<'a> {
    pub game: &'a mut Game,
    lists: Vec<Option<(Vec<i32>, usize)>>,
    group: Vec<usize>,
    group_pos: usize,
    /// Set when the script calls `FinishLeader`.
    pub finished: bool,
    /// Debug output from `write` / `watch`.
    pub log: Vec<String>,
    /// Units that attacked in this activation: they may still fire (the
    /// scripts' `Attack` then `ShootMissiles`; the original's commentary
    /// shows units throwing javelins before their charge).
    attacked: Vec<usize>,
}

impl<'a> AiHost<'a> {
    pub fn new(game: &'a mut Game) -> Self {
        let group = Self::group_of(game);
        Self { game, lists: Vec::new(), group, group_pos: 0, finished: false, log: Vec::new(), attacked: Vec::new() }
    }

    /// The active leader's group: the units sharing his group number,
    /// within command range or not (the scripts sort out the ones out of
    /// range with `InCommandRange`).
    fn group_of(game: &Game) -> Vec<usize> {
        let Some(l) = game.activation.active() else { return Vec::new() };
        let b = &game.battle;
        (0..b.units.len())
            .filter(|&u| {
                let o = &b.units[u];
                o.leader.is_none() && o.side == b.units[l].side && o.group == b.units[l].group && o.on_field()
            })
            .collect()
    }

    /// Whether `unit` can take an order now (`CanBeOrdered`, the unit's
    /// flag set at activation): a unit in the leader's command that had not
    /// moved, not yet done; or the leader himself if he has not moved.
    fn can_be_ordered(&self, unit: usize) -> bool {
        if Some(unit) == self.leader() {
            return !self.game.leader_moved;
        }
        self.game.orderable().contains(&unit)
    }

    fn unit(&self, id: i32) -> Option<usize> {
        usize::try_from(id).ok().filter(|&u| u < self.game.battle.units.len())
    }

    fn list(&mut self, id: i32) -> Option<&mut (Vec<i32>, usize)> {
        usize::try_from(id).ok().and_then(|i| self.lists.get_mut(i)).and_then(Option::as_mut)
    }

    fn leader(&self) -> Option<usize> {
        self.game.activation.active()
    }

    /// Charge the active leader for ordering `unit` with the AI's count
    /// (one per new unit); false if he cannot.
    fn charge(&mut self, unit: usize) -> bool {
        if !self.can_be_ordered(unit) {
            return false;
        }
        let Some(points) = self.game.points.as_mut() else { return false };
        if !points.can_order(unit) {
            return false;
        }
        points.ai_unit_done(unit);
        true
    }

    fn enemies_of(&self, unit: usize) -> impl Iterator<Item = usize> + '_ {
        let side = self.game.battle.units[unit].side;
        let b = &self.game.battle;
        (0..b.units.len())
            .filter(move |&e| b.units[e].side != side && b.units[e].leader.is_none() && b.units[e].state == UnitState::Fighting)
    }

    fn query(&self, attr: i32, u: usize) -> i32 {
        let unit = &self.game.battle.units[u];
        if let Some(r) = unit.leader {
            let used = self.game.points.as_ref().filter(|_| self.leader() == Some(u)).map_or(0, |p| p.used as i32);
            match attr {
                14 => return r.command_range as i32,
                15 => return r.initiative as i32,
                16 => return r.charisma as i32,
                17 => return r.personal_combat as i32,
                18 => return used,
                19 => return r.initiative as i32 - used,
                _ => {}
            }
        }
        match attr {
            1 => unit.tq as i32,
            2 => unit.hits,
            3 => unit.size as i32,
            4 => unit.max_mp,
            5 => unit.mp,
            6 => unit.missile.map_or(0, |(w, _)| w as i32),
            7 => unit.missile.map_or(0, |(w, _)| w.range()),
            8 => unit.missile.map_or(0, |(_, n)| n as i32),
            9 => MoveClass::of_type(unit.type_id).map_or(-1, |c| c as i32),
            10 => hex_id(unit.hex),
            11 => unit.second_hex().map_or(-1, hex_id),
            12 => unit.facing as i32,
            13 => script_nation(unit.nation),
            20 => unit.missile.is_some() as i32,
            21 => (unit.state == UnitState::Routed) as i32,
            _ => -1,
        }
    }

    /// Hexes for `AssignList`: 0 zone of control (front), 1 flank, 2 rear,
    /// 3 the unit's own hexes, 5 all around.
    fn around(&self, u: usize, kind: i32) -> Vec<i32> {
        let unit = &self.game.battle.units[u];
        let Some(front) = forward(unit.facing) else { return Vec::new() };
        let behind = front.map(|d| (d + 3) % 6);
        let mut out = Vec::new();
        for h in self.game.battle.hexes(u) {
            for (d, n) in h.neighbours().into_iter().enumerate() {
                let k = if front.contains(&d) {
                    0
                } else if behind.contains(&d) {
                    2
                } else {
                    1
                };
                if (kind == k || kind == 5) && !self.game.battle.hexes(u).contains(&n) && !out.contains(&hex_id(n)) {
                    out.push(hex_id(n));
                }
            }
        }
        if kind == 3 {
            out = self.game.battle.hexes(u).into_iter().map(hex_id).collect();
        }
        out
    }

    /// Turn `u` to face `dir` (the original's rule): north counts as NE
    /// and south as SW; it turns clockwise if that takes fewer steps, else
    /// counter-clockwise. 2 if it turned, 0 if not.
    fn rotate(&mut self, u: usize, dir: i32) -> i32 {
        let target = match dir {
            1 => 2,
            5 => 6,
            d => d as u8,
        };
        if ![2, 3, 4, 6, 7, 8].contains(&target) {
            return 0;
        }
        let steps = |left: bool| {
            let mut f = self.game.battle.units[u].facing;
            let mut n = 0;
            while f != target && n < 6 {
                f = orders::turned(f, left);
                n += 1;
            }
            n
        };
        let (cw, ccw) = (steps(false), steps(true));
        let (left, n) = if cw < ccw { (false, cw) } else { (true, ccw) };
        if n == 0 || !self.charge(u) {
            return 0;
        }
        let map = self.game.map.clone();
        for _ in 0..n {
            if orders::turn(&mut self.game.battle, &map, u, left).is_err() {
                break;
            }
        }
        2
    }

    /// `TurnToAttack` (the original's search): try facings 0 to 3 turns
    /// away, clockwise before counter-clockwise at each step, and turn to
    /// the first with an enemy in front. 0 if none.
    fn turn_to_attack(&mut self, u: usize) -> i32 {
        let start = self.game.battle.units[u].facing;
        for k in 0..4 {
            for left in [false, true] {
                let mut f = start;
                for _ in 0..k {
                    f = orders::turned(f, left);
                }
                self.game.battle.units[u].facing = f;
                let threat = !self.game.in_front(u).is_empty();
                self.game.battle.units[u].facing = start;
                if threat {
                    return if f == start { 0 } else { self.rotate(u, f as i32) };
                }
            }
        }
        0
    }

    /// Move `u` as close to `target` as it can this turn: 2 if it moved, 0
    /// if it could not get closer (as in the original, a unit already as
    /// close as it can get, e.g. next to the enemy it was sent at, reports
    /// 0, and the scripts count it as unable to move).
    fn move_toward(&mut self, target: Hex, u: usize) -> i32 {
        if self.game.battle.units[u].leader.is_some() {
            return self.move_leader(target, u);
        }
        let here = self.game.battle.units[u].hex;
        let mp = self.game.battle.units[u].mp;
        let options = reachable(&self.game.battle, &self.game.map, u, mp);
        let best = options
            .iter()
            .filter(|(h, _)| h.distance(target) < here.distance(target))
            .min_by_key(|(h, c)| (h.distance(target), **c, h.x, h.y))
            .map(|(h, _)| *h);
        let Some(to) = best else { return 0 };
        if !self.charge(u) {
            return 0;
        }
        match self.game.order_move(u, to) {
            Ok(_) => 2,
            Err(_) => 0,
        }
    }

    /// A leader moves to `target` if he can end there, else to the hex he
    /// can end in nearest it (see `movement::leader_moves`); once per
    /// activation.
    fn move_leader(&mut self, target: Hex, l: usize) -> i32 {
        let here = self.game.battle.units[l].hex;
        let options = self.game.leader_options();
        let to = if options.contains(&target) {
            target
        } else {
            *options.iter().min_by_key(|h| (h.distance(target), h.x, h.y)).unwrap_or(&here)
        };
        if to == here || !self.charge(l) {
            return 0;
        }
        self.game.move_leader(to) as i32 * 2
    }

    /// `AnyoneToCombat` (the original's priorities): among enemies the unit
    /// could reach a hex next to this turn, routed ones first, then those
    /// within three hits of routing, then those its class is superior to,
    /// then reachable flank and rear hexes of the others, then enemies it
    /// attacks frontally at a base column above 4; the nearest of the first
    /// non-empty group, as a hex, or −1.
    fn anyone_to_combat(&self, u: usize) -> i32 {
        use crate::combat::{attacker_superior, base_column};
        let b = &self.game.battle;
        let here = b.units[u].hex;
        let mut spots: Vec<Hex> = reachable(b, &self.game.map, u, b.units[u].mp).into_keys().collect();
        spots.push(here);
        let my_class = MoveClass::of_type(b.units[u].type_id).unwrap_or(MoveClass::HeavyInfantry);
        let side = b.units[u].side;
        let mut groups: [Vec<Hex>; 5] = Default::default();
        for e in 0..b.units.len() {
            let o = &b.units[e];
            if o.side == side || o.leader.is_some() || o.state == UnitState::Gone {
                continue;
            }
            if !spots.iter().any(|s| s.distance(o.hex) == 1) {
                continue;
            }
            let class = MoveClass::of_type(o.type_id).unwrap_or(MoveClass::HeavyInfantry);
            if o.state == UnitState::Routed {
                groups[0].push(o.hex);
            } else if (o.tq as i32 - o.hits) < 3 {
                groups[1].push(o.hex);
            } else if attacker_superior(my_class, class) {
                groups[2].push(o.hex);
            } else {
                let flank_rear: Vec<Hex> = [2, 1]
                    .iter()
                    .flat_map(|&k| self.around(e, k))
                    .filter_map(hex_of)
                    .filter(|h| spots.contains(h))
                    .collect();
                if !flank_rear.is_empty() {
                    groups[3].extend(flank_rear);
                } else if base_column(my_class, class, 0) > 4 {
                    groups[4].push(o.hex);
                }
            }
        }
        groups
            .iter()
            .find(|g| !g.is_empty())
            .and_then(|g| g.iter().min_by_key(|h| (h.distance(here), h.x, h.y)))
            .map_or(-1, |&h| hex_id(h))
    }

    /// `IsInGeneralDir`: the target is in the unit's cone, from its main
    /// hex or (two-hex units) its second.
    fn in_general_dir(&self, u: usize, target: Hex) -> bool {
        let unit = &self.game.battle.units[u];
        self.game.battle.hexes(u).into_iter().any(|h| in_cone(h, target, unit.facing))
    }
}

impl Host for AiHost<'_> {
    fn call(&mut self, name: &str, a: &[i32]) -> Option<i32> {
        let arg = |i: usize| a.get(i).copied().unwrap_or(-1);
        Some(match name {
            "getturnnum" => self.game.activation.turn as i32 + 1,
            "random" => match arg(0) {
                n if n > 0 => (self.game.rng.next() % n as u32) as i32,
                _ => 0,
            },
            "getleader" => self.leader().map_or(-1, |l| l as i32),
            "finishleader" => {
                self.finished = true;
                1
            }
            "breakpoint" | "dumplist" => 0,
            // Hannibal's: groups by their number (a leader's and his
            // units'), and the group orders
            "getgroup" => {
                let gid = arg(0);
                let b = &self.game.battle;
                let units: Vec<i32> = (0..b.units.len()).filter(|&u| b.units[u].on_field() && b.units[u].leader.is_none() && b.units[u].group as i32 == gid).map(|u| u as i32).collect();
                self.list(arg(1)).map(|l| *l = (units, 0)).is_some() as i32
            }
            "getgroupleader" => {
                let b = &self.game.battle;
                b.leaders().find(|&l| b.units[l].group as i32 == arg(0)).map_or(-1, |l| l as i32)
            }
            "groupmove" => {
                let Some(target) = hex_of(arg(0)) else { return Some(0) };
                match self.game.try_group_move() {
                    Some(crate::game::GroupMove::Full | crate::game::GroupMove::OneHex) => {
                        let to = self.game.group_move_options().into_iter().min_by_key(|h| (h.distance(target), h.x, h.y));
                        to.is_some_and(|h| self.game.do_group_move(h)) as i32
                    }
                    _ => 0,
                }
            }
            "dogroupberserk" => self.game.group_attack() as i32,
            "dogroupretreat" => self.game.group_retreat() as i32,
            "grouprestorecohesion" => self.game.group_restore() as i32,
            "inmomentumphase" => self.game.activation.in_momentum() as i32,
            "grouplistreset" => {
                self.group = Self::group_of(self.game);
                self.group_pos = 0;
                1
            }
            "grouplistgetnext" => {
                let v = self.group.get(self.group_pos).map_or(-1, |&u| u as i32);
                self.group_pos = (self.group_pos + 1).min(self.group.len());
                v
            }
            "grouplistgetprevious" => {
                self.group_pos = self.group_pos.saturating_sub(1);
                self.group.get(self.group_pos).map_or(-1, |&u| u as i32)
            }
            "grouplistget" => self.group.get(self.group_pos.saturating_sub(1)).map_or(-1, |&u| u as i32),
            "grouplistgetsize" => self.group.len() as i32,
            "hexoccupancy" => {
                let Some(h) = hex_of(arg(0)) else { return Some(-1) };
                let b = &self.game.battle;
                let mut at: Vec<usize> = (0..b.units.len()).filter(|&u| b.units[u].on_field() && b.hexes(u).contains(&h)).collect();
                at.sort_by_key(|&u| b.units[u].leader.is_some());
                at.first().map_or(-1, |&u| u as i32)
            }
            "adjacenthex" => {
                let Some(h) = hex_of(arg(0)) else { return Some(-1) };
                match neighbour(h, arg(1).max(0) as usize) {
                    Some(n) if on_map(&self.game.map, n) => hex_id(n),
                    _ => -1,
                }
            }
            "direction" => match (hex_of(arg(0)), hex_of(arg(1))) {
                (Some(x), Some(y)) => direction(x, y),
                _ => 0,
            },
            "distance" => match (hex_of(arg(0)), hex_of(arg(1))) {
                (Some(x), Some(y)) => x.distance(y),
                _ => 0,
            },
            "query" => self.unit(arg(1)).map_or(-1, |u| self.query(arg(0), u)),
            "isleader" => self.unit(arg(0)).is_some_and(|u| self.game.battle.units[u].leader.is_some()) as i32,
            "isenemy" => match (self.unit(arg(0)), self.unit(arg(1))) {
                (Some(x), Some(y)) => (self.game.battle.units[x].side != self.game.battle.units[y].side) as i32,
                _ => 0,
            },
            "incommandrange" | "canbeordered" => self.unit(arg(0)).map_or(-1, |u| self.can_be_ordered(u) as i32),
            // lists
            "getlist" => {
                self.lists.push(Some((Vec::new(), 0)));
                self.lists.len() as i32 - 1
            }
            "releaselist" => {
                if let Some(slot) = usize::try_from(arg(0)).ok().and_then(|i| self.lists.get_mut(i)) {
                    *slot = None;
                }
                1
            }
            "addtolist" => {
                let v = arg(1);
                self.list(arg(0)).map(|l| l.0.push(v)).is_some() as i32
            }
            "removefromlist" => {
                let v = arg(1);
                self.list(arg(0)).map(|l| l.0.retain(|&x| x != v)).is_some() as i32
            }
            "getnextinlist" => self.list(arg(0)).map_or(-1, |l| {
                let v = l.0.get(l.1).copied().unwrap_or(-1);
                l.1 = (l.1 + 1).min(l.0.len());
                v
            }),
            "resetlist" => self.list(arg(0)).map(|l| l.1 = 0).is_some() as i32,
            "clearlist" => self.list(arg(0)).map(|l| *l = (Vec::new(), 0)).is_some() as i32,
            "sizeoflist" => self.list(arg(0)).map_or(0, |l| l.0.len() as i32),
            "getlast" => self.list(arg(0)).map_or(-1, |l| l.0.last().copied().unwrap_or(-1)),
            "ismemberof" => {
                let v = arg(1);
                self.list(arg(0)).is_some_and(|l| l.0.contains(&v)) as i32
            }
            "assignlist" => {
                let Some(u) = self.unit(arg(2)) else { return Some(0) };
                let items = if arg(1) == 6 {
                    self.game.orderable().into_iter().map(|u| u as i32).collect()
                } else {
                    self.around(u, arg(1))
                };
                self.list(arg(0)).map(|l| *l = (items, 0)).is_some() as i32
            }
            "getunitpaths" | "getvalidmovement" => {
                let Some(u) = self.unit(*a.last().unwrap_or(&-1)) else { return Some(0) };
                reachable(&self.game.battle, &self.game.map, u, self.game.battle.units[u].mp).len() as i32
            }
            // orders
            // any empty hex to move to (the original asks only that)
            "canmove" => self.unit(arg(0)).is_some_and(|u| {
                let mp = self.game.battle.units[u].mp;
                !reachable(&self.game.battle, &self.game.map, u, mp).is_empty()
            }) as i32,
            "canrotate" => self.unit(arg(0)).is_some_and(|u| self.group.contains(&u) && self.game.battle.units[u].mp > 0) as i32,
            "rotate" => self.unit(arg(0)).map_or(0, |u| self.rotate(u, arg(1))),
            "moveto" | "movetoward" => match (hex_of(arg(0)), self.unit(arg(1))) {
                (Some(h), Some(u)) => self.move_toward(h, u),
                _ => 0,
            },
            "isingeneraldir" => match (self.unit(arg(0)), hex_of(arg(1))) {
                (Some(u), Some(h)) => self.in_general_dir(u, h) as i32,
                _ => 0,
            },
            "anyonetocombat" => self.unit(arg(0)).map_or(-1, |u| self.anyone_to_combat(u)),
            // rings outward from the unit; routed enemies count too
            "findclosestenemytoattack" => self.unit(arg(0)).map_or(-1, |u| {
                let b = &self.game.battle;
                let (here, side) = (b.units[u].hex, b.units[u].side);
                b.units
                    .iter()
                    .filter(|o| o.side != side && o.leader.is_none() && o.on_field())
                    .min_by_key(|o| (o.hex.distance(here), o.hex.x, o.hex.y))
                    .map_or(-1, |o| hex_id(o.hex))
            }),
            "findclosestenemytofire" => self.unit(arg(0)).map_or(-1, |u| {
                let s = &self.game.battle.units[u];
                let Some((w, n)) = s.missile.filter(|m| m.1 > 0) else { return -1 };
                let _ = n;
                let field = arc(s.hex, s.facing, w.range());
                self.enemies_of(u)
                    .filter(|&e| field.contains(&self.game.battle.units[e].hex))
                    .min_by_key(|&e| (self.game.battle.units[e].hex.distance(s.hex), e))
                    .map_or(-1, |e| hex_id(self.game.battle.units[e].hex))
            }),
            // any enemy, leaders included, within the radius
            "enemywithinrange" => self.unit(arg(0)).is_some_and(|u| {
                let b = &self.game.battle;
                let (here, side) = (b.units[u].hex, b.units[u].side);
                b.units.iter().any(|o| o.side != side && o.on_field() && o.hex.distance(here) <= arg(1))
            }) as i32,
            "turntoattack" => self.unit(arg(0)).map_or(-1, |u| self.turn_to_attack(u)),
            "canattack" => match (self.unit(arg(0)), hex_of(arg(1))) {
                (Some(u), Some(h)) => self.game.in_front(u).iter().any(|&e| self.game.battle.hexes(e).contains(&h)) as i32,
                _ => 0,
            },
            // attacks everyone in its zone of control, the unit on `hex` (if
            // any) as the primary target; always 1
            "attack" => {
                let (Some(u), h) = (self.unit(arg(0)), hex_of(arg(1))) else { return Some(1) };
                let front = self.game.in_front(u);
                let target = front.iter().copied().find(|&e| h.is_some_and(|h| self.game.battle.hexes(e).contains(&h))).or(front.first().copied());
                if let Some(t) = target
                    && self.charge(u)
                {
                    self.game.designate_shock(u, t);
                    self.attacked.push(u);
                }
                if self.can_be_ordered(u) {
                    self.game.spend(u, true);
                }
                1
            }
            "fire" => {
                let (Some(u), Some(h)) = (self.unit(arg(0)), hex_of(arg(1))) else { return Some(0) };
                let Some(t) = self.enemies_of(u).find(|&e| self.game.battle.hexes(e).contains(&h)) else { return Some(0) };
                // a unit that has just attacked fires on the same order
                if !self.attacked.contains(&u) && !self.charge(u) {
                    return Some(0);
                }
                let ok = self.game.order_volley(u, t).is_ok();
                if ok {
                    self.game.spend(u, false);
                }
                ok as i32
            }
            "attempttorally" => {
                let Some(u) = self.unit(arg(0)) else { return Some(0) };
                // the original counts the order before trying
                if !self.charge(u) {
                    return Some(0);
                }
                self.game.rally(u) as i32
            }
            "recover" => {
                let Some(u) = self.unit(arg(0)) else { return Some(0) };
                if !self.charge(u) {
                    return Some(0);
                }
                self.game.recover(u);
                1
            }
            // every friendly unit on the map: the nearest routed one if the
            // leader's initiative is over 3, else the nearest with hits
            "findhurtingunit" => {
                let Some(l) = self.leader() else { return Some(-1) };
                let b = &self.game.battle;
                let (lh, side) = (b.units[l].hex, b.units[l].side);
                let ini = b.units[l].leader.map_or(0, |r| r.initiative);
                let nearest = |routed: bool| {
                    b.units
                        .iter()
                        .filter(|o| o.side == side && o.leader.is_none() && o.on_field())
                        .filter(|o| if routed { o.state == UnitState::Routed } else { o.state == UnitState::Fighting && o.hits > 0 })
                        .min_by_key(|o| o.hex.distance(lh))
                        .map(|o| hex_id(o.hex))
                };
                match (ini > 3, nearest(true)) {
                    (true, Some(h)) => h,
                    _ => nearest(false).unwrap_or(-1),
                }
            }
            _ => return None,
        })
    }

    fn write(&mut self, message: &str, values: &[i32]) {
        let vals: Vec<String> = values.iter().map(i32::to_string).collect();
        self.log.push(format!("{message} {}", vals.join(" ")).trim_end().to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_and_directions() {
        let h = Hex::new(4, 4);
        let n = h.neighbours(); // N, NE, SE, S, SW, NW
        assert_eq!(n.map(|x| angle(h, x).unwrap()), [0, 4, 8, 12, 16, 20]);
        assert_eq!(direction(h, Hex::new(8, 4)), 3); // due east
        assert_eq!(direction(h, Hex::new(4, 0)), 2); // due north counts as NE
        assert_eq!(direction(h, Hex::new(4, 8)), 6); // due south counts as SW
        assert!(in_cone(h, n[0], 2) && in_cone(h, n[1], 2) && !in_cone(h, n[2], 2));
        assert!(in_cone(h, n[0], 8));
        // from y = 50 on the original's grid column is one more, so the step
        // north across y = 50 (only on Paraitacene) reads as just west of north
        assert_eq!(angle(Hex::new(4, 50), Hex::new(4, 49)), Some(23));
        assert_eq!(angle(Hex::new(4, 51), Hex::new(4, 50)), Some(0));
    }
}
