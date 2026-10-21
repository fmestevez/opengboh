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
use gboh_render::units::UnitView;

pub struct Play {
    pub game: Game,
    /// Sounds to play for what just happened.
    pub events: Vec<crate::sound::Event>,
    /// Moves to animate: the unit, the hexes it went through from where it
    /// started, and whether it ran (routed).
    pub walks: Vec<(usize, Vec<Hex>, bool)>,
    /// Fallen men lying on the battlefield.
    pub corpses: Vec<gboh_render::units::Corpse>,
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
}

impl Play {
    /// `ai`: the sides the computer plays (nation byte) with their plans.
    pub fn new(mut game: Game, ai: Vec<(u8, gboh_game::ai::Machine)>) -> Self {
        let leader = game.begin_turn();
        let demo = ai.len() > 1;
        let mut p = Self { game, ai, demo, events: vec![crate::sound::Event::Start], walks: Vec::new(), corpses: Vec::new(), seed: 12345, selected: None, leader, status: String::new(), over: false };
        p.say("battle begins");
        if !p.demo {
            p.computer_turns();
        }
        p
    }

    /// While the active leader is the computer's, let its script give his
    /// orders and finish him.
    fn computer_turns(&mut self) {
        while self.step_computer() {}
    }

    /// If the active leader is the computer's, play him (orders, then
    /// finish with momentum); returns whether one was played.
    pub fn step_computer(&mut self) -> bool {
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
                        self.walks.push((unit, path, running));
                    }
                }
                gboh_game::game::Happened::Casualties { unit, hex, kind, count } => {
                    let kind = match kind {
                        gboh_game::game::Casualty::Shock => 0,
                        gboh_game::game::Casualty::Arrow => 1,
                        gboh_game::game::Casualty::Trample => 2,
                    };
                    // bodies in proportion to the figures: elephants and
                    // chariots have few, so drop one at most; cavalry two
                    use gboh_game::movement::MoveClass::*;
                    let most = match gboh_game::movement::MoveClass::of_type(self.game.battle.units[unit].type_id) {
                        Some(Elephant | Chariot | Oxybeles) => 1,
                        Some(HeavyCavalry | LightCavalry | Lancer) => 2,
                        _ => 4,
                    };
                    for _ in 0..count.min(most) {
                        let mut r = || {
                            self.seed = self.seed.wrapping_mul(22695477).wrapping_add(1);
                            ((self.seed >> 16) & 0x7fff) as i32
                        };
                        let offset = (r() % 90 - 45, r() % 36 - 18);
                        let cel = r() as usize;
                        self.corpses.push(gboh_render::units::Corpse {
                            hex: (hex.x as usize, hex.y as usize),
                            type_id: self.game.battle.units[unit].type_id,
                            kind,
                            cel,
                            offset,
                        });
                    }
                }
                gboh_game::game::Happened::Volley { weapon, hit, .. } => {
                    self.events.push(crate::sound::Event::Volley(weapon, hit));
                }
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
        pending: &[(usize, Vec<Hex>, bool)],
    ) -> Vec<UnitView> {
        let mut views = self.views();
        // units shown on the move even if already out of the battle
        let shown = pending.iter().map(|(u, _, _)| *u).chain(walking.map(|w| w.0));
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
                });
            }
        }
        // latest first, so a unit with several pending walks ends up at the
        // start of its earliest
        for (u, path, _) in pending.iter().rev() {
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
            }
        }
        views
    }

    /// Units to draw, by scenario index.
    pub fn views(&self) -> Vec<UnitView> {
        self.game
            .battle
            .units
            .iter()
            .filter(|u| u.on_field())
            .map(|u| UnitView {
                hex: (u.hex.x as usize, u.hex.y as usize),
                facing: u.facing,
                type_id: u.type_id,
                nation: u.nation,
                id: u.source,
                routed: u.state == UnitState::Routed,
                walk: None,
            })
            .collect()
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
        true
    }

    /// A click on `hex`; returns whether the battlefield changed.
    pub fn click(&mut self, hex: Hex, right: bool) -> bool {
        if self.over {
            return false;
        }
        let target = self.unit_at(hex);
        let orderable = self.game.orderable();
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
                self.game.do_group_move(hex);
                self.say("group moved");
                return true;
            }
            if self.selected == Some(l) && self.lit().contains(&hex) {
                if !self.charge(l) {
                    self.say("no command points");
                    return false;
                }
                self.game.move_leader(hex);
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
                let name = self.game.battle.units[t].name.clone();
                match self.game.order_volley(s, t) {
                    Ok(h) if self.charge(s) => self.say(&format!("volley at {name}: {h} hits")),
                    Ok(_) => self.say("no command points"),
                    Err(_) => self.say("cannot volley at that"),
                }
                true
            }
            (Some(s), Some(t)) if self.game.battle.units[t].side != self.game.battle.units[s].side => {
                let ok = self.game.designate_shock(s, t);
                if ok {
                    self.charge(s);
                }
                self.say(if ok { "shock designated" } else { "cannot shock that" });
                true
            }
            (Some(s), None) if self.lit().contains(&hex) => {
                if !self.charge(s) {
                    self.say("no command points");
                    return false;
                }
                match self.game.order_move(s, hex) {
                    Ok(fire) => {
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
        if let Some(why) = self.game.group_move_refused() {
            self.say(&format!("no group move: {why}"));
            return false;
        }
        let result = self.game.try_group_move();
        self.selected = self.leader;
        self.say(&format!("group move: {result:?}"));
        true
    }

    /// Rally the selected routed unit (one order).
    pub fn rally(&mut self) -> bool {
        let Some(s) = self.selected else { return false };
        if self.game.battle.units[s].state != UnitState::Routed {
            self.say("only routed units rally");
            return false;
        }
        if !self.charge(s) {
            self.say("no command points");
            return false;
        }
        let ok = self.game.rally(s);
        self.say(if ok { "rallied" } else { "rally failed: terminal rout" });
        self.selected = None;
        true
    }

    /// Recover cohesion for the selected unit (one order).
    pub fn recover(&mut self) -> bool {
        let Some(s) = self.selected else { return false };
        if self.game.battle.units[s].state != UnitState::Fighting || self.game.battle.units[s].hits == 0 {
            self.say("nothing to recover");
            return false;
        }
        if !self.charge(s) {
            self.say("no command points");
            return false;
        }
        let n = self.game.recover(s);
        self.say(&format!("{n} cohesion hits removed"));
        self.selected = None;
        true
    }

    /// Turn the selected unit.
    pub fn turn(&mut self, left: bool) -> bool {
        let Some(s) = self.selected else { return false };
        if !self.charge(s) {
            self.say("no command points");
            return false;
        }
        let map = self.game.map.clone();
        match orders::turn(&mut self.game.battle, &map, s, left) {
            Ok(()) => self.say("turned"),
            Err(e) => self.say(&format!("{e:?}")),
        }
        true
    }

    /// Finish the active leader (and the turn after the last one), trying
    /// for momentum if asked, then let the computer play its leaders.
    pub fn finish(&mut self, momentum: bool) -> bool {
        if self.over {
            return false;
        }
        self.finish_one(momentum);
        if !self.demo {
            self.computer_turns();
        }
        true
    }

    fn finish_one(&mut self, momentum: bool) {
        self.selected = None;
        let shocks: Vec<_> = self.game.shocks.clone();
        let (outcomes, next) = self.game.finish_leader(momentum);
        if !outcomes.is_empty() {
            let elephants = shocks.iter().any(|(a, d)| {
                a.iter().map(|&(u, _)| u).chain(d.iter().copied()).any(|u| {
                    gboh_game::movement::MoveClass::of_type(self.game.battle.units[u].type_id)
                        == Some(gboh_game::movement::MoveClass::Elephant)
                })
            });
            self.events.push(crate::sound::Event::Shock { elephants });
        }
        let routed: usize = outcomes.iter().map(|o| o.routed.len() + o.eliminated.len()).sum();
        self.leader = next;
        if next.is_none() {
            if let Verdict::Won(a) = self.game.end_turn() {
                self.over = true;
                let nation = self.game.armies[a].0;
                let won = self.ai.len() != 1 || self.ai[0].0 != nation;
                self.events.push(crate::sound::Event::End { won });
                self.say(&format!("army of nation {nation} wins"));
                return;
            }
            self.leader = self.game.begin_turn();
            self.say(&format!("{} shocks, {routed} routed; new turn", outcomes.len()));
        } else {
            self.say(&format!("{} shocks, {routed} routed", outcomes.len()));
        }
    }
}

/// The face centre of map hex `(x, y)` in `scene`'s canvas pixels.
fn centre(scene: &Scene, x: usize, y: usize) -> (i32, i32) {
    let (w, _, face) = scene.layout.zoom.block();
    let (bx, by) = scene.layout.block_origin(x, scene.layout.cols - 1 - y);
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
        let (u, path, _) = p.walks.last().unwrap();
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
        println!("{} | Indian units moved: {moved} | walks queued {}", p.status, p.walks.len());
        assert!(!p.walks.is_empty(), "the computer's moves are queued for animation");
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
            let scene = Scene::build(&scn.map_file, &map, gboh_render::terrain::Zoom::Out, &pal, 1, &types, &mut load, &mut load_title).unwrap();
            let mut p = Play::new(Game::new(&scn, map, 3), ai);
            let (mut steps, mut frames) = (0, 0);
            while steps < 150 && p.step_computer() {
                steps += 1;
                p.take_log();
                let walks = std::mem::take(&mut p.walks);
                for (i, (u, path, running)) in walks.iter().enumerate() {
                    for w in path.windows(2) {
                        let v = p.views_walking(&scene, Some((*u, w[0], w[1], 0.5, 1, *running)), &walks[i + 1..]);
                        let _ = scene.compose_with(&v, &p.flagged(), &p.corpses);
                        frames += 1;
                    }
                }
                let _ = scene.compose_with(&p.views(), &p.flagged(), &p.corpses);
            }
            println!("{scn_file}: {steps} leaders, {frames} walk frames, {} fallen, over {}", p.corpses.len(), p.over);
        }
    }
}
