//! Playing a battle in the viewer: by hand on both sides, or against the
//! original's AI scripts on one.
//!
//! Left click one of the active leader's units (the flagged ones) to select
//! it; its reachable hexes light up. Left click a lit hex to move there, or
//! an enemy in front to designate a shock. Right click an enemy to volley.
//! Q / E turn the selected unit left / right, R rallies it (routed), C
//! recovers its cohesion. Click the active leader, then a lit hex, to move
//! him; G tries a group move (then a lit hex moves the whole group). Enter finishes the leader (shocks are resolved), M finishes him and
//! tries for momentum; after the last leader the turn ends.

use gboh_formats::pal::Palette;
use gboh_game::battle::UnitState;
use gboh_game::game::{Game, Verdict};
use gboh_game::hex::Hex;
use gboh_game::movement::highlighted;
use gboh_game::orders::{self, CommandPoints};
use gboh_render::Canvas;
use gboh_render::battlefield::Scene;
use gboh_render::missiles::{Flight, Missile};
use gboh_render::runaways::Runaway;
use gboh_render::units::{FIRE_CELS, FIRE_RELEASE, UnitView};

/// Something to animate, in the order it happened.
#[derive(Debug, Clone, PartialEq)]
pub enum Anim {
    /// A unit went through these hexes from where it started; `running`
    /// when routed.
    Walk { unit: usize, path: Vec<Hex>, running: bool },
    /// A missile unit shot at another, with the sound to play as it does,
    /// leaving a spent missile in the target's hex.
    Volley { shooter: usize, target: usize, missile: Missile, sound: crate::sound::Event, debris: Option<gboh_render::units::Corpse> },
    /// A unit broke: its men throw down their arms before they run, leaving
    /// them on the ground.
    Drop { unit: usize, debris: Vec<gboh_render::units::Corpse> },
    /// Men of a unit fell where they stood.
    Fall(Fallen),
    /// A shock combat: the units fighting, each with the way to its nearest
    /// enemy and the sides (bits, relative to its front) enemies stand on,
    /// and the men who fall during it.
    Fight { units: Vec<(usize, u8, u16)>, falls: Vec<Fallen>, sound: crate::sound::Event },
}

/// One of the player's orders, as recorded for a saved game: replaying them
/// in turn from the scenario gives the same battle, as play is
/// deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Click { hex: Hex, right: bool },
    ArmVolley,
    GroupMove,
    Rally,
    Recover,
    Turn { left: bool },
    AboutFace,
    Undo,
    Finish { momentum: bool },
    /// The viewer played one of the computer's leaders (demo pacing).
    StepComputer,
}

/// Something the player is told in a dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A leader tried for momentum.
    Momentum { leader: usize, won: bool },
    /// A routed unit tried to rally: back with these hits, or gone.
    Rally { unit: usize, hits: Option<i32> },
    /// The player's volley hit a unit; whether it now routs.
    Volley { target: usize, hits: i32, routed: bool },
    /// The active leader has no orders left.
    NoOrders,
}

/// Men of a unit falling: which were down before (bit per figure), the
/// ones who fall in the order they do with their bodies, and whether the
/// unit is destroyed.
#[derive(Debug, Clone, PartialEq)]
pub struct Fallen {
    pub unit: usize,
    pub before: u32,
    pub men: Vec<usize>,
    pub corpses: Vec<gboh_render::units::Corpse>,
    pub destroyed: bool,
    /// Loose horses and runaway chariots: after which of `men` falls, what,
    /// and the way it runs.
    pub runaways: Vec<(usize, Runaway, RunPath)>,
}

/// A runaway's way: map hexes with the screen direction it runs in.
pub type RunPath = Vec<((usize, usize), usize)>;

/// A corpse as the view turned half round shows it: its offset from the
/// hex centre turned, and arms lying the other way.
pub fn turn_corpse(c: &mut gboh_render::units::Corpse) {
    use gboh_render::units::SPEAR;
    c.offset = (-c.offset.0, -c.offset.1);
    if (SPEAR + 1..=SPEAR + 8).contains(&c.kind) {
        c.kind = SPEAR + (c.kind - SPEAR + 3) % 8 + 1;
    }
}

/// A screen direction (1 N … 8 NW) turned half round.
fn turn_dir(d: usize) -> usize {
    (d + 3) % 8 + 1
}

impl Anim {
    /// The animation as the view turned half round shows it.
    pub fn turn(&mut self) {
        match self {
            Anim::Volley { debris: Some(c), .. } => turn_corpse(c),
            Anim::Drop { debris, .. } => debris.iter_mut().for_each(turn_corpse),
            Anim::Fall(f) => f.turn(),
            Anim::Fight { falls, .. } => falls.iter_mut().for_each(Fallen::turn),
            _ => {}
        }
    }
}

impl Fallen {
    fn turn(&mut self) {
        self.corpses.iter_mut().for_each(turn_corpse);
        for (_, _, path) in self.runaways.iter_mut() {
            path.iter_mut().for_each(|(_, d)| *d = turn_dir(*d));
        }
    }

    /// When, in original fight milliseconds, the `k`-th man falls (the k-th
    /// of n at `(2k + 1) × 7200 / 2n`).
    pub fn falls_at(&self, k: usize) -> u32 {
        let n = self.men.len().max(1) as u32;
        (2 * k as u32 + 1) * gboh_render::units::FIGHT_MS / (2 * n)
    }

    /// Who is down once the first `k` have fallen.
    pub fn down(&self, k: usize) -> u32 {
        self.men[..k.min(self.men.len())].iter().fold(self.before, |m, &i| m | 1 << i)
    }
}

impl Anim {
    /// How many volley frames a volley takes, at most (the shooting strip,
    /// then the longest flight).
    pub fn volley_frames(scene: &Scene, from: (i32, i32), to: (i32, i32)) -> usize {
        let d = (((to.0 - from.0).pow(2) + (to.1 - from.1).pow(2)) as f32).sqrt();
        let speed = scene.layout.zoom.block().0 as f32 / 5.0;
        (FIRE_CELS.len()).max(FIRE_RELEASE + (d / speed) as usize + 2)
    }
}

/// The art a weapon throws.
pub fn missile_of(weapon: gboh_game::missile::Weapon) -> Missile {
    use gboh_game::missile::Weapon::*;
    match weapon {
        Bow | MountedBow => Missile::Arrow,
        Javelin | MountedJavelin => Missile::Javelin,
        Sling => Missile::Stone,
        Bolt => Missile::Bolt,
    }
}

/// A unit's part in a shock combat, for the results dialogs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatRow {
    pub unit: usize,
    /// Cohesion hits taken in this combat.
    pub current: i32,
    pub routed: bool,
    pub eliminated: bool,
}

/// A shock combat fought: its attackers and defenders, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combat {
    pub attackers: Vec<CombatRow>,
    pub defenders: Vec<CombatRow>,
    pub hex: Hex,
}

pub struct Play {
    pub game: Game,
    /// The combats fought since the results were last shown.
    pub combats: Vec<Combat>,
    /// A turn has ended since the game overview was last shown.
    pub turn_ended: bool,
    /// The running commentary, oldest first.
    pub commentary: Vec<String>,
    /// The missile volley button was pressed: the next click on an enemy
    /// fires at it.
    pub aiming: bool,
    /// Sounds to play for what just happened.
    pub events: Vec<crate::sound::Event>,
    /// Moves and volleys to animate.
    pub anims: Vec<Anim>,
    /// Fallen men lying on the battlefield.
    pub corpses: Vec<gboh_render::units::Corpse>,
    /// Who is down in each unit that has lost men, as drawn, and once every
    /// logged fall has been shown.
    shown: std::collections::HashMap<usize, u32>,
    latest: std::collections::HashMap<usize, u32>,
    /// Destroyed units still drawn until their fall has been shown.
    lingering: std::collections::HashSet<usize>,
    /// The screen grid, for where each man stands.
    layout: gboh_render::terrain::Layout,
    seed: u32,
    /// The sides the computer plays (scenario nation byte) and their
    /// scripts.
    ai: Vec<(u8, gboh_game::ai::Machine)>,
    /// The computer plays one leader per `step_computer` call (the viewer
    /// paces it) instead of right after a human leader finishes.
    pub demo: bool,
    selected: Option<usize>,
    leader: Option<usize>,
    pub status: String,
    pub over: bool,
    /// The army that won, once the battle is over.
    pub winner: Option<usize>,
    /// What to tell the player, oldest first.
    pub notices: Vec<Notice>,
    /// The player's orders so far, for saving.
    pub orders: Vec<Order>,
    /// The leader last told he has no orders left.
    told_no_orders: Option<usize>,
    /// The game as it was before each of the last unit's moves and turns
    /// this activation, for Undo (the original's undo list, `FUN_00521fe4`:
    /// one unit's actions; another unit's, a volley, a shock, a rally, a
    /// recovery or a group move clears it).
    undo: Vec<(usize, Game)>,
}

impl Play {
    /// `ai`: the sides the computer plays (nation byte) with their plans.
    pub fn new(mut game: Game, ai: Vec<(u8, gboh_game::ai::Machine)>) -> Self {
        let leader = game.begin_turn();
        let layout = gboh_render::terrain::Layout::new(&game.map, gboh_render::terrain::Zoom::In);
        let demo = ai.len() > 1;
        let mut p = Self { game, combats: Vec::new(), turn_ended: false, commentary: Vec::new(), aiming: false, ai, demo, events: vec![crate::sound::Event::Start], anims: Vec::new(), corpses: Vec::new(), shown: Default::default(), latest: Default::default(), lingering: Default::default(), layout, seed: 12345, selected: None, leader, status: String::new(), over: false, winner: None, notices: Vec::new(), told_no_orders: None, orders: Vec::new(), undo: Vec::new() };
        p.say("battle begins");
        p.comment(String::from("--- Turn #1 ---"));
        p.announce_leader();
        if !p.demo {
            p.computer_turns();
        }
        p
    }

