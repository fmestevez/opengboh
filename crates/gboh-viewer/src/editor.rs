//! The scenario editor's screen (after the *Collector's Edition*'s
//! `scenario editor.exe`): the battlefield on the left, drawn as in battle,
//! and a panel on the right with five tabs. **Map** paints levels, water,
//! woods, rough ground and roads, and puts streams and walls on hex sides;
//! **Units** places units of every nation of the three games from the
//! editor's catalogue (leaders form the groups the others join), and
//! selects, turns, moves and removes them; **Groups** gives each group its
//! timed orders and behaviour, from which the AI plans are written;
//! **Scenario** sets the title, the sides, their retreat edges and rout
//! levels and the turn limit; **File** starts a scenario on a map, opens
//! one, saves it and plays it. It works on the Caesar data (the art of all
//! three games is there), and saves to opengboh's own scenarios folder.

use crate::play;
use crate::ui::Font;
use gboh_formats::map::{Map, Side as MapSide};
use gboh_formats::pal::Palette;
use gboh_formats::scn::{Scenario, Unit};
use gboh_game::editor::{Editor, KINDS, NATIONS, Paint, STREAM, WALL};
use gboh_render::battlefield::Scene;
use gboh_render::terrain::Zoom;
use gboh_render::units::UnitView;
use gboh_render::{Canvas, Sprite};
use std::path::{Path, PathBuf};

/// The panel's width.
const PANEL: i32 = 320;
/// A row's height in the panel's lists and forms.
const ROW: i32 = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Map,
    Units,
    Groups,
    Scenario,
    File,
}

const TABS: [(Tab, &str); 5] = [(Tab::Map, "Map"), (Tab::Units, "Units"), (Tab::Groups, "Groups"), (Tab::Scenario, "Scenario"), (Tab::File, "File")];

/// The map brushes: hex ground, or a side feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Brush {
    Hex(Paint),
    Side(u16),
}

const BRUSHES: [(Brush, &str); 15] = [
    (Brush::Hex(Paint::Water), "Water"),
    (Brush::Hex(Paint::Level(1)), "Level 1"),
    (Brush::Hex(Paint::Level(2)), "Level 2"),
    (Brush::Hex(Paint::Level(3)), "Level 3"),
    (Brush::Hex(Paint::Level(4)), "Level 4"),
    (Brush::Hex(Paint::Level(5)), "Level 5"),
    (Brush::Hex(Paint::Level(6)), "Level 6"),
    (Brush::Hex(Paint::Level(7)), "Level 7"),
    (Brush::Hex(Paint::Clear), "Clear"),
    (Brush::Hex(Paint::Woods), "Woods"),
    (Brush::Hex(Paint::Rocky), "Rocky"),
    (Brush::Hex(Paint::Road), "Road"),
    (Brush::Side(STREAM), "Stream"),
    (Brush::Side(WALL), "Wall"),
    (Brush::Hex(Paint::Level(8)), "Level 8"),
];

/// A text field being typed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Title,
    Author,
    Description,
    FileName,
}

/// A place on the screen: left, top, width, height.
type Rect = (i32, i32, i32, i32);

/// What a click on the panel does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    Tab(Tab),
    Brush(usize),
    Side(bool),
    Kind(Option<u8>),
    Template(usize),
    ListUp,
    ListDown,
    Turn(bool),
    Remove,
    Group(i16),
    AddOrder(u32),
    OrderTurns(usize, i32),
    OrderHex(usize),
    DropOrder(usize),
    Behaviour,
    Focus(Field),
    Nation(bool, i32),
    Edge(bool, u8),
    Rout(bool, i32),
    Limit,
    LimitTurns(i32),
    LimitSide,
    MapSize(i32, i32),
    NewBlank,
    NewOn(usize),
    Open(usize),
    Save,
    Play,
    Quit,
}

/// What the screen asks the viewer to do.
pub enum Outcome {
    Nothing,
    Redraw,
    /// Play the saved scenario (its file in the scenarios folder).
    Play(PathBuf),
    Quit,
}

pub struct EditorScreen {
    data_dir: PathBuf,
    pub ed: Editor,
    scene: Option<Scene>,
    /// The battlefield with its units, drawn once per change.
    composed: Canvas,
    palette: Palette,
    font: Font,
    /// Palette indices of the panel's colours: parchment, dark ink and
    /// frame, highlight, white.
    colors: [u8; 4],
    pub scroll: (i32, i32),
    tab: Tab,
    brush: usize,
    /// Which side units are placed for.
    home: bool,
    catalogue: Vec<Unit>,
    kind: Option<u8>,
    list_top: usize,
    template: Option<usize>,
    selected: Option<usize>,
    group: Option<i16>,
    /// An order whose hex the next map click sets.
    setting_hex: Option<usize>,
    focus: Option<Field>,
    file_name: String,
    new_size: (i32, i32),
    maps: Vec<String>,
    scenarios: Vec<String>,
    message: String,
}

