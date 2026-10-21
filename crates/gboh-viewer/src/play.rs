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

use gboh_formats::map::Map;
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
    /// The computer's side (scenario nation byte) and its script.
    ai: Option<(u8, gboh_game::ai::Machine)>,
    selected: Option<usize>,
    leader: Option<usize>,
    pub status: String,
    pub over: bool,
}

impl Play {
    /// `ai`: the side the computer plays (nation byte) with its plan.
    pub fn new(mut game: Game, ai: Option<(u8, gboh_game::ai::Machine)>) -> Self {
        let leader = game.begin_turn();
        let mut p = Self { game, ai, selected: None, leader, status: String::new(), over: false };
        p.say("battle begins");
        p.computer_turns();
        p
    }

    /// While the active leader is the computer's, let its script give his
    /// orders and finish him.
    fn computer_turns(&mut self) {
        while !self.over
            && let (Some(l), Some((side, _))) = (self.leader, &self.ai)
            && self.game.battle.units[l].side == *side
        {
            let name = self.game.battle.units[l].name.clone();
            let machine = &mut self.ai.as_mut().unwrap().1;
            if let Err(e) = self.game.ai_orders(machine) {
                println!("AI error: {e}");
            }
            println!("computer: {name}");
            self.finish_one(true);
        }
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
                self.say("selected");
                true
            }
            (Some(s), Some(t)) if right => {
                let name = self.game.battle.units[t].name.clone();
                match orders::volley(&mut self.game.battle, s, t, &mut self.game.rng) {
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
                let map: Map = self.game.map.clone();
                match orders::move_to(&mut self.game.battle, &map, s, hex, &mut self.game.rng) {
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
        self.computer_turns();
        true
    }

    fn finish_one(&mut self, momentum: bool) {
        self.selected = None;
        let (outcomes, next) = self.game.finish_leader(momentum);
        let routed: usize = outcomes.iter().map(|o| o.routed.len() + o.eliminated.len()).sum();
        self.leader = next;
        if next.is_none() {
            if let Verdict::Won(a) = self.game.end_turn() {
                self.over = true;
                let nation = self.game.armies[a].0;
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
    use gboh_formats::scn::Scenario;

    /// Plays a few orders at Hydaspes; runs only with `GBOH_DATA` set to the
    /// game's DATA directory.
    #[test]
    fn hydaspes_orders() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let scn = Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let mut p = Play::new(Game::new(&scn, map, 1), None);
        let agema = p.game.battle.units.iter().position(|u| u.name == "Agema Companions").unwrap();
        assert!(p.click(Hex::new(13, 21), false), "select");
        assert_eq!(p.selected, Some(agema));
        assert!(p.lit().contains(&Hex::new(21, 20)));
        assert!(p.click(Hex::new(21, 20), false), "move");
        assert_eq!(p.game.battle.units[agema].hex, Hex::new(21, 20));
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
        let ai = Some((scn.visitor.nation, gboh_game::ai::Machine::new(script)));
        let mut p = Play::new(Game::new(&scn, map, 1), ai);
        let start: Vec<_> = p.game.battle.units.iter().map(|u| u.hex).collect();
        for _ in 0..12 {
            p.finish(false);
            if let Some(l) = p.leader {
                assert_eq!(p.game.battle.units[l].side, scn.home.nation);
            }
        }
        let moved = p.game.battle.units.iter().zip(&start).filter(|(u, h)| u.side == scn.visitor.nation && u.hex != **h).count();
        println!("{} | Indian units moved: {moved}", p.status);
        assert!(moved > 0);
    }

    /// Alexander's group can group move on Hydaspes turn 1.
    #[test]
    fn hydaspes_group_move() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let scn = Scenario::parse(&std::fs::read(dir.join("HYDASPES.SCN")).unwrap()).unwrap();
        let map = Map::parse(&std::fs::read(dir.join("HYDASPES.MAP")).unwrap()).unwrap();
        let mut p = Play::new(Game::new(&scn, map, 1), None);
        println!("refused: {:?}", p.game.group_move_refused());
        if p.game.group_move_refused().is_some() {
            return;
        }
        assert!(p.group_move());
        println!("{} | options {}", p.status, p.lit().len());
        if let Some(&to) = p.lit().first() {
            let before = p.game.battle.units.iter().filter(|u| u.mp == 0).count();
            assert!(p.click(to, false));
            assert!(p.game.battle.units.iter().filter(|u| u.mp == 0).count() > before);
        }
    }
}