    /// While the active leader is the computer's, let its script give his
    /// orders and finish him.
    fn computer_turns(&mut self) {
        while self.play_computer() {}
    }

    /// If the active leader is the computer's, play him (orders, then
    /// finish with momentum); returns whether one was played.
    pub fn step_computer(&mut self) -> bool {
        self.orders.push(Order::StepComputer);
        self.play_computer()
    }

    fn play_computer(&mut self) -> bool {
        let Some(l) = self.leader.filter(|_| !self.over) else { return false };
        let side = self.game.battle.units[l].side;
        let Some(i) = self.ai.iter().position(|(s, _)| *s == side) else { return false };
        let name = self.game.battle.units[l].name.clone();
        if let Err(e) = self.game.ai_orders(&mut self.ai[i].1) {
            println!("AI error: {e}");
        }
        println!("computer: {name}");
        self.finish_one(true);
        true
    }

    /// Replay saved `orders`, showing nothing: what each brings about is
    /// taken from the log and landed at once (bodies on the ground, units
    /// as they end up), and nothing is left to animate, sound or tell.
    pub fn replay(&mut self, orders: &[Order]) {
        for &o in orders {
            self.apply(o);
            self.take_log();
            for a in std::mem::take(&mut self.anims) {
                self.land(&a);
            }
        }
        self.events.clear();
        self.notices.clear();
    }

    /// Give `order` (as the player would: from a save, or the other player
    /// of a network game).
    pub fn apply(&mut self, order: Order) {
        match order {
            Order::Click { hex, right } => {
                self.click(hex, right);
            }
            Order::ArmVolley => self.arm_volley(),
            Order::GroupMove => {
                self.group_move();
            }
            Order::Rally => {
                self.rally();
            }
            Order::Recover => {
                self.recover();
            }
            Order::Turn { left } => {
                self.turn(left);
            }
            Order::AboutFace => {
                self.about_face();
            }
            Order::Undo => {
                self.undo();
            }
            Order::Finish { momentum } => {
                self.finish(momentum);
            }
            Order::StepComputer => {
                self.step_computer();
            }
        }
    }

    /// Turn the view half round (or back): the corpses lying about and those
    /// still to fall are seen from the other side.
    pub fn set_reversed(&mut self, reversed: bool) {
        if self.layout.reversed == reversed {
            return;
        }
        self.layout.reversed = reversed;
        self.corpses.iter_mut().for_each(turn_corpse);
        self.anims.iter_mut().for_each(Anim::turn);
    }

    /// How the battle ended for the player, once it has: the nation whose
    /// end picture to show and whether it won. The player is the one army
    /// the computer does not play; with both or neither played by hand, the
    /// winner (the original shows victory when a human won, or when none
    /// played).
    pub fn ending(&self) -> Option<(u8, bool)> {
        let a = self.winner?;
        let human: Vec<usize> = (0..2).filter(|&k| !self.ai.iter().any(|(n, _)| *n == self.game.armies[k].0)).collect();
        Some(match human.as_slice() {
            [h] => (self.game.armies[*h].0, *h == a),
            _ => (self.game.armies[a].0, true),
        })
    }

    /// Add a line to the running commentary (the last hundred kept).
    fn comment(&mut self, line: String) {
        self.commentary.push(line);
        if self.commentary.len() > 100 {
            self.commentary.remove(0);
        }
    }

    /// Say who is now active.
    fn announce_leader(&mut self) {
        if let Some(l) = self.leader {
            let name = self.game.battle.units[l].name.clone();
            self.comment(format!("{name} is now active."));
        }
    }

    fn name(&self, unit: usize) -> String {
        self.game.battle.units[unit].name.clone()
    }

    fn say(&mut self, what: &str) {
        let g = &self.game;
        let who = self.leader.map_or("-".to_string(), |l| g.battle.units[l].name.clone());
        let com = g.points.as_ref().map_or(String::new(), |c: &CommandPoints| format!(" Com {}/{}", c.left(), c.max));
        let sel = self.selected.map_or(String::new(), |u| {
            let u = &g.battle.units[u];
            format!(" | {} MP {} hits {}/{}", u.name, u.mp, u.hits, u.tq)
        });
        self.status = format!("Turn {} {who}{com}{sel} | {what}", g.activation.turn + 1);
        println!("{what}");
    }

    /// Turn what the game logged into walks to animate and sounds.
    pub fn take_log(&mut self) {
        for h in std::mem::take(&mut self.game.log) {
            match h {
                gboh_game::game::Happened::Moved { unit, mut path, running } => {
                    // a unit fleeing off the map logs the hex beyond the edge
                    let map = &self.game.map;
                    path.retain(|h| h.x >= 0 && h.y >= 0 && (h.x as usize) < map.width && (h.y as usize) < map.height);
                    let class = gboh_game::movement::MoveClass::of_type(self.game.battle.units[unit].type_id);
                    if let (Some(c), false) = (class, running) {
                        self.events.push(crate::sound::Event::Move(c));
                    }
                    if path.len() >= 2 {
                        self.anims.push(Anim::Walk { unit, path, running });
                    }
                }
                gboh_game::game::Happened::Casualties { unit, hex, facing, kind, count, hits } => {
                    let line = format!("{} takes {count} cohesion hit{}.", self.name(unit), if count == 1 { "" } else { "s" });
                    self.comment(line);
                    let kind = match kind {
                        gboh_game::game::Casualty::Shock => 0,
                        gboh_game::game::Casualty::Arrow => 1,
                        gboh_game::game::Casualty::Trample => 2,
                    };
                    if kind == 0 {
                        self.combat_row(unit).into_iter().for_each(|r| r.current += count);
                    }
                    let before = self.down_now(unit, hits - count);
                    let o = &self.game.battle.units[unit];
                    let formation = gboh_render::units::Formation::of_type(o.type_id);
                    let n = formation.figures().len();
                    let target = formation.standing(hits.max(0) as u16, o.tq);
                    let count = (n - before.count_ones() as usize).saturating_sub(target);
                    // in shock the men facing the enemy are likelier to fall
                    let enemies = (kind == 0).then(|| self.enemy_sides(unit)).flatten();
                    let men = gboh_render::units::pick_fallen(o.type_id, before, count, enemies);
                    self.fall(unit, hex, facing, before, men, kind, false);
                }
                gboh_game::game::Happened::Shock { attackers, defenders } => {
                    let names = |us: &[usize]| us.iter().map(|&u| self.name(u)).collect::<Vec<_>>().join(", ");
                    let line = format!("{} attack{} {}.", names(&attackers), if attackers.len() == 1 { "s" } else { "" }, names(&defenders));
                    self.comment(line);
                    let row = |&unit: &usize| CombatRow { unit, current: 0, routed: false, eliminated: false };
                    let hex = defenders.first().map_or(Hex::new(0, 0), |&d| self.game.battle.units[d].hex);
                    self.combats.push(Combat { attackers: attackers.iter().map(row).collect(), defenders: defenders.iter().map(row).collect(), hex });
                    let units = &self.game.battle.units;
                    let face = |u: usize, others: &[usize]| {
                        let at = units[u].hex;
                        let nearest = others.iter().map(|&o| units[o].hex).min_by_key(|h| at.distance(*h));
                        let d = nearest.map_or(0, |h| gboh_game::ai_host::direction(at, h));
                        // the sides of the unit the enemies stand on (the original's
                        // CombatAnimate, from the hexes in contact)
                        let sides = others.iter().fold(0u16, |m, &o| {
                            let d = gboh_game::ai_host::direction(at, units[o].hex);
                            if d > 0 { m | 1 << gboh_render::units::relative_side(units[u].facing, d as u8) } else { m }
                        });
                        (u, if d > 0 { d as u8 } else { units[u].facing }, sides)
                    };
                    let mut fighting: Vec<(usize, u8, u16)> = attackers.iter().map(|&a| face(a, &defenders)).collect();
                    fighting.extend(defenders.iter().map(|&d| face(d, &attackers)));
                    let elephants = fighting.iter().any(|&(u, ..)| {
                        gboh_game::movement::MoveClass::of_type(units[u].type_id) == Some(gboh_game::movement::MoveClass::Elephant)
                    });
                    let sound = crate::sound::Event::Shock { elephants };
                    self.anims.push(Anim::Fight { units: fighting, falls: Vec::new(), sound });
                }
                gboh_game::game::Happened::Routed { unit } => {
                    let line = format!("{} is routing!", self.name(unit));
                    self.comment(line);
                    self.combat_row(unit).into_iter().for_each(|r| r.routed = true);
                    let o = &self.game.battle.units[unit];
                    let mut view = self.view_of(unit, o.hits);
                    view.fallen = Some(self.down_now(unit, o.hits));
                    let mut seed = self.rand();
                    let mut cel = || {
                        seed = seed.wrapping_mul(22695477).wrapping_add(1);
                        ((seed >> 16) & 0x7fff) as usize
                    };
                    use gboh_game::missile::Weapon::{Bow, MountedBow};
                    let bow = matches!(self.game.battle.units[unit].missile, Some((Bow | MountedBow, _)));
                    let debris = gboh_render::units::rout_debris(&self.layout, &view, bow, &mut cel);
                    self.anims.push(Anim::Drop { unit, debris });
                }
                gboh_game::game::Happened::Eliminated { unit, hex, facing } => {
                    let line = format!("{} is eliminated!", self.name(unit));
                    self.comment(line);
                    self.combat_row(unit).into_iter().for_each(|r| r.eliminated = true);
                    let before = self.down_now(unit, self.game.battle.units[unit].hits);
                    let n = gboh_render::units::Formation::of_type(self.game.battle.units[unit].type_id).figures().len();
                    let men: Vec<usize> = (0..n).filter(|i| before & (1 << i) == 0).collect();
                    self.lingering.insert(unit);
                    self.fall(unit, hex, facing, before, men, 3, true);
                }
                gboh_game::game::Happened::Volley { shooter, target, weapon, hit } => {
                    let line = format!("{} shoots at {}.", self.name(shooter), self.name(target));
                    self.comment(line);
                    let sound = crate::sound::Event::Volley(weapon, hit);
                    let missile = missile_of(weapon);
                    let at = self.game.battle.units[target].hex;
                    let map = &self.game.map;
                    // a spent missile in the hex (none on water)
                    let debris = (at.x >= 0 && at.y >= 0 && map.elevation_at(at.x as usize, at.y as usize) > 0).then(|| {
                        let (r1, r2, cel) = (self.rand(), self.rand(), self.rand() as usize);
                        let set = gboh_render::missiles::Missile::ALL.iter().position(|&m| m == missile).unwrap_or(0);
                        gboh_render::units::Corpse {
                            hex: (at.x as usize, at.y as usize),
                            type_id: self.game.battle.units[target].type_id,
                            kind: gboh_render::units::MISSILE + set,
                            cel,
                            offset: gboh_render::missiles::debris_offset(missile, r1, r2),
                        }
                    });
                    self.anims.push(Anim::Volley { shooter, target, missile, sound, debris });
                }
            }
        }
    }