/// Where the editor saves its scenarios (and reads back its maps and plans
/// from): opengboh's own folder, never the game's.
pub fn scenarios_dir() -> PathBuf {
    crate::library::root().join("scenarios")
}

/// The files of `name` in the scenarios folder, else the data folder.
fn find(data_dir: &Path, name: &str) -> Option<PathBuf> {
    crate::find_ci(&scenarios_dir(), name).or_else(|| crate::find_ci(data_dir, name))
}

impl EditorScreen {
    pub fn new(data_dir: &Path) -> Self {
        let palette = crate::palette_path(data_dir).and_then(|p| std::fs::read(p).ok()).and_then(|d| Palette::parse(&d).ok()).unwrap_or_else(|| Palette {
            name: String::new(),
            colors: [[0; 3]; 256],
            remap: Vec::new(),
        });
        let near = |rgb: [u8; 3]| {
            (1..=255u8).min_by_key(|&i| palette.colors[i as usize].iter().zip(rgb).map(|(&a, b)| (a as i32 - b as i32).pow(2)).sum::<i32>()).unwrap_or(1)
        };
        let colors = [near([226, 210, 170]), near([48, 32, 20]), near([196, 160, 80]), near([255, 255, 255])];
        let catalogue = std::fs::read(data_dir.join("Scenario Editor").join("MainUnits.unt"))
            .ok()
            .and_then(|d| gboh_formats::scn::parse_catalogue(&d).ok())
            .unwrap_or_default();
        let mut s = Self {
            data_dir: data_dir.into(),
            ed: Editor::new(Map::blank("New Map", 31, 31), "NEWMAP.MAP"),
            scene: None,
            composed: Canvas::new(1, 1, 0),
            palette,
            font: Font::small(data_dir).in_ink(colors[1]),
            colors,
            scroll: (0, 0),
            tab: Tab::File,
            brush: 1,
            home: true,
            catalogue,
            kind: None,
            list_top: 0,
            template: None,
            selected: None,
            group: None,
            setting_hex: None,
            focus: None,
            file_name: "MYBATTLE".into(),
            new_size: (31, 31),
            maps: Vec::new(),
            scenarios: Vec::new(),
            message: "Start a scenario on a map, or open one (File).".into(),
        };
        s.list_files();
        s.rebuild(true);
        s
    }