    /// `unit`'s row in the latest combat it fought, if any.
    fn combat_row(&mut self, unit: usize) -> Option<&mut CombatRow> {
        self.combats.iter_mut().rev().find_map(|c| c.attackers.iter_mut().chain(c.defenders.iter_mut()).find(|r| r.unit == unit))
    }

    /// Who is down in `unit` after its logged falls; for one that has lost
    /// none yet, as many as `hits` take.
    fn down_now(&self, unit: usize, hits: i32) -> u32 {
        match self.latest.get(&unit) {
            // recovered since its last fall was shown: men are back up
            Some(&m) if self.shown.get(&unit) == Some(&m) => self.stood_up(unit, m, hits),
            Some(&m) => m,
            None => gboh_render::units::fallen_by_hits(&self.view_of(unit, hits)),
        }
    }

    /// `unit` drawn where it is, with `hits`.
    fn view_of(&self, unit: usize, hits: i32) -> UnitView {
        let o = &self.game.battle.units[unit];
        UnitView {
            hex: (o.hex.x.max(0) as usize, o.hex.y.max(0) as usize),
            facing: o.facing,
            type_id: o.type_id,
            nation: o.nation,
            id: o.source,
            routed: false,
            walk: None,
            hits: hits.max(0) as u16,
            tq: o.tq,
            act: None,
            fallen: None,
        }
    }

    /// The sides of `unit` (relative to its front) where the enemies of the
    /// fight just queued stand.
    fn enemy_sides(&self, unit: usize) -> Option<[bool; 9]> {
        let Some(Anim::Fight { units, .. }) = self.anims.last() else { return None };
        let &(_, _, bits) = units.iter().find(|f| f.0 == unit)?;
        Some(std::array::from_fn(|d| bits & (1 << d) != 0))
    }

    /// Queue `men` of `unit` (on `hex`, facing `facing`; `before` down)
    /// falling, each a body where he stood (the original turns the man's
    /// own sprite into one, `FUN_004e77de`).
    #[allow(clippy::too_many_arguments)]
    fn fall(&mut self, unit: usize, hex: Hex, facing: u8, before: u32, men: Vec<usize>, kind: usize, destroyed: bool) {
        let after = men.iter().fold(before, |m, &i| m | 1 << i);
        self.latest.insert(unit, after);
        let map = &self.game.map;
        let on_map = hex.x >= 0 && hex.y >= 0 && (hex.x as usize) < map.width && (hex.y as usize) < map.height;
        let mut view = self.view_of(unit, 0);
        view.hex = (hex.x.max(0) as usize, hex.y.max(0) as usize);
        view.facing = facing;
        view.fallen = Some(before);
        let places = gboh_render::units::figure_places(&self.layout, &view);
        let type_id = view.type_id;
        let mut corpses = Vec::new();
        let mut runaways = Vec::new();
        for (k, &i) in men.iter().enumerate() {
            let Some(&(_, on, offset, _)) = places.iter().find(|p| p.0 == i).filter(|_| on_map) else { continue };
            let cel = self.rand() as usize;
            corpses.push(gboh_render::units::Corpse { hex: on, type_id, kind, cel, offset });
            // in shock a horse may bolt, a chariot run away (FUN_004eb617)
            if kind == 0 {
                let (roll, second) = (self.rand() % 100, self.rand() % 100);
                if let Some(r) = Runaway::left_by(type_id, roll, second) {
                    let dir = (self.rand() % 8 + 1) as usize;
                    let mut seed = self.rand();
                    let mut rand = || {
                        seed = seed.wrapping_mul(22695477).wrapping_add(1);
                        (seed >> 16) & 0x7fff
                    };
                    let path = gboh_render::runaways::path(&self.game.map, &self.layout, on, dir, &mut rand);
                    runaways.push((k, r, path));
                }
            }
        }
        // bodies off the map are not kept; their men still fall
        let men = if on_map { men } else { Vec::new() };
        self.push_fall(Fallen { unit, before, men, corpses, destroyed, runaways });
    }

    /// The viewer's own random numbers (0..32768).
    fn rand(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(22695477).wrapping_add(1);
        (self.seed >> 16) & 0x7fff
    }

    /// Queue men falling: during the fight they belong to if one was just
    /// queued with their unit in it, else on their own.
    fn push_fall(&mut self, f: Fallen) {
        if let Some(Anim::Fight { units, falls, .. }) = self.anims.last_mut()
            && units.iter().any(|&(u, ..)| u == f.unit)
        {
            falls.push(f);
            return;
        }
        self.anims.push(Anim::Fall(f));
    }

    /// Fight `fight` at `ms` original milliseconds: its units fight, and
    /// men fall one by one, evenly spread over the fight (the original:
    /// the k-th of n at `(2k + 1) × 7200 / 2n` ms). Returns the bodies down
    /// so far.
    pub fn fight(&self, views: &mut [UnitView], fight: &Anim, ms: u32) -> Vec<gboh_render::units::Corpse> {
        let Anim::Fight { units, falls, .. } = fight else { return Vec::new() };
        let ids = |u: usize| self.game.battle.units[u].source;
        for &(u, toward, sides) in units {
            if let Some(v) = views.iter_mut().find(|v| v.id == ids(u)) {
                v.act = Some(gboh_render::units::Act::Fight { ms, sides, toward });
            }
        }
        let mut down = Vec::new();
        for f in falls {
            let gone = (0..f.men.len()).filter(|&k| f.falls_at(k) <= ms).count();
            if let Some(v) = views.iter_mut().find(|v| v.id == ids(f.unit)) {
                v.fallen = Some(f.down(gone));
            }
            down.extend_from_slice(&f.corpses[..gone.min(f.corpses.len())]);
        }
        down
    }

    /// The selected unit, if any.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The active leader, if any.
    pub fn leader(&self) -> Option<usize> {
        self.leader
    }

    /// Frame `frame` of `unit` throwing down its arms.
    pub fn dropping(&self, views: &mut [UnitView], unit: usize, frame: usize) {
        let id = self.game.battle.units[unit].source;
        if let Some(v) = views.iter_mut().find(|v| v.id == id) {
            v.act = Some(gboh_render::units::Act::Drop(frame));
        }
    }

    /// Show a fall, or the end of a fight: the bodies go down and the
    /// units are drawn as they now are.
    pub fn land(&mut self, anim: &Anim) {
        let falls: Vec<&Fallen> = match anim {
            Anim::Fall(f) => vec![f],
            Anim::Fight { falls, .. } => falls.iter().collect(),
            Anim::Drop { debris, .. } => {
                self.corpses.extend(debris.iter().copied());
                return;
            }
            Anim::Volley { debris, .. } => {
                self.corpses.extend(debris.iter().copied());
                return;
            }
            _ => return,
        };
        for f in falls {
            self.corpses.extend(f.corpses.iter().copied());
            self.shown.insert(f.unit, f.down(f.men.len()));
            if f.destroyed {
                self.lingering.remove(&f.unit);
            }
        }
    }