    /// The maps and scenarios there are to start from.
    fn list_files(&mut self) {
        let names = |ext: &str| {
            let mut v: Vec<String> = [scenarios_dir(), self.data_dir.clone()]
                .iter()
                .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.to_ascii_lowercase().ends_with(ext))
                .collect();
            v.sort_by_key(|n| n.to_ascii_lowercase());
            v.dedup_by_key(|n| n.to_ascii_lowercase());
            v
        };
        // (only what reads as one: the data's `*.map` art indexes aren't)
        let dir = self.data_dir.clone();
        let reads = |n: &String, map: bool| {
            find(&dir, n).and_then(|p| std::fs::read(p).ok()).is_some_and(|d| if map { Map::parse(&d).is_ok() } else { Scenario::parse(&d).is_ok() })
        };
        self.maps = names(".map").into_iter().filter(|n| reads(n, true)).collect();
        self.scenarios = names(".scn").into_iter().filter(|n| reads(n, false)).collect();
    }

    /// Redraw the battlefield: its terrain too if `terrain`.
    fn rebuild(&mut self, terrain: bool) {
        let dir = self.data_dir.clone();
        let mut load = |name: &str| crate::find_ci(&dir, name).and_then(|p| std::fs::read(p).ok());
        let titles = crate::eif_title_index(&dir);
        let mut load_title = |title: &str| titles.get(title).and_then(|n| std::fs::read(dir.join(n)).ok());
        let map_file = self.ed.scn.map_file.clone();
        if self.scene.is_none() {
            let types: Vec<usize> = (0..gboh_formats::units::TYPE_COUNT).collect();
            match Scene::build_view(&map_file, &self.ed.map, Zoom::Normal, false, &self.palette, 1, &types, &mut load, &mut load_title) {
                Ok(s) => self.scene = Some(s),
                Err(e) => self.message = format!("can't draw the map: {e}"),
            }
        } else if terrain
            && let Some(scene) = &mut self.scene
            && let Err(e) = scene.rebuild_terrain(&map_file, &self.ed.map, &self.palette, &mut load, &mut load_title)
        {
            self.message = format!("can't draw the map: {e}");
        }
        let Some(scene) = &self.scene else { return };
        let views: Vec<UnitView> = self.ed.scn.units.iter().enumerate().filter_map(|(i, u)| UnitView::of_scenario(&self.ed.map, i, u)).collect();
        let flagged: Vec<usize> = self.selected.into_iter().collect();
        self.composed = scene.compose(&views, &flagged);
        if let Some(g) = self.group {
            let hexes: Vec<gboh_game::hex::Hex> = self
                .ed
                .scn
                .units
                .iter()
                .filter(|u| u.unk_b as i16 == g)
                .filter_map(|u| self.ed.map.hex_from_id(u.hex))
                .map(|(x, y)| gboh_game::hex::Hex::new(x as i32, y as i32))
                .collect();
            play::light(&mut self.composed, scene, &hexes, &self.palette);
        }
    }

    /// The catalogue entries shown: by kind, and for the side's nation
    /// first.
    fn shown(&self) -> Vec<usize> {
        let nation = if self.home { self.ed.scn.home.nation } else { self.ed.scn.visitor.nation };
        let mut v: Vec<usize> = (0..self.catalogue.len()).filter(|&i| self.kind.is_none_or(|k| self.catalogue[i].kind == k)).collect();
        v.sort_by_key(|&i| (self.catalogue[i].nation != nation, self.catalogue[i].nation, self.catalogue[i].name.clone()));
        v
    }

    /// The whole screen, `w` × `h`, and the panel's clickable places.
    pub fn canvas(&self, w: usize, h: usize) -> (Canvas, Vec<(Act, Rect)>) {
        let [paper, ink, gold, white] = self.colors;
        let mut c = Canvas::new(w, h, ink);
        let view_w = (w as i32 - PANEL).max(0);
        c.blit(&Sprite { width: self.composed.width, height: self.composed.height, pixels: self.composed.pixels.clone() }, -self.scroll.0, -self.scroll.1);
        fill(&mut c, (view_w, 0, PANEL, h as i32), paper);
        fill(&mut c, (view_w, 0, 2, h as i32), ink);
        let mut acts = Vec::new();
        let x0 = view_w + 10;
        let inner = PANEL - 20;
        // the tabs
        let tw = inner / TABS.len() as i32;
        for (k, (tab, label)) in TABS.iter().enumerate() {
            let r = (x0 + k as i32 * tw, 8, tw - 4, ROW + 4);
            self.button(&mut c, r, label, *tab == self.tab);
            acts.push((Act::Tab(*tab), r));
        }
        let mut y = 40;
        let line = |c: &mut Canvas, text: &str, y: &mut i32| {
            self.font.draw(c, text, x0, *y + 3);
            *y += ROW;
        };
        match self.tab {
            Tab::Map => {
                line(&mut c, "Click hexes to paint; a side brush near a hex's edge.", &mut y);
                for (k, (_, label)) in BRUSHES.iter().enumerate() {
                    let r = (x0 + (k as i32 % 3) * (inner / 3), y + (k as i32 / 3) * (ROW + 4), inner / 3 - 4, ROW + 2);
                    self.button(&mut c, r, label, k == self.brush);
                    acts.push((Act::Brush(k), r));
                }
            }
            Tab::Units => {
                let side = |home: bool| {
                    let s = if home { self.ed.scn.home } else { self.ed.scn.visitor };
                    format!("{}: {}", if home { "Home" } else { "Visitor" }, NATIONS.get(s.nation as usize).copied().unwrap_or("(none)"))
                };
                for (k, home) in [true, false].into_iter().enumerate() {
                    let r = (x0 + k as i32 * (inner / 2), y, inner / 2 - 4, ROW + 2);
                    self.button(&mut c, r, &side(home), home == self.home);
                    acts.push((Act::Side(home), r));
                }
                y += ROW + 8;
                // the kinds
                let short = |n: &'static str| n.replace("Barbarian", "Barb.");
                let kinds: Vec<(Option<u8>, String)> =
                    std::iter::once((None, "All".to_string())).chain(KINDS.iter().enumerate().map(|(k, n)| (Some(k as u8), short(n)))).collect();
                for (k, (kind, label)) in kinds.iter().enumerate() {
                    let r = (x0 + (k as i32 % 3) * (inner / 3), y + (k as i32 / 3) * (ROW + 2), inner / 3 - 4, ROW);
                    self.button(&mut c, r, label, *kind == self.kind);
                    acts.push((Act::Kind(*kind), r));
                }
                y += (kinds.len() as u32).div_ceil(3) as i32 * (ROW + 2) + 6;
                // the catalogue
                let shown = self.shown();
                let rows = ((h as i32 - y - 140) / ROW).max(3) as usize;
                frame(&mut c, (x0 - 2, y - 2, inner + 4, rows as i32 * ROW + 4), ink);
                for (k, &i) in shown.iter().skip(self.list_top).take(rows).enumerate() {
                    let u = &self.catalogue[i];
                    let r = (x0, y + k as i32 * ROW, inner - 22, ROW);
                    if Some(i) == self.template {
                        fill(&mut c, r, gold);
                    }
                    let nation = NATIONS.get(u.nation as usize).copied().unwrap_or("?");
                    self.font.draw(&mut c, &format!("{} ({nation})", u.name), x0 + 3, r.1 + 3);
                    acts.push((Act::Template(i), r));
                }
                let (up, down) = ((x0 + inner - 20, y, 20, ROW), (x0 + inner - 20, y + rows as i32 * ROW - ROW, 20, ROW));
                self.button(&mut c, up, "^", false);
                self.button(&mut c, down, "v", false);
                acts.push((Act::ListUp, up));
                acts.push((Act::ListDown, down));
                y += rows as i32 * ROW + 10;
                match self.selected.and_then(|i| self.ed.scn.units.get(i)) {
                    Some(u) => {
                        line(&mut c, &format!("Selected: {}", u.name), &mut y);
                        for (k, (act, label)) in
                            [(Act::Turn(false), "Turn left"), (Act::Turn(true), "Turn right"), (Act::Remove, "Remove")].into_iter().enumerate()
                        {
                            let r = (x0 + k as i32 * (inner / 3), y, inner / 3 - 4, ROW + 2);
                            self.button(&mut c, r, label, false);
                            acts.push((act, r));
                        }
                        y += ROW + 6;
                        line(&mut c, "Click an empty hex to move it there.", &mut y);
                    }
                    None => line(&mut c, "Click an empty hex to place; a unit to select.", &mut y),
                }
                let g = self.group.and_then(|g| self.ed.leader_of(g)).map_or("(none: place a leader)".to_string(), |l| self.ed.scn.units[l].name.clone());
                line(&mut c, &format!("Group joined: {g}"), &mut y);
            }
            Tab::Groups => {
                let groups: Vec<_> = self.ed.scn.editor.iter().flat_map(|e| e.groups.clone()).collect();
                if groups.is_empty() {
                    line(&mut c, "No groups yet: place leaders (Units).", &mut y);
                }
                for g in &groups {
                    let r = (x0, y, inner, ROW);
                    self.button(&mut c, r, &g.name, Some(g.id) == self.group);
                    acts.push((Act::Group(g.id), r));
                    y += ROW + 2;
                }
                y += 8;
                if let Some(g) = groups.iter().find(|g| Some(g.id) == self.group) {
                    line(&mut c, "Orders, in turn (turns each; a move to its hex):", &mut y);
                    for (k, &(t, turns, hex)) in g.orders.iter().enumerate().filter(|(_, o)| o.0 != 0) {
                        let what = ["", "Defend", "Move", "Attack"][t.min(3) as usize];
                        let at = self.ed.map.hex_from_id(hex).map_or(String::new(), |(x, y)| format!(" to {:02}{:02}", x, y));
                        self.font.draw(&mut c, &format!("{what} {turns} turns{at}"), x0, y + 3);
                        let mut bx = x0 + inner - 4 * 34;
                        for (act, label) in [(Act::OrderTurns(k, -1), "-"), (Act::OrderTurns(k, 1), "+"), (Act::OrderHex(k), "Hex"), (Act::DropOrder(k), "X")] {
                            if label == "Hex" && t != 2 {
                                bx += 34;
                                continue;
                            }
                            let r = (bx, y, 32, ROW);
                            self.button(&mut c, r, label, self.setting_hex == Some(k) && label == "Hex");
                            acts.push((act, r));
                            bx += 34;
                        }
                        y += ROW + 2;
                    }
                    for (k, (t, label)) in [(1, "+ Defend"), (2, "+ Move"), (3, "+ Attack")].into_iter().enumerate() {
                        let r = (x0 + k as i32 * (inner / 3), y, inner / 3 - 4, ROW + 2);
                        self.button(&mut c, r, label, false);
                        acts.push((Act::AddOrder(t), r));
                    }
                    y += ROW + 8;
                    let r = (x0, y, inner, ROW + 2);
                    self.button(&mut c, r, if g.behaviour == 3 { "Then: attack" } else { "Then: defend" }, false);
                    acts.push((Act::Behaviour, r));
                }
            }
            Tab::Scenario => {
                let info = self.ed.scn.editor.clone().unwrap_or_default();
                for (field, label, value) in [
                    (Field::Title, "Title", self.ed.scn.title.clone()),
                    (Field::Author, "Author", info.author.clone()),
                    (Field::Description, "Description", info.description.clone()),
                ] {
                    line(&mut c, label, &mut y);
                    let r = (x0, y, inner, ROW + 2);
                    fill(&mut c, r, if self.focus == Some(field) { white } else { paper });
                    frame(&mut c, r, ink);
                    let shown: String = value.chars().rev().take(48).collect::<Vec<_>>().into_iter().rev().collect();
                    self.font.draw(&mut c, &shown, x0 + 3, y + 4);
                    acts.push((Act::Focus(field), r));
                    y += ROW + 8;
                }
                for home in [true, false] {
                    let s = if home { self.ed.scn.home } else { self.ed.scn.visitor };
                    let name = NATIONS.get(s.nation as usize).copied().unwrap_or("(choose)");
                    line(&mut c, &format!("{}: {name}, rout level {}", if home { "Home" } else { "Visitor" }, s.rout_level), &mut y);
                    let mut bx = x0;
                    for (act, label) in [(Act::Nation(home, -1), "<"), (Act::Nation(home, 1), ">"), (Act::Rout(home, -5), "-5"), (Act::Rout(home, 5), "+5")] {
                        let r = (bx, y, 34, ROW);
                        self.button(&mut c, r, label, false);
                        acts.push((act, r));
                        bx += 36;
                    }
                    bx += 6;
                    for (edge, label) in [(1, "N"), (3, "E"), (5, "S"), (7, "W")] {
                        let r = (bx, y, 22, ROW);
                        self.button(&mut c, r, label, s.retreat == edge);
                        acts.push((Act::Edge(home, edge), r));
                        bx += 24;
                    }
                    y += ROW + 8;
                }
                line(&mut c, "Retreat edge: where its routed units flee.", &mut y);
                y += 4;
                let limit = info.turn_limit;
                let r = (x0, y, 120, ROW + 2);
                self.button(&mut c, r, if limit.is_some() { "Turn limit: on" } else { "Turn limit: off" }, limit.is_some());
                acts.push((Act::Limit, r));
                if let Some((t, side)) = limit {
                    let mut bx = x0 + 126;
                    for (act, label) in [(Act::LimitTurns(-1), "-"), (Act::LimitTurns(1), "+")] {
                        let r = (bx, y, 24, ROW + 2);
                        self.button(&mut c, r, label, false);
                        acts.push((act, r));
                        bx += 26;
                    }
                    self.font.draw(&mut c, &format!("{t} turns"), bx + 4, y + 4);
                    y += ROW + 6;
                    let r = (x0, y, inner, ROW + 2);
                    self.button(&mut c, r, &format!("The {} must win by then", if side == 0 { "home side" } else { "visitor" }), false);
                    acts.push((Act::LimitSide, r));
                }
            }
            Tab::File => {
                line(&mut c, "New map:", &mut y);
                let mut bx = x0;
                for (act, label) in [(Act::MapSize(-2, 0), "w-"), (Act::MapSize(2, 0), "w+"), (Act::MapSize(0, -1), "h-"), (Act::MapSize(0, 1), "h+")] {
                    let r = (bx, y, 30, ROW);
                    self.button(&mut c, r, label, false);
                    acts.push((act, r));
                    bx += 32;
                }
                self.font.draw(&mut c, &format!("{} x {}", self.new_size.0, self.new_size.1), bx + 4, y + 3);
                let r = (x0 + inner - 70, y, 70, ROW);
                self.button(&mut c, r, "New", false);
                acts.push((Act::NewBlank, r));
                y += ROW + 8;
                let rows = ((h as i32 - y - 160) / ROW / 2).max(3) as usize;
                line(&mut c, "Start on a map:", &mut y);
                for (k, m) in self.maps.iter().enumerate().take(rows) {
                    let r = (x0, y, inner, ROW);
                    self.button(&mut c, r, m, false);
                    acts.push((Act::NewOn(k), r));
                    y += ROW;
                }
                y += 6;
                line(&mut c, "Open (editor or game scenarios):", &mut y);
                for (k, m) in self.scenarios.iter().enumerate().take(rows) {
                    let r = (x0, y, inner, ROW);
                    self.button(&mut c, r, m, false);
                    acts.push((Act::Open(k), r));
                    y += ROW;
                }
                y += 8;
                line(&mut c, "Save as:", &mut y);
                let r = (x0, y, inner - 60, ROW + 2);
                fill(&mut c, r, if self.focus == Some(Field::FileName) { white } else { paper });
                frame(&mut c, r, ink);
                self.font.draw(&mut c, &self.file_name, x0 + 3, y + 4);
                acts.push((Act::Focus(Field::FileName), r));
                let r = (x0 + inner - 56, y, 56, ROW + 2);
                self.button(&mut c, r, "Save", false);
                acts.push((Act::Save, r));
                y += ROW + 8;
                for (k, (act, label)) in [(Act::Play, "Save and play"), (Act::Quit, "Leave the editor")].into_iter().enumerate() {
                    let r = (x0 + k as i32 * (inner / 2), y, inner / 2 - 4, ROW + 4);
                    self.button(&mut c, r, label, false);
                    acts.push((act, r));
                }
            }
        }
        // the message line
        fill(&mut c, (view_w + 2, h as i32 - ROW - 6, PANEL - 2, ROW + 6), gold);
        self.font.draw(&mut c, &self.message, x0, h as i32 - ROW - 1);
        (c, acts)
    }

    fn button(&self, c: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), label: &str, on: bool) {
        let [paper, ink, gold, _] = self.colors;
        fill(c, (x, y, w, h), if on { gold } else { paper });
        frame(c, (x, y, w, h), ink);
        let tw = self.font.width(label);
        self.font.draw(c, label, x + (w - tw).max(4) / 2, y + (h - 11) / 2);
    }

    /// The map hex under window pixel `(x, y)`, and the side nearest it
    /// (when near the hex's edge).
    fn hex_at(&self, x: i32, y: i32) -> Option<((usize, usize), Option<MapSide>)> {
        let scene = self.scene.as_ref()?;
        let (px, py) = (x + self.scroll.0, y + self.scroll.1);
        let h = play::pick(scene, px, py)?;
        let (hx, hy) = (h.x as usize, h.y as usize);
        let (cx, cy) = play::centre(scene, hx, hy);
        let (dx, dy) = ((px - cx) as f32, (cy - py) as f32);
        let (w, _, _) = scene.layout.zoom.block();
        let side = ((dx * dx + dy * dy).sqrt() > w as f32 * 0.28).then(|| {
            // screen sectors: right N, upper right NW, upper left SW, left S,
            // lower left SE, lower right NE
            let a = dy.atan2(dx).to_degrees().rem_euclid(360.0);
            [MapSide::N, MapSide::NW, MapSide::SW, MapSide::S, MapSide::SE, MapSide::NE][(((a + 30.0) / 60.0) as usize) % 6]
        });
        Some(((hx, hy), side))
    }

    /// A click at window pixel `(x, y)`: on the panel, or on the map.
    pub fn click(&mut self, x: i32, y: i32, w: usize, h: usize) -> Outcome {
        let view_w = w as i32 - PANEL;
        if x >= view_w {
            let (_, acts) = self.canvas(w, h);
            let Some(&(act, _)) = acts.iter().rev().find(|(_, (ax, ay, aw, ah))| x >= *ax && x < ax + aw && y >= *ay && y < ay + ah) else {
                self.focus = None;
                return Outcome::Redraw;
            };
            return self.act(act);
        }
        self.focus = None;
        let Some(((hx, hy), side)) = self.hex_at(x, y) else { return Outcome::Nothing };
        let result = match self.tab {
            Tab::Map => match BRUSHES[self.brush].0 {
                Brush::Hex(p) => self.ed.paint(hx, hy, p).map(|_| true),
                Brush::Side(code) => match side {
                    Some(s) => {
                        self.ed.toggle_side(hx, hy, s, code);
                        Ok(true)
                    }
                    None => Err("click nearer the hex side".into()),
                },
            },
            Tab::Units => self.unit_click(hx, hy).map(|_| false),
            Tab::Groups => match self.setting_hex.take() {
                Some(k) => {
                    let id = self.ed.hex_id(hx, hy);
                    self.with_group(|g| g.orders[k].2 = id);
                    Ok(false)
                }
                None => {
                    // a click on a unit picks its group
                    if let Some(i) = self.ed.unit_at(hx, hy) {
                        self.group = Some(self.ed.scn.units[i].unk_b as i16);
                    }
                    Ok(false)
                }
            },
            _ => Ok(false),
        };
        match result {
            Ok(terrain) => {
                self.message.clear();
                self.rebuild(terrain);
            }
            Err(e) => self.message = e,
        }
        Outcome::Redraw
    }

    fn unit_click(&mut self, x: usize, y: usize) -> Result<(), String> {
        if let Some(i) = self.ed.unit_at(x, y) {
            self.selected = Some(i);
            self.group = Some(self.ed.scn.units[i].unk_b as i16);
            return Ok(());
        }
        if let Some(i) = self.selected.take() {
            return self.ed.move_unit(i, x, y);
        }
        let t = self.template.ok_or("choose a unit from the list")?;
        let unit = self.catalogue[t].clone();
        let i = self.ed.place(&unit, x, y, self.home, self.group)?;
        if unit.kind == 0 {
            self.group = Some(self.ed.scn.units[i].unk_b as i16);
        }
        Ok(())
    }

    fn with_group(&mut self, f: impl FnOnce(&mut gboh_formats::scn::Group)) {
        let Some(id) = self.group else { return };
        if let Some(g) = self.ed.scn.editor.as_mut().and_then(|e| e.groups.iter_mut().find(|g| g.id == id)) {
            f(g);
        }
    }

    fn act(&mut self, act: Act) -> Outcome {
        let mut redraw_units = false;
        match act {
            Act::Tab(t) => self.tab = t,
            Act::Brush(k) => self.brush = k,
            Act::Side(home) => self.home = home,
            Act::Kind(k) => {
                self.kind = k;
                self.list_top = 0;
            }
            Act::Template(i) => {
                self.template = Some(i);
                self.selected = None;
                redraw_units = true;
            }
            Act::ListUp => self.list_top = self.list_top.saturating_sub(10),
            Act::ListDown => self.list_top = (self.list_top + 10).min(self.shown().len().saturating_sub(1)),
            Act::Turn(right) => {
                if let Some(i) = self.selected
                    && let Err(e) = self.ed.reface(i, right)
                {
                    self.message = e;
                }
                redraw_units = true;
            }
            Act::Remove => {
                if let Some(i) = self.selected.take() {
                    match self.ed.remove(i) {
                        Ok(()) => self.message = "Removed.".into(),
                        Err(e) => {
                            self.selected = Some(i);
                            self.message = e;
                        }
                    }
                }
                redraw_units = true;
            }
            Act::Group(id) => {
                self.group = Some(id);
                self.setting_hex = None;
                redraw_units = true;
            }
            Act::AddOrder(t) => {
                let mut full = false;
                self.with_group(|g| match g.orders.iter_mut().find(|o| o.0 == 0) {
                    Some(o) => *o = (t, 3, 0),
                    None => full = true,
                });
                if full {
                    self.message = "twenty orders at most".into();
                }
            }
            Act::OrderTurns(k, d) => self.with_group(|g| g.orders[k].1 = (g.orders[k].1 as i32 + d).clamp(1, 50) as u16),
            Act::OrderHex(k) => {
                self.setting_hex = Some(k);
                self.message = "Click the hex the group moves to.".into();
            }
            Act::DropOrder(k) => self.with_group(|g| {
                g.orders.remove(k);
                g.orders.push((0, 0, 0));
            }),
            Act::Behaviour => self.with_group(|g| g.behaviour = if g.behaviour == 3 { 1 } else { 3 }),
            Act::Focus(f) => self.focus = Some(f),
            Act::Nation(home, d) => {
                let s = if home { &mut self.ed.scn.home } else { &mut self.ed.scn.visitor };
                let n = if s.nation as usize >= NATIONS.len() { if d > 0 { -1 } else { NATIONS.len() as i32 } } else { s.nation as i32 };
                s.nation = (n + d).rem_euclid(NATIONS.len() as i32) as u8;
            }
            Act::Edge(home, e) => (if home { &mut self.ed.scn.home } else { &mut self.ed.scn.visitor }).retreat = e,
            Act::Rout(home, d) => {
                let s = if home { &mut self.ed.scn.home } else { &mut self.ed.scn.visitor };
                s.rout_level = (s.rout_level as i32 + d).clamp(1, 5000) as u16;
            }
            Act::Limit => {
                let info = self.ed.scn.editor.get_or_insert_with(Default::default);
                info.turn_limit = if info.turn_limit.is_some() { None } else { Some((10, 0)) };
            }
            Act::LimitTurns(d) => {
                if let Some(l) = self.ed.scn.editor.as_mut().and_then(|e| e.turn_limit.as_mut()) {
                    l.0 = (l.0 as i32 + d).clamp(1, 5000) as u16;
                }
            }
            Act::LimitSide => {
                if let Some(l) = self.ed.scn.editor.as_mut().and_then(|e| e.turn_limit.as_mut()) {
                    l.1 = 1 - l.1.min(1);
                }
            }
            Act::MapSize(dw, dh) => {
                // (the first dimension odd, both at most 50)
                self.new_size = ((self.new_size.0 + dw).clamp(11, 49) | 1, (self.new_size.1 + dh).clamp(10, 50));
            }
            Act::NewBlank => {
                let name = format!("{}.MAP", self.file_name);
                let (w, h) = (self.new_size.0 as usize, self.new_size.1 as usize);
                self.start(Editor::new(Map::blank(&self.file_name, w, h), &name));
                self.message = "A new map: paint it (Map), then place units (Units).".into();
            }
            Act::NewOn(k) => {
                let name = self.maps[k].clone();
                match find(&self.data_dir, &name).and_then(|p| std::fs::read(p).ok()).map(|d| Map::parse(&d)) {
                    Some(Ok(map)) => {
                        self.start(Editor::new(map, &name));
                        self.message = format!("On {name}: set the sides (Scenario), place units (Units).");
                    }
                    _ => self.message = format!("can't read {name}"),
                }
            }
            Act::Open(k) => {
                let name = self.scenarios[k].clone();
                match self.open(&name) {
                    Ok(()) => self.message = format!("Opened {name}."),
                    Err(e) => self.message = e,
                }
            }
            Act::Save => match self.save() {
                Ok(p) => self.message = format!("Saved {}.", p.display()),
                Err(e) => self.message = e,
            },
            Act::Play => match self.save() {
                Ok(p) => return Outcome::Play(p),
                Err(e) => self.message = e,
            },
            Act::Quit => return Outcome::Quit,
        }
        if redraw_units {
            self.rebuild(false);
        }
        Outcome::Redraw
    }

    /// Edit a fresh scenario (`ed`): redraw its map from scratch.
    fn start(&mut self, ed: Editor) {
        self.ed = ed;
        self.scene = None;
        self.selected = None;
        self.group = None;
        self.scroll = (0, 0);
        self.rebuild(true);
        self.tab = Tab::Scenario;
    }

    fn open(&mut self, name: &str) -> Result<(), String> {
        let read = |n: &str| find(&self.data_dir, n).and_then(|p| std::fs::read(p).ok()).ok_or(format!("{n} not found"));
        let scn = Scenario::parse(&read(name)?).map_err(|e| e.to_string())?;
        let map = Map::parse(&read(&scn.map_file)?).map_err(|e| e.to_string())?;
        self.file_name = name.rsplit_once('.').map_or(name, |(b, _)| b).to_string();
        self.start(Editor::open(scn, map));
        Ok(())
    }

    /// Save to the scenarios folder: the scenario, its map, its plans.
    fn save(&mut self) -> Result<PathBuf, String> {
        let problems = self.ed.problems();
        if let Some(p) = problems.first() {
            return Err(p.clone());
        }
        let base: String = self.file_name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(8).collect();
        if base.is_empty() {
            return Err("type a file name".into());
        }
        if self.ed.scn.map_file.to_ascii_uppercase() != format!("{}.MAP", base.to_ascii_uppercase()) {
            // (its own copy of the map, beside it)
            self.ed.scn.map_file = format!("{base}.MAP");
        }
        let dir = scenarios_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for (name, data) in self.ed.files(&base, guid(), guid()) {
            std::fs::write(dir.join(name), data).map_err(|e| e.to_string())?;
        }
        self.list_files();
        Ok(dir.join(format!("{base}.scn")))
    }

    /// Typing into the field in focus: returns whether it took the key.
    pub fn key(&mut self, text: Option<&str>, backspace: bool, enter: bool) -> bool {
        let Some(f) = self.focus else { return false };
        if enter {
            self.focus = None;
            return true;
        }
        let info = self.ed.scn.editor.get_or_insert_with(Default::default);
        let s = match f {
            Field::Title => &mut self.ed.scn.title,
            Field::Author => &mut info.author,
            Field::Description => &mut info.description,
            Field::FileName => &mut self.file_name,
        };
        if backspace {
            s.pop();
        } else if let Some(t) = text {
            s.extend(t.chars().filter(|c| !c.is_control()));
        }
        true
    }

    /// Scroll the battlefield by `(dx, dy)` pixels, kept on it.
    pub fn scroll_by(&mut self, dx: i32, dy: i32, w: usize, h: usize) {
        let max_x = (self.composed.width as i32 - (w as i32 - PANEL)).max(0);
        let max_y = (self.composed.height as i32 - h as i32).max(0);
        self.scroll = ((self.scroll.0 + dx).clamp(0, max_x), (self.scroll.1 + dy).clamp(0, max_y));
    }

    /// Whether `x` is over the panel.
    pub fn on_panel(x: i32, w: usize) -> bool {
        x >= w as i32 - PANEL
    }

    /// Scroll the panel's list (the catalogue) by `rows`.
    pub fn scroll_list(&mut self, rows: i32) {
        self.list_top = (self.list_top as i32 + rows).clamp(0, self.shown().len().saturating_sub(1) as i32) as usize;
    }

    /// The game palette the screen draws in.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }
}

/// A fresh identifier for a saved file (the editor's GUID): time and a
/// counter, enough to tell saves apart.
fn guid() -> [u8; 16] {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut g = [0u8; 16];
    g[..12].copy_from_slice(&t.to_le_bytes()[..12]);
    g[12..].copy_from_slice(&N.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    g
}

fn fill(c: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), color: u8) {
    for yy in y.max(0)..(y + h).min(c.height as i32) {
        for xx in x.max(0)..(x + w).min(c.width as i32) {
            c.pixels[yy as usize * c.width + xx as usize] = color;
        }
    }
}

fn frame(c: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), color: u8) {
    fill(c, (x, y, w, 1), color);
    fill(c, (x, y + h - 1, w, 1), color);
    fill(c, (x, y, 1, h), color);
    fill(c, (x + w - 1, y, 1, h), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The editor opens on Caesar's data with its catalogue, starts on a
    /// map, places a leader and a unit, saves and reads it back; runs only
    /// with `GBOH_CAESAR`.
    #[test]
    fn edits_and_saves_a_scenario() {
        let Ok(dir) = std::env::var("GBOH_CAESAR") else { return };
        let saves = std::env::temp_dir().join(format!("gboh-editor-{}", std::process::id()));
        // (the scenarios folder beside a test saves folder)
        unsafe { std::env::set_var("GBOH_SAVES", saves.join("saves")) };
        let mut e = EditorScreen::new(Path::new(&dir));
        assert!(e.catalogue.len() > 2000);
        let k = e.maps.iter().position(|m| m.eq_ignore_ascii_case("PHARSAL.MAP")).unwrap();
        e.act(Act::NewOn(k));
        e.act(Act::Nation(true, 10));
        e.act(Act::Nation(false, 3));
        e.ed.scn.title = "Test".into();
        let leader = e.catalogue.iter().position(|u| u.kind == 0).unwrap();
        let foot = e.catalogue.iter().position(|u| u.kind == 6).unwrap();
        e.template = Some(leader);
        e.unit_click(10, 10).unwrap();
        e.template = Some(foot);
        e.selected = None;
        e.unit_click(11, 10).unwrap();
        assert_eq!(e.ed.scn.units.len(), 2);
        e.file_name = "TESTED".into();
        let path = e.save().unwrap();
        let scn = Scenario::parse(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(scn.units.len(), 2);
        let (canvas, acts) = e.canvas(1200, 800);
        assert_eq!((canvas.width, canvas.height), (1200, 800));
        assert!(!acts.is_empty());
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            crate::ui::write_ppm(&canvas, &e.palette, &Path::new(&out).join("editor.ppm")).unwrap();
        }
        let _ = std::fs::remove_dir_all(&saves);
    }
}