    /// Units to draw, with `walking` (a unit, the hex it is leaving, the
    /// next hex, how far along and the frame) drawn on its way.
    /// Units in `pending` (walks not yet shown) are drawn where they
    /// started.
    pub fn views_walking(
        &self,
        scene: &Scene,
        walking: Option<(usize, Hex, Hex, f32, usize, bool)>,
        pending: &[Anim],
    ) -> Vec<UnitView> {
        let mut views = self.views();
        let pending: Vec<(usize, &Vec<Hex>)> = pending
            .iter()
            .filter_map(|a| match a {
                Anim::Walk { unit, path, .. } => Some((*unit, path)),
                Anim::Volley { .. } | Anim::Fall { .. } | Anim::Drop { .. } | Anim::Fight { .. } => None,
            })
            .collect();
        // units shown on the move even if already out of the battle
        let shown = pending.iter().map(|(u, _)| *u).chain(walking.map(|w| w.0));
        for u in shown.collect::<Vec<_>>() {
            let o = &self.game.battle.units[u];
            if !views.iter().any(|v| v.id == o.source) {
                views.push(UnitView {
                    hex: (o.hex.x as usize, o.hex.y as usize),
                    facing: o.facing,
                    type_id: o.type_id,
                    nation: o.nation,
                    id: o.source,
                    routed: true,
                    walk: None,
                    hits: o.hits.max(0) as u16,
                    tq: o.tq,
                    act: None,
                    fallen: self.shown_down(u),
                });
            }
        }
        // latest first, so a unit with several pending walks ends up at the
        // start of its earliest
        for (u, path) in pending.iter().rev() {
            let id = self.game.battle.units[*u].source;
            if let (Some(v), Some(h)) = (views.iter_mut().find(|v| v.id == id), path.first()) {
                v.hex = (h.x as usize, h.y as usize);
            }
        }
        if let Some((u, from, to, t, frame, running)) = walking {
            let id = self.game.battle.units[u].source;
            if let Some(v) = views.iter_mut().find(|v| v.id == id) {
                if running {
                    // routed men run the way they are going
                    v.routed = true;
                    let d = gboh_game::ai_host::direction(from, to);
                    if d > 0 {
                        v.facing = d as u8;
                    }
                }
                let a = gboh_render::objects::anchor_at(&scene.map, &scene.layout, (from.x as usize, from.y as usize), (0, 0));
                let b = gboh_render::objects::anchor_at(&scene.map, &scene.layout, (to.x as usize, to.y as usize), (0, 0));
                v.hex = (from.x as usize, from.y as usize);
                v.walk = Some((frame, (((b.0 - a.0) as f32 * t) as i32, ((b.1 - a.1) as f32 * t) as i32)));
                // each man on his own way, where the walking data has one
                let dir = scene.screen_facing(gboh_game::ai_host::direction(from, to).max(0) as u8);
                let (fh, th) = ((from.x as usize, from.y as usize), (to.x as usize, to.y as usize));
                if let Some(ms) = scene.step_ms_between(v.type_id, scene.screen_facing(v.facing), dir, fh, th) {
                    v.act = Some(gboh_render::units::Act::Walk { dir, ms: (t * ms as f32) as u32 });
                }
            }
        }
        views
    }

    /// Frame `frame` of `shooter`'s volley at `target`: the shooter's view
    /// gets its shooting pose, and each of its men's missiles is placed on
    /// its way.
    pub fn volley(&self, scene: &Scene, views: &mut [UnitView], shooter: usize, target: usize, missile: Missile, frame: usize) -> Vec<Flight> {
        let (sid, tid) = (self.game.battle.units[shooter].source, self.game.battle.units[target].source);
        let Some(sv) = views.iter_mut().find(|v| v.id == sid) else { return Vec::new() };
        sv.act = Some(gboh_render::units::Act::Shoot(frame.min(FIRE_CELS.len() - 1)));
        let sv = *sv;
        let Some(tv) = views.iter().find(|v| v.id == tid).copied() else { return Vec::new() };
        let (map, layout) = (&scene.map, &scene.layout);
        let (w, h, _) = layout.zoom.block();
        let men = gboh_render::units::figure_spots(map, layout, &sv);
        let marks = gboh_render::units::figure_spots(map, layout, &tv);
        if frame < FIRE_RELEASE || marks.is_empty() {
            return Vec::new();
        }
        let facing = scene.screen_facing(sv.facing);
        let mut seed = (sid as u32).wrapping_mul(2654435761) ^ (tid as u32).wrapping_mul(40503);
        let mut r = |n: i32| {
            seed = seed.wrapping_mul(22695477).wrapping_add(1);
            ((seed >> 16) & 0x7fff) as i32 % n
        };
        men.iter()
            .filter_map(|&((x, y), _)| {
                // from by the man's shoulder to a man of the target, scattered
                let from = (x, y - h * 3 / 5);
                let ((mx, my), _) = marks[r(marks.len() as i32) as usize];
                let spread = w / 7;
                let to = (mx + r(2 * spread + 1) - spread, my - h / 4 + r(spread + 1) - spread / 2);
                let total = Anim::volley_frames(scene, from, to) - FIRE_RELEASE - 1;
                let t = (frame - FIRE_RELEASE) as f32 / total.max(1) as f32;
                let d = (((to.0 - from.0).pow(2) + (to.1 - from.1).pow(2)) as f32).sqrt();
                (t <= 1.0).then_some(Flight { missile, facing, from, to, lift: h / 4 + (d / 4.0) as i32, t })
            })
            .collect()
    }

    /// Units to draw, by scenario index.
    pub fn views(&self) -> Vec<UnitView> {
        self.game
            .battle
            .units
            .iter()
            .enumerate()
            .filter(|(i, u)| u.on_field() || self.lingering.contains(i))
            .filter(|(_, u)| u.hex.x >= 0 && u.hex.y >= 0)
            .map(|(i, u)| UnitView {
                hex: (u.hex.x as usize, u.hex.y as usize),
                facing: u.facing,
                type_id: u.type_id,
                nation: u.nation,
                id: u.source,
                routed: u.state == UnitState::Routed,
                walk: None,
                hits: u.hits.max(0) as u16,
                tq: u.tq,
                act: None,
                fallen: self.shown_down(i),
            })
            .collect()
    }

    /// Who is drawn down in `unit`: as its falls left it, but with men
    /// standing up again once it has recovered cohesion and none of its
    /// falls are waiting (the original's `RestoreCohesion`).
    fn shown_down(&self, unit: usize) -> Option<u32> {
        let mask = *self.shown.get(&unit)?;
        if self.latest.get(&unit) != Some(&mask) {
            return Some(mask);
        }
        Some(self.stood_up(unit, mask, self.game.battle.units[unit].hits))
    }

    /// `mask` with men standing up until no more are down than `hits`
    /// take (🟡 which rise first is not traced: the lowest first).
    fn stood_up(&self, unit: usize, mut mask: u32, hits: i32) -> u32 {
        let o = &self.game.battle.units[unit];
        let formation = gboh_render::units::Formation::of_type(o.type_id);
        let allowed = formation.figures().len() - formation.standing(hits.max(0) as u16, o.tq);
        while mask.count_ones() as usize > allowed {
            mask &= mask - 1;
        }
        mask
    }

    /// Scenario indices of the units the active leader can order.
    pub fn flagged(&self) -> Vec<usize> {
        self.game.orderable().into_iter().map(|u| self.game.battle.units[u].source).collect()
    }

    /// Hexes to light up: the selected unit's reachable ones, or where the
    /// selected leader can move.
    pub fn lit(&self) -> Vec<Hex> {
        match self.selected {
            Some(u) if Some(u) == self.leader && self.game.group_move.is_some() => self.game.group_move_options(),
            Some(u) if Some(u) == self.leader => self.game.leader_options(),
            Some(u) if self.game.battle.units[u].state == UnitState::Routed => Vec::new(),
            Some(u) => highlighted(&self.game.battle, &self.game.map, u, self.game.battle.units[u].mp),
            None => Vec::new(),
        }
    }

    fn unit_at(&self, hex: Hex) -> Option<usize> {
        let b = &self.game.battle;
        (0..b.units.len()).find(|&u| b.units[u].on_field() && b.hexes(u).contains(&hex) && b.units[u].leader.is_none())
    }

    /// Charge the leader for ordering `unit`; false if he has no points.
    fn charge(&mut self, unit: usize) -> bool {
        let Some(l) = self.leader else { return false };
        let Some(points) = &mut self.game.points else { return false };
        if !points.can_order(unit) {
            return false;
        }
        points.unit_done(&self.game.battle, l, unit);
        if points.left() == 0 && self.told_no_orders != Some(l) {
            self.told_no_orders = Some(l);
            self.notices.push(Notice::NoOrders);
        }
        true
    }

    /// Whether the selected unit can shoot: it has missiles left and has
    /// not fired.
    pub fn can_fire(&self) -> bool {
        self.selected.is_some_and(|u| {
            let o = &self.game.battle.units[u];
            o.missile.is_some_and(|(_, left)| left > 0) && !o.fired && self.game.orderable().contains(&u)
        })
    }

    /// The missile volley button: arm a volley for the next click.
    pub fn arm_volley(&mut self) {
        self.orders.push(Order::ArmVolley);
        if self.can_fire() {
            self.aiming = true;
            self.say("pick a target");
        }
    }

    /// A click on `hex`; returns whether the battlefield changed.
    pub fn click(&mut self, hex: Hex, right: bool) -> bool {
        self.orders.push(Order::Click { hex, right });
        if self.over {
            return false;
        }
        let target = self.unit_at(hex);
        let orderable = self.game.orderable();
        // an armed volley: a left click fires (or, off an enemy, disarms)
        let right = right || std::mem::take(&mut self.aiming) && !right && target.is_some_and(|t| !orderable.contains(&t));
        // the active leader: select him, or move him to a lit hex
        if let Some(l) = self.leader
            && !right
        {
            if self.game.battle.units[l].hex == hex && target.is_none_or(|t| !orderable.contains(&t)) {
                self.selected = Some(l);
                self.say("leader selected");
                return true;
            }
            if self.selected == Some(l) && self.game.group_move.is_some() && self.lit().contains(&hex) {
                self.undo.clear();
                self.game.do_group_move(hex);
                self.say("group moved");
                return true;
            }
            if self.selected == Some(l) && self.lit().contains(&hex) {
                let before = self.game.clone();
                if !self.charge(l) {
                    self.say("no command points");
                    return false;
                }
                self.game.move_leader(hex);
                self.keep_undo(l, before);
                self.say("leader moved");
                return true;
            }
        }
        match (self.selected, target) {
            (_, Some(t)) if !right && orderable.contains(&t) => {
                self.selected = Some(t);
                self.events.push(crate::sound::Event::Select);
                self.say("selected");
                true
            }
            (Some(s), Some(t)) if right => {
                self.undo.clear();
                let name = self.game.battle.units[t].name.clone();
                match self.game.order_volley(s, t) {
                    Ok(h) if self.charge(s) => {
                        self.say(&format!("volley at {name}: {h} hits"));
                        let routed = self.game.battle.units[t].state != UnitState::Fighting;
                        self.notices.push(Notice::Volley { target: t, hits: h, routed });
                    }
                    Ok(_) => self.say("no command points"),
                    Err(_) => self.say("cannot volley at that"),
                }
                true
            }
            (Some(s), Some(t)) if self.game.battle.units[t].side != self.game.battle.units[s].side => {
                let ok = self.game.designate_shock(s, t);
                if ok {
                    self.undo.clear();
                    self.charge(s);
                }
                self.say(if ok { "shock designated" } else { "cannot shock that" });
                true
            }
            (Some(s), None) if self.lit().contains(&hex) => {
                let before = self.game.clone();
                if !self.charge(s) {
                    self.say("no command points");
                    return false;
                }
                match self.game.order_move(s, hex) {
                    Ok(fire) => {
                        // a move that drew fire cannot be taken back
                        if fire.is_empty() {
                            self.keep_undo(s, before);
                        } else {
                            self.undo.clear();
                        }
                        let hits: i32 = fire.iter().map(|&(_, h)| h).sum();
                        self.say(&if fire.is_empty() { "moved".into() } else { format!("moved; reaction fire {hits} hits") });
                    }
                    Err(e) => self.say(&format!("{e:?}")),
                }
                true
            }
            _ => false,
        }
    }

    /// Try a group move with the active leader (all his orders); if it is
    /// granted, he is selected with his group's destinations lit.
    pub fn group_move(&mut self) -> bool {
        self.orders.push(Order::GroupMove);
        if let Some(why) = self.game.group_move_refused() {
            self.say(&format!("no group move: {why}"));
            return false;
        }
        self.undo.clear();
        let result = self.game.try_group_move();
        self.selected = self.leader;
        self.say(&format!("group move: {result:?}"));
        true
    }

    /// Rally the selected routed unit (one order).
    pub fn rally(&mut self) -> bool {
        self.orders.push(Order::Rally);
        let Some(s) = self.selected else { return false };
        if self.game.battle.units[s].state != UnitState::Routed {
            self.say("only routed units rally");
            return false;
        }
        if !self.charge(s) {
            self.say("no command points");
            return false;
        }
        self.undo.clear();
        let ok = self.game.rally(s);
        self.say(if ok { "rallied" } else { "rally failed: terminal rout" });
        let hits = ok.then(|| self.game.battle.units[s].hits);
        self.notices.push(Notice::Rally { unit: s, hits });
        self.selected = None;
        true
    }

    /// Recover cohesion for the selected unit (one order).
    pub fn recover(&mut self) -> bool {
        self.orders.push(Order::Recover);
        let Some(s) = self.selected else { return false };
        if self.game.battle.units[s].state != UnitState::Fighting || self.game.battle.units[s].hits == 0 {
            self.say("nothing to recover");
            return false;
        }
        if !self.charge(s) {
            self.say("no command points");
            return false;
        }
        self.undo.clear();
        let n = self.game.recover(s);
        self.say(&format!("{n} cohesion hits removed"));
        self.selected = None;
        true
    }

    /// Turn the selected unit.
    pub fn turn(&mut self, left: bool) -> bool {
        self.orders.push(Order::Turn { left });
        let Some(s) = self.selected else { return false };
        let before = self.game.clone();
        if !self.charge(s) {
            self.say("no command points");
            return false;
        }
        let map = self.game.map.clone();
        match orders::turn(&mut self.game.battle, &map, s, left) {
            Ok(()) => {
                self.keep_undo(s, before);
                self.say("turned")
            }
            Err(e) => self.say(&format!("{e:?}")),
        }
        true
    }

    /// About face for the selected unit.
    pub fn about_face(&mut self) -> bool {
        self.orders.push(Order::AboutFace);
        let Some(s) = self.selected else { return false };
        let u = &self.game.battle.units[s];
        if u.mp < 3 || u.state != UnitState::Fighting {
            self.say("cannot about face");
            return false;
        }
        let before = self.game.clone();
        if !self.charge(s) {
            self.say("no command points");
            return false;
        }
        match orders::about_face(&mut self.game.battle, s) {
            Ok(()) => {
                self.keep_undo(s, before);
                self.say("about face")
            }
            Err(e) => self.say(&format!("{e:?}")),
        }
        true
    }

    /// Remember the game as it was before `unit`'s move or turn; another
    /// unit's clears what was kept for the last.
    fn keep_undo(&mut self, unit: usize, before: Game) {
        if self.undo.last().is_some_and(|(u, _)| *u != unit) {
            self.undo.clear();
        }
        self.undo.push((unit, before));
    }

    /// The cursor for the mouse over `hex` (the original's `ChangeCursor`
    /// names): a missile's while a volley is armed and the hex holds a unit
    /// the selected one can shoot (`FUN_0056ddca`), the sword over one it
    /// can shock (`FUN_00537261`), else none (the arrow).
    pub fn cursor_at(&self, hex: Hex) -> Option<&'static str> {
        let s = self.selected?;
        let t = self.unit_at(hex)?;
        if self.aiming {
            use gboh_game::missile::Weapon::*;
            return self.game.can_volley(s, t).then(|| match self.game.battle.units[s].missile.map(|m| m.0) {
                Some(Sling) => "SLING",
                Some(Javelin | MountedJavelin) => "JAVELIN",
                Some(Bolt) => "BOLT",
                _ => "ARROW",
            });
        }
        (self.game.orderable().contains(&s) && self.game.can_shock(s, t)).then_some("SWORD")
    }

    /// Whether the selected unit can about face (three movement points,
    /// not routed, one the active leader may order).
    pub fn can_about_face(&self) -> bool {
        self.selected.is_some_and(|u| {
            let o = &self.game.battle.units[u];
            o.mp >= 3 && o.state == UnitState::Fighting && self.game.orderable().contains(&u)
        })
    }

    /// Whether there is a move or turn to take back.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() && !self.over
    }

    /// Take back the last unit's last move or turn: it stands as before,
    /// and its order is given back.
    pub fn undo(&mut self) -> bool {
        self.orders.push(Order::Undo);
        let Some((unit, before)) = self.undo.pop() else { return false };
        self.game = before;
        self.game.log.clear();
        self.selected = Some(unit);
        self.say("undone");
        true
    }

    /// Finish the active leader (and the turn after the last one), trying
    /// for momentum if asked, then let the computer play its leaders.
    pub fn finish(&mut self, momentum: bool) -> bool {
        self.orders.push(Order::Finish { momentum });
        if self.over {
            return false;
        }
        let before = self.leader;
        let tried = momentum && self.game.can_attempt_momentum();
        self.finish_one(momentum);
        if let (Some(leader), true) = (before, tried) {
            self.notices.push(Notice::Momentum { leader, won: self.leader == before });
        }
        if !self.demo {
            self.computer_turns();
        }
        true
    }

    fn finish_one(&mut self, momentum: bool) {
        self.selected = None;
        self.undo.clear();
        let (outcomes, next) = self.game.finish_leader(momentum);
        let routed: usize = outcomes.iter().map(|o| o.routed.len() + o.eliminated.len()).sum();
        self.leader = next;
        if next.is_some() {
            self.announce_leader();
        }
        if next.is_none() {
            if let Verdict::Won(a) = self.game.end_turn() {
                self.over = true;
                self.winner = Some(a);
                let nation = self.game.armies[a].0;
                let won = self.ai.len() != 1 || self.ai[0].0 != nation;
                self.events.push(crate::sound::Event::End { won });
                self.say(&format!("army of nation {nation} wins"));
                return;
            }
            self.turn_ended = true;
            self.leader = self.game.begin_turn();
            let line = format!("--- Turn #{} ---", self.game.activation.turn + 1);
            self.comment(line);
            self.announce_leader();
            self.say(&format!("{} shocks, {routed} routed; new turn", outcomes.len()));
        } else {
            self.say(&format!("{} shocks, {routed} routed", outcomes.len()));
        }
    }
}

/// The face centre of map hex `(x, y)` in `scene`'s canvas pixels.
fn centre(scene: &Scene, x: usize, y: usize) -> (i32, i32) {
    let (w, _, face) = scene.layout.zoom.block();
    let (r, c) = scene.layout.to_tile(x, y);
    let (bx, by) = scene.layout.block_origin(r, c);
    let z = scene.map.elevation_at(x, y) as i32;
    (bx + w / 2, by - z * scene.layout.zoom.level_step() + face / 2)
}

/// The map hex under canvas pixel `(px, py)`.
pub fn pick(scene: &Scene, px: i32, py: i32) -> Option<Hex> {
    let (w, _, _) = scene.layout.zoom.block();
    let mut best = None;
    for x in 0..scene.map.width {
        for y in 0..scene.map.height {
            let (cx, cy) = centre(scene, x, y);
            let d = (cx - px).pow(2) + (cy - py).pow(2);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, Hex::new(x as i32, y as i32)));
            }
        }
    }
    best.filter(|&(d, _)| d <= (w / 2) * (w / 2)).map(|(_, h)| h)
}

/// Brighten `hexes` on `canvas` through remap table 3, as the game lights a
/// unit's movement range.
pub fn light(canvas: &mut Canvas, scene: &Scene, hexes: &[Hex], pal: &Palette) {
    let (w, _, face) = scene.layout.zoom.block();
    let (hw, hf) = (w / 2, face / 2);
    for h in hexes {
        let (cx, cy) = centre(scene, h.x as usize, h.y as usize);
        for dy in -hf..=hf {
            for dx in -hw..=hw {
                // pointy-topped hexagon
                if dy.abs() * hw > hf * hw - dx.abs() * hf / 2 {
                    continue;
                }
                let (x, y) = (cx + dx, cy + dy);
                if x < 0 || y < 0 || x as usize >= canvas.width || y as usize >= canvas.height {
                    continue;
                }
                let i = y as usize * canvas.width + x as usize;
                canvas.pixels[i] = pal.remap[3][canvas.pixels[i] as usize];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gboh_formats::map::Map;
    use gboh_formats::scn::Scenario;

    /// Plays a few orders at Hydaspes; runs only with `GBOH_DATA` set to the
    /// game's DATA directory.
    #[test]
    fn hydaspes_orders() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let scn = Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let mut p = Play::new(Game::new(&scn, map, 1), Vec::new());
        let agema = p.game.battle.units.iter().position(|u| u.name == "Agema Companions").unwrap();
        assert!(p.click(Hex::new(13, 21), false), "select");
        assert_eq!(p.selected, Some(agema));
        assert!(p.lit().contains(&Hex::new(21, 20)));
        assert!(p.click(Hex::new(21, 20), false), "move");
        p.take_log();
        assert_eq!(p.game.battle.units[agema].hex, Hex::new(21, 20));
        // the move is queued for animation, from the start to the end hex
        let Some(Anim::Walk { unit: u, path, .. }) = p.anims.last() else { panic!("no walk queued") };
        assert_eq!(*u, agema);
        assert_eq!((path[0], *path.last().unwrap()), (Hex::new(13, 21), Hex::new(21, 20)));
        println!("{}", p.status);
        // Agema is in Coenus's group: 2 command points
        assert_eq!(p.game.points.as_ref().unwrap().left(), 5);
        assert!(p.turn(true));
        // move Alexander himself
        let alexander = p.leader.unwrap();
        let at = p.game.battle.units[alexander].hex;
        assert!(p.click(at, false), "select leader");
        let to = *p.lit().iter().find(|&&h| h != at).expect("leader can move");
        assert!(p.click(to, false), "move leader");
        assert_eq!(p.game.battle.units[alexander].hex, to);
        assert!(p.lit().is_empty(), "a leader moves once");
        assert!(p.finish(false));
        println!("{}", p.status);
    }

    /// Undo takes back the last unit's moves and turns, one at a time, with
    /// their command points; another unit's move forgets them.
    #[test]
    fn undoes_moves() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let scn = Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let mut p = Play::new(Game::new(&scn, map, 1), Vec::new());
        let agema = p.game.battle.units.iter().position(|u| u.name == "Agema Companions").unwrap();
        let start = p.game.battle.units[agema].clone();
        let points = p.game.points.as_ref().unwrap().left();
        assert!(!p.can_undo());
        p.click(Hex::new(13, 21), false);
        let to = *p.lit().iter().find(|h| start.hex.neighbours().contains(h)).unwrap();
        p.click(to, false);
        assert_eq!(p.game.battle.units[agema].hex, to);
        assert!(p.turn(true));
        assert!(p.can_undo());
        assert!(p.undo());
        assert_eq!(p.game.battle.units[agema].hex, to);
        assert_eq!(p.game.battle.units[agema].facing, start.facing);
        assert!(p.undo());
        assert_eq!((p.game.battle.units[agema].hex, p.game.battle.units[agema].mp), (start.hex, start.mp));
        assert_eq!(p.game.points.as_ref().unwrap().left(), points);
        assert!(!p.undo());
        // about face, undone the same way
        p.game.battle.units[agema].mp = start.max_mp;
        assert!(p.about_face());
        assert_eq!(p.game.battle.units[agema].facing, gboh_game::orders::turned(gboh_game::orders::turned(gboh_game::orders::turned(start.facing, true), true), true));
        assert!(p.undo());
        assert_eq!(p.game.battle.units[agema].facing, start.facing);
    }

    /// The missile volley button arms a volley that the next click on an
    /// enemy fires, and the commentary tells of it.
    #[test]
    fn armed_volley() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let scn = Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let mut p = Play::new(Game::new(&scn, map, 1), Vec::new());
        let shooter = *p
            .game
            .orderable()
            .iter()
            .find(|&&u| p.game.battle.units[u].missile.is_some_and(|(_, n)| n > 0))
            .expect("a missile unit to order");
        let at = p.game.battle.units[shooter].hex;
        assert!(p.click(at, false), "select");
        assert!(p.can_fire(), "{:?}", p.game.battle.units[shooter]);
        // bring an enemy two hexes off
        let side = p.game.battle.units[shooter].side;
        let enemy = p.game.battle.units.iter().position(|u| u.side != side && u.on_field() && u.leader.is_none() && !u.two_hex()).unwrap();
        let spot = [(0, 2), (0, -2), (2, 0), (-2, 0), (2, 1), (-2, 1)]
            .into_iter()
            .map(|(dx, dy)| Hex::new(at.x + dx, at.y + dy))
            .find(|&h| p.unit_at(h).is_none())
            .unwrap();
        p.game.battle.units[enemy].hex = spot;
        p.arm_volley();
        assert!(p.aiming);
        // the cursor over the target is the shooter's missile, the arrow
        // over its own position
        let missile = p.cursor_at(spot);
        assert!(matches!(missile, Some("ARROW" | "SLING" | "JAVELIN" | "BOLT")), "{missile:?}");
        assert_eq!(p.cursor_at(at), None);
        assert!(p.click(spot, false), "fire");
        assert!(!p.aiming);
        assert!(p.game.battle.units[shooter].fired, "{}", p.status);
        assert!(!p.can_fire());
        p.take_log();
        assert!(p.commentary.iter().any(|l| l.contains("shoots at")), "{:?}", p.commentary);
    }

    /// In the reversed view a click still finds the hex under it, and
    /// corpses lying about are turned with the view.
    #[test]
    fn reversed_view() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let scn = Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let pal = gboh_formats::pal::Palette::parse(&std::fs::read(dir.join("GAME.PAL")).unwrap()).unwrap();
        let find = |name: &str| std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name)).map(|e| e.path());
        let mut load = |name: &str| find(name).and_then(|p| std::fs::read(p).ok());
        let titles = crate::eif_title_index(dir);
        let mut load_title = |title: &str| titles.get(title).and_then(|n| std::fs::read(dir.join(n)).ok());
        let scene = Scene::build_view(&scn.map_file, &map, gboh_render::terrain::Zoom::Normal, true, &pal, 1, &[], &mut load, &mut load_title).unwrap();
        for hex in [Hex::new(3, 4), Hex::new(20, 10), Hex::new(31, 25)] {
            let (x, y) = centre(&scene, hex.x as usize, hex.y as usize);
            assert_eq!(pick(&scene, x, y), Some(hex));
        }
        let mut p = Play::new(Game::new(&scn, map, 1), Vec::new());
        let spear = gboh_render::units::Corpse { hex: (3, 4), type_id: 0, kind: gboh_render::units::SPEAR + 2, cel: 0, offset: (10, -4) };
        p.corpses.push(spear);
        p.set_reversed(true);
        assert_eq!((p.corpses[0].kind, p.corpses[0].offset), (gboh_render::units::SPEAR + 6, (-10, 4)));
        p.set_reversed(false);
        assert_eq!(p.corpses[0], spear);
    }

    /// The player's orders queue what the original tells in dialogs: the
    /// volley's result, no orders left, and the momentum attempt.
    #[test]
    fn notices() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let scn = Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let mut p = Play::new(Game::new(&scn, map, 1), Vec::new());
        assert!(!p.game.can_attempt_momentum(), "no order given yet");
        // turn units until the leader's points run out
        let orderable = p.game.orderable();
        for &u in &orderable {
            let at = p.game.battle.units[u].hex;
            if p.unit_at(at) != Some(u) {
                continue;
            }
            p.click(at, false);
            p.turn(true);
            if p.notices.contains(&Notice::NoOrders) {
                break;
            }
        }
        assert!(p.notices.contains(&Notice::NoOrders), "{:?} {}", p.notices, p.status);
        assert_eq!(p.notices.iter().filter(|n| **n == Notice::NoOrders).count(), 1, "told once");
        assert!(p.game.can_attempt_momentum());
        let leader = p.leader.unwrap();
        p.notices.clear();
        p.finish(true);
        assert!(matches!(p.notices.first(), Some(Notice::Momentum { leader: l, .. }) if *l == leader), "{:?}", p.notices);
    }

    /// Replaying the recorded orders from the scenario gives the same
    /// battle: units, the active leader, the turn and the bodies on the
    /// ground (the computer playing the Indians in between).
    #[test]
    fn replays_orders() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let scn = Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let ai = || {
            let mut read = |name: &str| std::fs::read(dir.join(name)).ok().map(|d| String::from_utf8_lossy(&d).into_owned());
            let script = gboh_game::ai::Script::load(&scn.visitor_ai[0], &mut read).unwrap();
            vec![(scn.visitor.nation, gboh_game::ai::Machine::new(script))]
        };
        let mut p = Play::new(Game::new(&scn, map.clone(), 1), ai());
        // the player's leaders charge the nearest enemy, three turns long
        for _ in 0..60 {
            if p.over || p.game.activation.turn >= 3 {
                break;
            }
            for u in p.game.orderable() {
                let at = p.game.battle.units[u].hex;
                if p.unit_at(at) != Some(u) {
                    continue;
                }
                p.click(at, false);
                if let Some(&to) = p.lit().iter().min_by_key(|h| (h.x - 25).abs() + (h.y - 14).abs()) {
                    p.click(to, false);
                }
            }
            p.finish(false);
            p.take_log();
            for a in std::mem::take(&mut p.anims) {
                p.land(&a);
            }
        }
        assert!(p.game.activation.turn >= 1, "some turns played");
        let mut q = Play::new(Game::new(&scn, map, 1), ai());
        q.replay(&p.orders.clone());
        let state = |p: &Play| p.game.battle.units.iter().map(|u| (u.hex, u.facing, u.hits, u.state, u.mp)).collect::<Vec<_>>();
        assert_eq!(state(&q), state(&p));
        assert_eq!((q.leader, q.game.activation.turn), (p.leader, p.game.activation.turn));
        assert_eq!(q.corpses, p.corpses);
        assert_eq!(q.orders, p.orders);
    }

    /// The end picture is the player's: the army the computer does not
    /// play, won or lost; with nobody or everybody playing by hand, the
    /// winner's victory.
    #[test]
    fn endings() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let scn = Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let mut read = |name: &str| std::fs::read(dir.join(name)).ok().map(|d| String::from_utf8_lossy(&d).into_owned());
        let script = gboh_game::ai::Script::load(&scn.visitor_ai[0], &mut read).unwrap();
        let mut p = Play::new(Game::new(&scn, map.clone(), 1), vec![(scn.visitor.nation, gboh_game::ai::Machine::new(script))]);
        let (mac, ind) = (p.game.armies[0].0, p.game.armies[1].0);
        assert_eq!(p.ending(), None);
        p.winner = Some(1);
        assert_eq!(p.ending(), Some((mac, false)));
        p.winner = Some(0);
        assert_eq!(p.ending(), Some((mac, true)));
        let mut hot = Play::new(Game::new(&scn, map, 1), Vec::new());
        hot.winner = Some(1);
        assert_eq!(hot.ending(), Some((ind, true)));
    }

    /// The computer plays the Indians: after a Macedonian leader finishes,
    /// play returns only on a Macedonian leader (or the end).
    #[test]
    fn computer_plays_the_indians() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let read = |n: &str| std::fs::read(dir.join(n.to_ascii_uppercase())).ok();
        let scn = Scenario::parse(&read("HYDASPES.SCN").unwrap()).unwrap();
        let map = Map::parse(&read("HYDASPES.MAP").unwrap()).unwrap();
        let mut text = |n: &str| read(n).map(|d| String::from_utf8_lossy(&d).into_owned());
        let script = gboh_game::ai::Script::load(&scn.visitor_ai[0], &mut text).unwrap();
        let ai = vec![(scn.visitor.nation, gboh_game::ai::Machine::new(script))];
        let mut p = Play::new(Game::new(&scn, map, 1), ai);
        let start: Vec<_> = p.game.battle.units.iter().map(|u| u.hex).collect();
        for _ in 0..12 {
            p.finish(false);
            p.take_log();
            if let Some(l) = p.leader {
                assert_eq!(p.game.battle.units[l].side, scn.home.nation);
            }
        }
        let moved = p.game.battle.units.iter().zip(&start).filter(|(u, h)| u.side == scn.visitor.nation && u.hex != **h).count();
        println!("{} | Indian units moved: {moved} | animations queued {}", p.status, p.anims.len());
        assert!(!p.anims.is_empty(), "the computer's moves are queued for animation");
        // the falls land as the viewer plays them
        for a in std::mem::take(&mut p.anims) {
            p.land(&a);
        }
        println!("fallen men: {}", p.corpses.len());
        assert!(!p.corpses.is_empty(), "casualties leave bodies");
        // optionally dump a frame with the bodies, as a binary PPM
        if let Ok(out) = std::env::var("GBOH_DUMP") {
            let files: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
            let find = |n: &str| files.iter().find(|p| p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(n))).cloned();
            let mut load = |n: &str| find(n).and_then(|p| std::fs::read(p).ok());
            let heads: Vec<(String, Vec<u8>)> = files
                .iter()
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("eif")))
                .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(p).unwrap()[..128].to_vec()))
                .collect();
            let titles = gboh_render::battlefield::eif_titles(heads.iter().map(|(n, h)| (n.as_str(), h.as_slice())));
            let mut load_title = |t: &str| titles.get(t).and_then(|n| std::fs::read(dir.join(n)).ok());
            let pal = gboh_render::battlefield::palette(&mut load).unwrap();
            let types: Vec<usize> = (0..gboh_formats::units::ART_TITLES.len()).collect();
            let map = p.game.map.clone();
            let scene = Scene::build(&scn.map_file, &map, gboh_render::terrain::Zoom::In, &pal, 1, &types, &mut load, &mut load_title).unwrap();
            let c = scene.compose_with(&p.views(), &[], &p.corpses);
            let rgba = c.to_rgba(&pal);
            let mut ppm = format!("P6 {} {} 255\n", c.width, c.height).into_bytes();
            for px in rgba.chunks(4) {
                ppm.extend_from_slice(&px[..3]);
            }
            std::fs::write(out, ppm).unwrap();
        }
        assert!(moved > 0);
    }

    /// Alexander's group can group move on Hydaspes turn 1.
    #[test]
    fn hydaspes_group_move() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let scn = Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let mut p = Play::new(Game::new(&scn, map, 1), Vec::new());
        println!("refused: {:?}", p.game.group_move_refused());
        if p.game.group_move_refused().is_some() {
            return;
        }
        assert!(p.group_move());
        println!("{} | options {}", p.status, p.lit().len());
        if let Some(&to) = p.lit().first() {
            p.take_log();
            let before = p.game.battle.units.iter().filter(|u| u.mp == 0).count();
            assert!(p.click(to, false));
            assert!(p.game.battle.units.iter().filter(|u| u.mp == 0).count() > before);
        }
    }

    /// How long composing a frame takes (walking animation recomposes every
    /// frame); runs only with `GBOH_DATA`.
    #[test]
    fn compose_time() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let scn = Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let files: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
        let find = |n: &str| files.iter().find(|p| p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(n))).cloned();
        let mut load = |n: &str| find(n).and_then(|p| std::fs::read(p).ok());
        let heads: Vec<(String, Vec<u8>)> = files
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("eif")))
            .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(p).unwrap()[..128].to_vec()))
            .collect();
        let titles = gboh_render::battlefield::eif_titles(heads.iter().map(|(n, h)| (n.as_str(), h.as_slice())));
        let mut load_title = |t: &str| titles.get(t).and_then(|n| std::fs::read(dir.join(n)).ok());
        let pal = gboh_render::battlefield::palette(&mut load).unwrap();
        let p = Play::new(Game::new(&scn, map.clone(), 1), Vec::new());
        let types: Vec<usize> = (0..gboh_formats::units::ART_TITLES.len()).collect();
        for z in [gboh_render::terrain::Zoom::In, gboh_render::terrain::Zoom::Normal, gboh_render::terrain::Zoom::Out] {
            let scene = Scene::build(&scn.map_file, &map, z, &pal, 1, &types, &mut load, &mut load_title).unwrap();
            let t = std::time::Instant::now();
            for _ in 0..5 {
                let _ = scene.compose(&p.views(), &p.flagged());
            }
            println!("{z:?}: {:.1} ms per frame", t.elapsed().as_secs_f64() * 1000.0 / 5.0);
        }
    }

    /// Plays demo battles (the computer on both sides) and renders every
    /// walk step, as the viewer would; runs only with `GBOH_DATA`.
    #[test]
    fn demo_renders() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let files: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
        let find = |n: &str| files.iter().find(|p| p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(n))).cloned();
        let read = |n: &str| find(n).and_then(|p| std::fs::read(p).ok());
        let heads: Vec<(String, Vec<u8>)> = files
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("eif")))
            .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(p).unwrap()[..128].to_vec()))
            .collect();
        let titles = gboh_render::battlefield::eif_titles(heads.iter().map(|(n, h)| (n.as_str(), h.as_slice())));
        let mut load = |n: &str| read(n);
        let pal = gboh_render::battlefield::palette(&mut load).unwrap();
        let types: Vec<usize> = (0..gboh_formats::units::ART_TITLES.len()).collect();
        for scn_file in ["HYDASPES.SCN", "GAUGAMEL.SCN", "PELIUM.SCN"] {
            let scn = Scenario::parse(&read(scn_file).unwrap()).unwrap();
            let map = Map::parse(&read(&scn.map_file).unwrap()).unwrap();
            let mut text = |n: &str| read(n).map(|d| String::from_utf8_lossy(&d).into_owned());
            let mut ai = Vec::new();
            for (nation, plans) in [(scn.visitor.nation, &scn.visitor_ai), (scn.home.nation, &scn.home_ai)] {
                let script = gboh_game::ai::Script::load(&plans[0], &mut text).unwrap();
                ai.push((nation, gboh_game::ai::Machine::new(script)));
            }
            let mut load = |n: &str| read(n);
            let mut load_title = |t: &str| titles.get(t).and_then(|n| std::fs::read(dir.join(n)).ok());
            let zoom = if std::env::var_os("GBOH_SHOTS").is_some() { gboh_render::terrain::Zoom::Normal } else { gboh_render::terrain::Zoom::Out };
            let scene = Scene::build(&scn.map_file, &map, zoom, &pal, 1, &types, &mut load, &mut load_title).unwrap();
            let mut p = Play::new(Game::new(&scn, map, 3), ai);
            let (mut steps, mut frames, mut volleys, mut shot, mut dropped, mut fought, mut runs, mut walked) = (0, 0, 0, false, false, false, 0, false);
            while steps < 150 && p.step_computer() {
                steps += 1;
                p.take_log();
                let anims = std::mem::take(&mut p.anims);
                for (i, a) in anims.iter().enumerate() {
                    match a {
                        Anim::Walk { unit, path, running } => {
                            let foot = gboh_render::units::Formation::of_type(p.game.battle.units[*unit].type_id) == gboh_render::units::Formation::Infantry;
                            let shots = std::env::var("GBOH_SHOTS").ok().filter(|_| foot && !walked && !*running);
                            walked |= shots.is_some();
                            for w in path.windows(2) {
                                let times: &[f32] = if shots.is_some() { &[0.0, 0.2, 0.4, 0.6, 0.8, 1.0] } else { &[0.5] };
                                for (k, &t) in times.iter().enumerate() {
                                    let v = p.views_walking(&scene, Some((*unit, w[0], w[1], t, k, *running)), &anims[i + 1..]);
                                    let canvas = scene.compose_with(&v, &p.flagged(), &p.corpses);
                                    if let Some(dir) = &shots {
                                        let (cx, cy) = gboh_render::objects::anchor_at(&scene.map, &scene.layout, (w[0].x as usize, w[0].y as usize), (0, 0));
                                        let (x0, y0) = ((cx - 110).max(0) as usize, (cy - 90).max(0) as usize);
                                        let (x1, y1) = (((cx + 110) as usize).min(canvas.width), ((cy + 60) as usize).min(canvas.height));
                                        let rgba = canvas.to_rgba(&pal);
                                        let mut out = format!("P6 {} {} 255\n", x1 - x0, y1 - y0).into_bytes();
                                        for y in y0..y1 {
                                            for x in x0..x1 {
                                                out.extend_from_slice(&rgba[(y * canvas.width + x) * 4..][..3]);
                                            }
                                        }
                                        std::fs::write(format!("{dir}/{}_walk{k}.ppm", scn_file.split('.').next().unwrap()), out).unwrap();
                                    }
                                    frames += 1;
                                }
                                if shots.is_some() {
                                    break;
                                }
                            }
                        }
                        Anim::Fall(_) => p.land(a),
                        Anim::Fight { .. } => {
                            let fight_shots = std::env::var("GBOH_SHOTS").ok().filter(|_| !fought && matches!(a, Anim::Fight { falls, .. } if falls.iter().any(|f| !f.runaways.is_empty())));
                            fought |= fight_shots.is_some();
                            let times: Vec<u32> = if fight_shots.is_some() { (0..16).map(|k| k * 800).collect() } else { vec![0, 3000, 7200] };
                            for ms in times {
                                let mut v = p.views_walking(&scene, None, &anims[i + 1..]);
                                let down = p.fight(&mut v, a, ms);
                                let mut corpses = p.corpses.clone();
                                corpses.extend(down);
                                // runaways at the viewer's pace (fight time is real time, 0.12 s a hex)
                                let mut runners = Vec::new();
                                if let Anim::Fight { falls, .. } = a {
                                    for f in falls {
                                        for (k, kind, path) in &f.runaways {
                                            if let Some(t) = ms.checked_sub(f.falls_at(*k)) {
                                                let secs = t as f32 / 1000.0;
                                                runners.push((*kind, path.as_slice(), secs / 0.12, (secs / 0.1) as usize));
                                                runs += 1;
                                            }
                                        }
                                    }
                                }
                                let canvas = scene.compose_all(&v, &p.flagged(), &corpses, &[], &runners);
                                if let (Some(dir), Anim::Fight { units, .. }) = (&fight_shots, a) {
                                    let h = p.game.battle.units[units[0].0].hex;
                                    let (cx, cy) = gboh_render::objects::anchor_at(&scene.map, &scene.layout, (h.x as usize, h.y as usize), (0, 0));
                                    let (x0, y0) = ((cx - 200).max(0) as usize, (cy - 150).max(0) as usize);
                                    let (x1, y1) = (((cx + 200) as usize).min(canvas.width), ((cy + 110) as usize).min(canvas.height));
                                    let rgba = canvas.to_rgba(&pal);
                                    let mut out = format!("P6 {} {} 255\n", x1 - x0, y1 - y0).into_bytes();
                                    for y in y0..y1 {
                                        for x in x0..x1 {
                                            out.extend_from_slice(&rgba[(y * canvas.width + x) * 4..][..3]);
                                        }
                                    }
                                    std::fs::write(format!("{dir}/{}_fight{:02}.ppm", scn_file.split('.').next().unwrap(), ms / 800), out).unwrap();
                                }
                                frames += 1;
                            }
                            p.land(a);
                        }
                        Anim::Drop { unit, .. } => {
                            let o = &p.game.battle.units[*unit];
                            let n = scene.unit_sprites(o.type_id).map_or(0, |a| a.drop_frames(scene.screen_facing(o.facing)));
                            let shots = std::env::var("GBOH_SHOTS").ok().filter(|_| !dropped && n > 0);
                            dropped |= shots.is_some();
                            for f in 0..n.max(1) {
                                let mut v = p.views_walking(&scene, None, &anims[i + 1..]);
                                p.dropping(&mut v, *unit, f);
                                let canvas = scene.compose_with(&v, &p.flagged(), &p.corpses);
                                if let (Some(dir), Some(uv)) = (&shots, v.iter().find(|x| x.id == p.game.battle.units[*unit].source)) {
                                    let (cx, cy) = gboh_render::objects::anchor_at(&scene.map, &scene.layout, uv.hex, (0, 0));
                                    let (x0, y0) = ((cx - 70).max(0) as usize, (cy - 70).max(0) as usize);
                                    let (x1, y1) = (((cx + 70) as usize).min(canvas.width), ((cy + 30) as usize).min(canvas.height));
                                    let rgba = canvas.to_rgba(&pal);
                                    let mut out = format!("P6 {} {} 255\n", x1 - x0, y1 - y0).into_bytes();
                                    for y in y0..y1 {
                                        for x in x0..x1 {
                                            out.extend_from_slice(&rgba[(y * canvas.width + x) * 4..][..3]);
                                        }
                                    }
                                    std::fs::write(format!("{dir}/{}_drop{f:02}.ppm", scn_file.split('.').next().unwrap()), out).unwrap();
                                }
                                frames += 1;
                            }
                            p.land(a);
                        }
                        Anim::Volley { shooter, target, missile, .. } => {
                            // GBOH_SHOTS: write the first volley's frames there (PPM)
                            let far = p.game.battle.units[*shooter].hex.distance(p.game.battle.units[*target].hex) >= 2;
                            let shots = std::env::var("GBOH_SHOTS").ok().filter(|_| far && !shot);
                            shot |= shots.is_some();
                            let list: Vec<usize> = if shots.is_some() { (0..24).collect() } else { vec![0, FIRE_RELEASE, FIRE_RELEASE + 4, 40] };
                            for f in list {
                                let mut v = p.views_walking(&scene, None, &anims[i + 1..]);
                                let flights = p.volley(&scene, &mut v, *shooter, *target, *missile, f);
                                let canvas = scene.compose_all(&v, &p.flagged(), &p.corpses, &flights, &[]);
                                if let Some(dir) = &shots {
                                    let at = |u: usize| {
                                        let h = p.game.battle.units[u].hex;
                                        gboh_render::objects::anchor_at(&scene.map, &scene.layout, (h.x as usize, h.y as usize), (0, 0))
                                    };
                                    let (a, b) = (at(*shooter), at(*target));
                                    let x0 = (a.0.min(b.0) - 80).max(0) as usize;
                                    let y0 = (a.1.min(b.1) - 110).max(0) as usize;
                                    let x1 = ((a.0.max(b.0) + 80) as usize).min(canvas.width);
                                    let y1 = ((a.1.max(b.1) + 40) as usize).min(canvas.height);
                                    let rgba = canvas.to_rgba(&pal);
                                    let mut out = format!("P6 {} {} 255\n", x1 - x0, y1 - y0).into_bytes();
                                    for y in y0..y1 {
                                        for x in x0..x1 {
                                            out.extend_from_slice(&rgba[(y * canvas.width + x) * 4..][..3]);
                                        }
                                    }
                                    let name = scn_file.split('.').next().unwrap();
                                    std::fs::write(format!("{dir}/{name}_{f:02}.ppm"), out).unwrap();
                                }
                                frames += 1;
                                volleys += 1;
                            }
                            p.land(a);
                        }
                    }
                }
                let _ = scene.compose_with(&p.views(), &p.flagged(), &p.corpses);
            }
            println!("{scn_file}: {steps} leaders, {frames} frames ({volleys} volley, {runs} runaway), {} fallen, over {}", p.corpses.len(), p.over);
            if let Ok(dir) = std::env::var("GBOH_SHOTS") {
                let canvas = scene.compose_with(&p.views(), &p.flagged(), &p.corpses);
                let rgba = canvas.to_rgba(&pal);
                let mut out = format!("P6 {} {} 255\n", canvas.width, canvas.height).into_bytes();
                for px in rgba.chunks(4) {
                    out.extend_from_slice(&px[..3]);
                }
                std::fs::write(format!("{dir}/{}_end.ppm", scn_file.split('.').next().unwrap()), out).unwrap();
                let at: Vec<_> = p.corpses.iter().map(|c| gboh_render::objects::anchor_at(&scene.map, &scene.layout, c.hex, c.offset)).collect();
                std::fs::write(format!("{dir}/{}_corpses.txt", scn_file.split('.').next().unwrap()), format!("{at:?}")).unwrap();
            }
        }
    }
}
