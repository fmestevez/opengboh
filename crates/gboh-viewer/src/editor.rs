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
use gboh_game::editor::{Editor, KINDS, MAX_SIZE, NATIONS, Paint, STREAM, WALL};
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
    /// Show a hex's properties (and set its sides there).
    Inspect,
    /// Clear a hex's properties: its ground, streams and walls.
    ClearHex,
}

const BRUSHES: [(Brush, &str); 17] = [
    (Brush::Hex(Paint::Water), "Water"),
    (Brush::Hex(Paint::Level(1)), "Level 1"),
    (Brush::Hex(Paint::Level(2)), "Level 2"),
    (Brush::Hex(Paint::Level(3)), "Level 3"),
    (Brush::Hex(Paint::Level(4)), "Level 4"),
    (Brush::Hex(Paint::Level(5)), "Level 5"),
    (Brush::Hex(Paint::Level(6)), "Level 6"),
    (Brush::Hex(Paint::Level(7)), "Level 7"),
    (Brush::Hex(Paint::Level(8)), "Level 8"),
    (Brush::Hex(Paint::Clear), "Clear"),
    (Brush::Hex(Paint::Woods), "Woods"),
    (Brush::Hex(Paint::Rocky), "Rocky"),
    (Brush::Hex(Paint::Road), "Road"),
    (Brush::Side(STREAM), "Stream"),
    (Brush::Side(WALL), "Wall"),
    (Brush::Inspect, "Properties"),
    (Brush::ClearHex, "Clear hex"),
];

/// A text field being typed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Title,
    Author,
    Description,
    FileName,
    /// The catalogue's search.
    Search,
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
    HexSide(usize, u16),
    Resize(i32, i32),
    ApplyResize,
    Package,
    NewBlank,
    NewOn(usize),
    Open(usize),
    Save,
    Play,
    Quit,
    Undo,
    Redo,
    /// A unit of the group shown: selected, and the map centred on it.
    Member(usize),
}

/// The most changes there are to undo.
const UNDO_DEPTH: usize = 100;

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
    /// The map's size the battlefield was drawn for.
    scene_size: (usize, usize),
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
    /// Shown only the catalogue's units whose name or nation has this.
    search: String,
    /// The window's size at the last click.
    window: (usize, usize),
    list_top: usize,
    template: Option<usize>,
    selected: Option<usize>,
    group: Option<i16>,
    /// An order whose hex the next map click sets.
    setting_hex: Option<usize>,
    /// The hex last painted in a drag.
    painted: Option<(usize, usize)>,
    /// The scenario as it was before each change (the last the newest),
    /// the changes undone, and the scenario as last committed to them.
    undo: Vec<Editor>,
    redo: Vec<Editor>,
    last: Editor,
    /// A side brush's drag: whether it puts the feature on (else takes it
    /// off, as the click that began it did), and the side last done.
    side_drag: Option<(bool, (usize, usize), MapSide)>,
    /// A redraw owed (with the terrain if `true`), done once the window's
    /// events are handled ([`EditorScreen::flush`]), so a drag paints as
    /// fast as the mouse moves.
    stale: Option<bool>,
    /// Whether the battlefield scrolled since the last flush.
    moved: bool,
    /// The hex whose properties are shown.
    inspected: Option<(usize, usize)>,
    /// The size the map would be resized to.
    resize_to: (i32, i32),
    /// A package whose unpacking would replace files, until clicked again.
    replacing: Option<String>,
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
            scene_size: (0, 0),
            palette,
            font: Font::small(data_dir).in_ink(colors[1]),
            colors,
            scroll: (0, 0),
            tab: Tab::File,
            brush: 1,
            home: true,
            catalogue,
            kind: None,
            search: String::new(),
            window: (0, 0),
            list_top: 0,
            template: None,
            selected: None,
            group: None,
            setting_hex: None,
            painted: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last: Editor::new(Map::blank("New Map", 1, 1), ""),
            side_drag: None,
            stale: None,
            moved: false,
            inspected: None,
            resize_to: (31, 31),
            replacing: None,
            focus: None,
            file_name: "MYBATTLE".into(),
            new_size: (31, 31),
            maps: Vec::new(),
            scenarios: Vec::new(),
            message: "Start a scenario on a map, or open one (File).".into(),
        };
        s.last = s.ed.clone();
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
        let package = |n: &String| find(&dir, n).and_then(|p| std::fs::read(p).ok()).is_some_and(|d| gboh_formats::psf::unpack(&d).is_ok());
        self.scenarios.extend(names(".psf").into_iter().filter(package));
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
                Ok(s) => {
                    self.scene = Some(s);
                    self.scene_size = (self.ed.map.width, self.ed.map.height);
                }
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
        let search = self.search.trim().to_lowercase();
        let found = |u: &Unit| {
            search.is_empty() || u.name.to_lowercase().contains(&search) || NATIONS.get(u.nation as usize).is_some_and(|n| n.to_lowercase().contains(&search))
        };
        let mut v: Vec<usize> =
            (0..self.catalogue.len()).filter(|&i| self.kind.is_none_or(|k| self.catalogue[i].kind == k) && found(&self.catalogue[i])).collect();
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
                y += (BRUSHES.len() as u32).div_ceil(3) as i32 * (ROW + 4) + 10;
                line(&mut c, &format!("Map: {} x {} hexes", self.ed.map.width, self.ed.map.height), &mut y);
                let mut bx = x0;
                for (act, label) in [(Act::Resize(-2, 0), "w-"), (Act::Resize(2, 0), "w+"), (Act::Resize(0, -1), "h-"), (Act::Resize(0, 1), "h+")] {
                    let r = (bx, y, 30, ROW);
                    self.button(&mut c, r, label, false);
                    acts.push((act, r));
                    bx += 32;
                }
                self.font.draw(&mut c, &format!("{} x {}", self.resize_to.0, self.resize_to.1), bx + 4, y + 3);
                let r = (x0 + inner - 70, y, 70, ROW);
                self.button(&mut c, r, "Resize", false);
                acts.push((Act::ApplyResize, r));
                y += ROW + 10;
                match self.inspected {
                    Some((hx, hy)) => {
                        let level = self.ed.map.elevation_at(hx, hy);
                        let ground = match self.ed.ground(hx, hy) {
                            Paint::Woods => "woods",
                            Paint::Rocky => "rough ground",
                            Paint::Road => "a road",
                            _ => "clear",
                        };
                        let level = if level == 0 { "water".to_string() } else { format!("level {level}") };
                        line(&mut c, &format!("Hex {hx:02}{hy:02}: {level}, {ground}"), &mut y);
                        for (code, label) in [(STREAM, "Stream"), (WALL, "Wall")] {
                            self.font.draw(&mut c, label, x0, y + 3);
                            let mut bx = x0 + 56;
                            for (k, (side, name)) in MapSide::ALL.into_iter().zip(["N", "NE", "SE", "S", "SW", "NW"]).enumerate() {
                                let r = (bx, y, 36, ROW);
                                self.button(&mut c, r, name, self.ed.has_side(hx, hy, side, code));
                                acts.push((Act::HexSide(k, code), r));
                                bx += 38;
                            }
                            y += ROW + 4;
                        }
                    }
                    None => line(&mut c, "Properties: click a hex to see and set its sides.", &mut y),
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
                // the catalogue, searched
                self.font.draw(&mut c, "Search", x0, y + 4);
                let r = (x0 + 50, y, inner - 50, ROW + 2);
                fill(&mut c, r, if self.focus == Some(Field::Search) { white } else { paper });
                frame(&mut c, r, ink);
                self.font.draw(&mut c, &self.search, r.0 + 3, y + 4);
                acts.push((Act::Focus(Field::Search), r));
                y += ROW + 8;
                let shown = self.shown();
                if shown.is_empty() {
                    self.font.draw(&mut c, "(no unit matches)", x0 + 3, y + 3);
                }
                let rows = ((h as i32 - y - 210) / ROW).max(3) as usize;
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
                // the unit selected, else the one to place: a man of it, and
                // what it is
                let shown = self.selected.and_then(|i| self.ed.scn.units.get(i)).or(self.template.and_then(|t| self.catalogue.get(t)));
                if let Some(u) = shown {
                    let r = (x0, y, 60, 56);
                    fill(&mut c, r, white);
                    frame(&mut c, r, ink);
                    self.figure(&mut c, u, r);
                    let kind = KINDS.get(u.kind as usize).copied().unwrap_or("?");
                    let nation = NATIONS.get(u.nation as usize).copied().unwrap_or("?");
                    let stats = match &u.extra {
                        gboh_formats::scn::UnitExtra::Leader([i, range, charisma, elite]) => {
                            format!("Initiative {i}, range {range}, charisma {charisma}{}", if *elite > 0 { ", elite" } else { "" })
                        }
                        gboh_formats::scn::UnitExtra::Combat { a, b, .. } => format!("TQ {b}, size {a}"),
                        gboh_formats::scn::UnitExtra::Missile { a, b, m1, .. } => format!("TQ {b}, size {a}, range {m1}"),
                    };
                    for (k, text) in [u.name.as_str(), &format!("{kind}, {nation}"), &stats].into_iter().enumerate() {
                        self.font.draw(&mut c, text, x0 + 68, y + 6 + k as i32 * 16);
                    }
                    y += 62;
                }
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
                    y += ROW + 12;
                    // its units (the original's Group Information)
                    let members: Vec<usize> = (0..self.ed.scn.units.len()).filter(|&i| self.ed.scn.units[i].unk_b as i16 == g.id).collect();
                    line(&mut c, &format!("Its units ({}): click one to find it", members.len()), &mut y);
                    let rows = ((h as i32 - y - 70) / ROW).max(1) as usize;
                    for (k, &i) in members.iter().take(rows).enumerate() {
                        let r = (x0, y + k as i32 * ROW, inner, ROW);
                        if Some(i) == self.selected {
                            fill(&mut c, r, gold);
                        }
                        let u = &self.ed.scn.units[i];
                        let at = self.ed.map.hex_from_id(u.hex).map_or(String::new(), |(x, y)| format!(" at {x:02}{y:02}"));
                        self.font.draw(&mut c, &format!("{}{at}", u.name), x0 + 3, r.1 + 3);
                        acts.push((Act::Member(i), r));
                    }
                    if members.len() > rows {
                        self.font.draw(&mut c, &format!("and {} more", members.len() - rows), x0 + 3, y + rows as i32 * ROW + 3);
                    }
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
                y += ROW + 6;
                let r = (x0, y, inner, ROW + 2);
                self.button(&mut c, r, "Save as a package (.psf)", false);
                acts.push((Act::Package, r));
                y += ROW + 8;
                for (k, (act, label)) in [(Act::Play, "Save and play"), (Act::Quit, "Leave the editor")].into_iter().enumerate() {
                    let r = (x0 + k as i32 * (inner / 2), y, inner / 2 - 4, ROW + 4);
                    self.button(&mut c, r, label, false);
                    acts.push((act, r));
                }
            }
        }
        // undo and redo, over the message line
        let by = h as i32 - 2 * ROW - 14;
        for (k, (act, label, can)) in
            [(Act::Undo, "Undo", !self.undo.is_empty() || self.ed != self.last), (Act::Redo, "Redo", !self.redo.is_empty())].into_iter().enumerate()
        {
            let r = (x0 + k as i32 * 64, by, 60, ROW + 2);
            self.button(&mut c, r, label, false);
            if can {
                acts.push((act, r));
            }
        }
        // the message line
        fill(&mut c, (view_w + 2, h as i32 - ROW - 6, PANEL - 2, ROW + 6), gold);
        self.font.draw(&mut c, &self.message, x0, h as i32 - ROW - 1);
        (c, acts)
    }

    /// One of `unit`'s men, twice his size, in the middle of `r`.
    fn figure(&self, c: &mut Canvas, unit: &Unit, (x, y, w, h): Rect) {
        let Some((sprite, _)) = gboh_formats::units::unit_type(unit)
            .and_then(|t| self.scene.as_ref()?.unit_sprites(t))
            .and_then(|art| art.walk_cel(4, 0).or_else(|| art.walk_cel(0, 0)))
        else {
            return;
        };
        let (sw, sh) = (sprite.width as i32 * 2, sprite.height as i32 * 2);
        let (ox, oy) = (x + (w - sw) / 2, y + (h - sh) / 2);
        for py in 0..sh.min(h) {
            for px in 0..sw.min(w) {
                let p = sprite.pixels[(py / 2) as usize * sprite.width + (px / 2) as usize];
                let (dx, dy) = (ox + px, oy + py);
                if p != 0 && dx >= x && dy >= y && dx >= 0 && dy >= 0 && (dx as usize) < c.width && (dy as usize) < c.height {
                    c.pixels[dy as usize * c.width + dx as usize] = p;
                }
            }
        }
    }

    fn button(&self, c: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), label: &str, on: bool) {
        let [paper, ink, gold, _] = self.colors;
        fill(c, (x, y, w, h), if on { gold } else { paper });
        frame(c, (x, y, w, h), ink);
        let tw = self.font.width(label);
        self.font.draw(c, label, x + (w - tw).max(4) / 2, y + (h - 11) / 2);
    }

    /// The map hex under window pixel `(x, y)`, and the side nearest it.
    fn hex_at(&self, x: i32, y: i32) -> Option<((usize, usize), Option<MapSide>)> {
        let scene = self.scene.as_ref()?;
        let (px, py) = (x + self.scroll.0, y + self.scroll.1);
        let h = play::pick(scene, px, py)?;
        let (hx, hy) = (h.x as usize, h.y as usize);
        let (cx, cy) = play::centre(scene, hx, hy);
        let (dx, dy) = ((px - cx) as f32, (cy - py) as f32);
        let side = (dx != 0.0 || dy != 0.0).then(|| {
            // screen sectors: right N, upper right NW, upper left SW, left S,
            // lower left SE, lower right NE
            let a = dy.atan2(dx).to_degrees().rem_euclid(360.0);
            [MapSide::N, MapSide::NW, MapSide::SW, MapSide::S, MapSide::SE, MapSide::NE][(((a + 30.0) / 60.0) as usize) % 6]
        });
        Some(((hx, hy), side))
    }

    /// A click at window pixel `(x, y)`: on the panel, or on the map.
    pub fn click(&mut self, x: i32, y: i32, w: usize, h: usize) -> Outcome {
        // (what was typed before is a change of its own)
        self.commit();
        self.window = (w, h);
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
                Brush::Hex(p) => {
                    // (a drag from here paints on)
                    self.painted = Some((hx, hy));
                    self.ed.paint(hx, hy, p).map(|_| true)
                }
                Brush::Side(code) => match side {
                    Some(s) => {
                        self.ed.toggle_side(hx, hy, s, code);
                        // (a drag from here goes on putting on, or taking off)
                        self.side_drag = Some((self.ed.has_side(hx, hy, s, code), (hx, hy), s));
                        Ok(true)
                    }
                    None => Err("click nearer the hex side".into()),
                },
                Brush::Inspect => {
                    self.inspected = Some((hx, hy));
                    Ok(false)
                }
                Brush::ClearHex => {
                    self.painted = Some((hx, hy));
                    self.ed.clear_hex(hx, hy);
                    Ok(true)
                }
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
        let mut redraw_terrain = false;
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
                // (the first dimension odd)
                let max = MAX_SIZE as i32;
                self.new_size = ((self.new_size.0 + dw).clamp(11, max) | 1, (self.new_size.1 + dh).clamp(10, max));
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
            Act::HexSide(k, code) => {
                if let Some((x, y)) = self.inspected {
                    self.ed.toggle_side(x, y, MapSide::ALL[k], code);
                    redraw_terrain = true;
                }
            }
            Act::Resize(dw, dh) => {
                let max = MAX_SIZE as i32;
                self.resize_to = ((self.resize_to.0 + dw).clamp(1, max) | 1, (self.resize_to.1 + dh).clamp(1, max));
            }
            Act::ApplyResize => match self.ed.resize(self.resize_to.0 as usize, self.resize_to.1 as usize) {
                Ok(()) => {
                    // (a new size: the battlefield drawn afresh)
                    self.scene = None;
                    self.scroll = (0, 0);
                    self.inspected = None;
                    self.rebuild(true);
                    self.message = "Resized.".into();
                }
                Err(e) => self.message = e,
            },
            Act::Package => match self.package() {
                Ok(p) => self.message = format!("Saved {}.", p.display()),
                Err(e) => self.message = e,
            },
            Act::Save => match self.save() {
                Ok(p) => self.message = format!("Saved {}.", p.display()),
                Err(e) => self.message = e,
            },
            Act::Play => match self.save() {
                Ok(p) => return Outcome::Play(p),
                Err(e) => self.message = e,
            },
            Act::Quit => return Outcome::Quit,
            Act::Member(i) => {
                self.selected = Some(i);
                self.centre_on(i);
                redraw_units = true;
            }
            Act::Undo => self.undo(),
            Act::Redo => self.redo(),
        }
        if redraw_units || redraw_terrain {
            self.rebuild(redraw_terrain);
        }
        Outcome::Redraw
    }

    /// Edit a fresh scenario (`ed`): redraw its map from scratch.
    fn start(&mut self, ed: Editor) {
        // (a scenario of its own: nothing of the last to undo)
        self.undo.clear();
        self.redo.clear();
        self.last = ed.clone();
        self.ed = ed;
        self.scene = None;
        self.selected = None;
        self.group = None;
        self.scroll = (0, 0);
        self.inspected = None;
        self.resize_to = (self.ed.map.width as i32, self.ed.map.height as i32);
        self.rebuild(true);
        self.tab = Tab::Scenario;
    }

    fn open(&mut self, name: &str) -> Result<(), String> {
        if name.to_ascii_lowercase().ends_with(".psf") {
            let scn = self.unpack(name)?;
            return self.open(&scn);
        }
        let read = |n: &str| find(&self.data_dir, n).and_then(|p| std::fs::read(p).ok()).ok_or(format!("{n} not found"));
        let scn = Scenario::parse(&read(name)?).map_err(|e| e.to_string())?;
        let map = Map::parse(&read(&scn.map_file)?).map_err(|e| e.to_string())?;
        self.file_name = name.rsplit_once('.').map_or(name, |(b, _)| b).to_string();
        self.start(Editor::open(scn, map));
        Ok(())
    }

    /// Unpack package `name` into the scenarios folder; returns its
    /// scenario's name. A file it would replace with different contents
    /// is replaced only on a second try.
    fn unpack(&mut self, name: &str) -> Result<String, String> {
        let data = find(&self.data_dir, name).and_then(|p| std::fs::read(p).ok()).ok_or(format!("{name} not found"))?;
        let files = gboh_formats::psf::unpack(&data).map_err(|e| e.to_string())?;
        let scn = files.iter().find(|(n, _)| n.to_ascii_lowercase().ends_with(".scn")).ok_or("the package has no scenario")?.0.clone();
        let dir = scenarios_dir();
        let replaced: Vec<&str> = files.iter().filter(|(n, d)| std::fs::read(dir.join(n)).is_ok_and(|old| old != *d)).map(|(n, _)| n.as_str()).collect();
        if !replaced.is_empty() && self.replacing.as_deref() != Some(name) {
            self.replacing = Some(name.into());
            return Err(format!("It would replace {}: click again to.", replaced.join(", ")));
        }
        self.replacing = None;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for (n, d) in &files {
            std::fs::write(dir.join(n), d).map_err(|e| e.to_string())?;
        }
        Ok(scn)
    }

    /// Save, and pack the files saved into `<name>.psf` beside them (in
    /// the original's order: map, scenario, home plan, visitor plan).
    fn package(&mut self) -> Result<PathBuf, String> {
        let mut files = self.write()?;
        files.swap(0, 1);
        let base = files[1].0.rsplit_once('.').map_or(files[1].0.clone(), |(b, _)| b.to_string());
        let path = scenarios_dir().join(format!("{base}.psf"));
        std::fs::write(&path, gboh_formats::psf::pack(&files)).map_err(|e| e.to_string())?;
        self.list_files();
        Ok(path)
    }

    /// Save to the scenarios folder: the scenario, its map, its plans.
    fn save(&mut self) -> Result<PathBuf, String> {
        let files = self.write()?;
        Ok(scenarios_dir().join(&files[0].0))
    }

    /// Write the scenario, its map and its plans to the scenarios folder;
    /// returns them (name, bytes).
    fn write(&mut self) -> Result<Vec<(String, Vec<u8>)>, String> {
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
        let files = self.ed.files(&base, guid(), guid());
        for (name, data) in &files {
            std::fs::write(dir.join(name), data).map_err(|e| e.to_string())?;
        }
        self.list_files();
        Ok(files)
    }

    /// A drag over the battlefield from `from` to `to` (window pixels):
    /// paints with a hex brush on the Map tab, else pans the battlefield.
    pub fn drag(&mut self, from: (i32, i32), to: (i32, i32), w: usize, h: usize) {
        if Self::on_panel(from.0, w) {
            return;
        }
        let brush = BRUSHES[self.brush].0;
        if self.tab != Tab::Map || !matches!(brush, Brush::Hex(_) | Brush::ClearHex | Brush::Side(_)) {
            return self.scroll_by(from.0 - to.0, from.1 - to.1, w, h);
        }
        // every hex on the way (the mouse reports a fast drag in leaps)
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let steps = (dx.abs().max(dy.abs()) / 6).max(1);
        for k in 1..=steps {
            let (x, y) = (from.0 + dx * k / steps, from.1 + dy * k / steps);
            let Some((hex, side)) = self.hex_at(x, y).filter(|_| !Self::on_panel(x, w)) else { continue };
            // a side brush: the side nearest the pointer, on or off
            if let (Brush::Side(code), Some(s), Some((on, last_hex, last_side))) = (brush, side, self.side_drag) {
                if (hex, s) != (last_hex, last_side) {
                    self.side_drag = Some((on, hex, s));
                    if self.ed.has_side(hex.0, hex.1, s, code) != on {
                        self.ed.toggle_side(hex.0, hex.1, s, code);
                        self.stale = Some(true);
                    }
                }
                continue;
            }
            if matches!(brush, Brush::Side(_)) {
                continue;
            }
            if self.painted == Some(hex) {
                continue;
            }
            self.painted = Some(hex);
            let done = match brush {
                Brush::Hex(paint) => self.ed.paint(hex.0, hex.1, paint),
                _ => {
                    self.ed.clear_hex(hex.0, hex.1);
                    Ok(())
                }
            };
            match done {
                Ok(()) => self.stale = Some(true),
                Err(e) => self.message = e,
            }
        }
    }

    /// Do the redraw owed, if any: returns whether the screen changed.
    pub fn flush(&mut self) -> bool {
        let moved = std::mem::take(&mut self.moved);
        match self.stale.take() {
            Some(terrain) => {
                self.rebuild(terrain);
                true
            }
            None => moved,
        }
    }

    /// The end of a drag (or a click): what it changed is one change to
    /// undo.
    pub fn drag_end(&mut self) {
        self.painted = None;
        self.side_drag = None;
        self.commit();
    }

    /// If the scenario changed since the last commit, keep how it was, to
    /// undo.
    fn commit(&mut self) {
        if self.ed == self.last {
            return;
        }
        self.undo.push(std::mem::replace(&mut self.last, self.ed.clone()));
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Undo the last change.
    pub fn undo(&mut self) {
        self.commit();
        let Some(prev) = self.undo.pop() else { return };
        self.redo.push(std::mem::replace(&mut self.ed, prev));
        self.restored();
        self.message = "Undone.".into();
    }

    /// Redo the last change undone.
    pub fn redo(&mut self) {
        let Some(next) = self.redo.pop() else { return };
        self.undo.push(std::mem::replace(&mut self.ed, next));
        self.restored();
        self.message = "Redone.".into();
    }

    /// After an undo or redo: the screen drawn afresh for the scenario.
    fn restored(&mut self) {
        self.last = self.ed.clone();
        if self.scene_size != (self.ed.map.width, self.ed.map.height) {
            self.scene = None;
            self.scroll = (0, 0);
        }
        self.selected = None;
        self.setting_hex = None;
        if self.group.is_some_and(|g| self.ed.leader_of(g).is_none()) {
            self.group = None;
        }
        if self.inspected.is_some_and(|(x, y)| x >= self.ed.map.width || y >= self.ed.map.height) {
            self.inspected = None;
        }
        self.stale = Some(true);
    }

    /// Typing into the field in focus: returns whether it took the key.
    pub fn key(&mut self, text: Option<&str>, backspace: bool, enter: bool) -> bool {
        let Some(f) = self.focus else { return false };
        if enter {
            self.focus = None;
            self.commit();
            return true;
        }
        let info = self.ed.scn.editor.get_or_insert_with(Default::default);
        let s = match f {
            Field::Title => &mut self.ed.scn.title,
            Field::Author => &mut info.author,
            Field::Description => &mut info.description,
            Field::FileName => &mut self.file_name,
            Field::Search => {
                self.list_top = 0;
                &mut self.search
            }
        };
        if backspace {
            s.pop();
        } else if let Some(t) = text {
            s.extend(t.chars().filter(|c| !c.is_control()));
        }
        true
    }

    /// Scroll the battlefield to put unit `i` in the middle of the view.
    fn centre_on(&mut self, i: usize) {
        let (Some(scene), Some((x, y))) = (&self.scene, self.ed.scn.units.get(i).and_then(|u| self.ed.map.hex_from_id(u.hex))) else { return };
        let (cx, cy) = play::centre(scene, x, y);
        let (w, h) = self.window;
        let (view_w, view_h) = ((w as i32 - PANEL).max(0), h as i32);
        self.scroll = (0, 0);
        self.scroll_by(cx - view_w / 2, cy - view_h / 2, w, h);
    }

    /// Scroll the battlefield by `(dx, dy)` pixels, kept on it.
    pub fn scroll_by(&mut self, dx: i32, dy: i32, w: usize, h: usize) {
        let max_x = (self.composed.width as i32 - (w as i32 - PANEL)).max(0);
        let max_y = (self.composed.height as i32 - h as i32).max(0);
        self.scroll = ((self.scroll.0 + dx).clamp(0, max_x), (self.scroll.1 + dy).clamp(0, max_y));
        self.moved = true;
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
    /// map, places a leader and a unit, saves, reads it back and plays it;
    /// runs only with `GBOH_CAESAR`.
    #[test]
    fn edits_and_saves_a_scenario() {
        let Ok(dir) = std::env::var("GBOH_CAESAR") else { return };
        let saves = std::env::temp_dir().join(format!("gboh-editor-{}", std::process::id()));
        // (the scenarios folder beside a test saves folder)
        let _var = crate::SAVES_VAR.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("GBOH_SAVES", saves.join("saves")) };
        let mut e = EditorScreen::new(Path::new(&dir));
        assert!(e.catalogue.len() > 2000);
        let k = e.maps.iter().position(|m| m.eq_ignore_ascii_case("PHARSAL.MAP")).unwrap();
        e.act(Act::NewOn(k));
        // a brush: painted as the button goes down, then along a drag in
        // one leap, redrawn once at the flush
        e.tab = Tab::Map;
        e.brush = BRUSHES.iter().position(|b| b.0 == Brush::Hex(Paint::Woods)).unwrap();
        let at = |e: &EditorScreen, x: usize, y: usize| {
            let (cx, cy) = play::centre(e.scene.as_ref().unwrap(), x, y);
            (cx - e.scroll.0, cy - e.scroll.1)
        };
        // (a window as big as the battlefield, all of it in view)
        let (ww, wh) = (e.composed.width + PANEL as usize, e.composed.height);
        let ((x0, y0), (x1, y1)) = (at(&e, 4, 6), at(&e, 4, 12));
        e.click(x0, y0, ww, wh);
        e.drag((x0, y0), (x1, y1), ww, wh);
        assert!(e.stale.is_some());
        assert!(e.flush() && e.stale.is_none());
        e.drag_end();
        assert!((6..=12).all(|y| e.ed.ground(4, y) == Paint::Woods), "{}", e.message);
        // a stream drawn along a drag, then taken off along it (on a blank
        // map)
        e.act(Act::NewBlank);
        e.tab = Tab::Map;
        e.brush = BRUSHES.iter().position(|b| b.0 == Brush::Side(STREAM)).unwrap();
        let streams = |e: &EditorScreen, x: usize| (5..=13).flat_map(|y| MapSide::ALL.map(|s| (y, s))).filter(|&(y, s)| e.ed.has_side(x, y, s, STREAM)).count();
        let sx = 14;
        assert_eq!(streams(&e, sx), 0);
        let ((sx0, sy0), (sx1, sy1)) = (at(&e, sx, 6), at(&e, sx, 12));
        let (sx0, sx1) = (sx0 + 20, sx1 + 20);
        e.click(sx0, sy0, ww, wh);
        e.drag((sx0, sy0), (sx1, sy1), ww, wh);
        e.drag_end();
        e.flush();
        assert!(streams(&e, sx) >= 5, "{} sides", streams(&e, sx));
        e.click(sx0, sy0, ww, wh);
        e.drag((sx0, sy0), (sx1, sy1), ww, wh);
        e.drag_end();
        assert_eq!(streams(&e, sx), 0);
        // undone a change at a time (a drag is one), and redone
        let blank = e.ed.clone();
        e.brush = BRUSHES.iter().position(|b| b.0 == Brush::Hex(Paint::Rocky)).unwrap();
        let (rx0, ry0) = at(&e, 3, 3);
        let (rx1, ry1) = at(&e, 3, 9);
        e.click(rx0, ry0, ww, wh);
        e.drag((rx0, ry0), (rx1, ry1), ww, wh);
        e.drag_end();
        let rocky = e.ed.clone();
        assert!(rocky != blank);
        e.undo();
        assert!(e.ed == blank, "the drag undone at once");
        e.redo();
        assert!(e.ed == rocky);
        e.flush();
        // a level slopes the ground around it
        e.brush = BRUSHES.iter().position(|b| b.0 == Brush::Hex(Paint::Level(3))).unwrap();
        let (x, y) = at(&e, 8, 8);
        e.click(x, y, ww, wh);
        assert_eq!(e.ed.map.elevation_at(8, 8), 3, "{}", e.message);
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
        // and the visitor's, across the field
        e.home = false;
        e.group = None;
        e.template = Some(leader);
        e.unit_click(10, 14).unwrap();
        e.template = Some(foot);
        e.selected = None;
        e.unit_click(11, 14).unwrap();
        assert_eq!(e.ed.scn.units.len(), 4);
        e.file_name = "TESTED".into();
        let path = e.save().unwrap();
        let scn = Scenario::parse(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(scn.units.len(), 4);
        // it plays as Save and play starts it: from the scenarios folder,
        // with its map and plans, the computer on both sides
        let mut b = crate::load_battle(Path::new(&dir), "TESTED.scn", [true, true], false).unwrap();
        let play = b.play.as_mut().unwrap();
        let mut steps = 0;
        while steps < 60 && play.step_computer() {
            steps += 1;
        }
        assert!(steps > 1, "{steps}");
        // the catalogue searched, by name or nation
        e.focus = Some(Field::Search);
        for c in ["h", "y", "p", "a", "s"] {
            e.key(Some(c), false, false);
        }
        let found = e.shown();
        assert!(!found.is_empty() && found.iter().all(|&i| e.catalogue[i].name.to_lowercase().contains("hypas")));
        e.key(None, false, true);
        e.search = "pompeian".into();
        assert!(e.shown().iter().all(|&i| e.catalogue[i].nation == 10));
        // a group's member found on the map
        let member = e.ed.scn.units.iter().position(|u| u.kind != 0).unwrap();
        e.group = Some(e.ed.scn.units[member].unk_b as i16);
        e.window = (900, 600);
        e.act(Act::Member(member));
        assert_eq!(e.selected, Some(member));
        // packed, and opened from the package (nothing to replace)
        e.act(Act::Package);
        assert!(e.scenarios.iter().any(|n| n == "TESTED.psf"), "{}", e.message);
        e.open("TESTED.psf").unwrap();
        assert_eq!(e.ed.scn.units.len(), 4);
        // resized, keeping its units
        let w = e.ed.map.width;
        e.act(Act::Resize(-2, 0));
        e.act(Act::ApplyResize);
        assert_eq!(e.ed.map.width, w - 2, "{}", e.message);
        e.tab = Tab::Map;
        e.brush = BRUSHES.iter().position(|b| b.0 == Brush::Inspect).unwrap();
        e.inspected = Some((10, 10));
        let (canvas, acts) = e.canvas(1200, 800);
        assert_eq!((canvas.width, canvas.height), (1200, 800));
        assert!(!acts.is_empty());
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            crate::ui::write_ppm(&canvas, &e.palette, &Path::new(&out).join("editor.ppm")).unwrap();
            e.tab = Tab::Units;
            e.selected = None;
            e.template = e.catalogue.iter().position(|u| u.name.contains("Hypaspist")).or(Some(foot));
            let (units, _) = e.canvas(1200, 800);
            crate::ui::write_ppm(&units, &e.palette, &Path::new(&out).join("editor-units.ppm")).unwrap();
            e.tab = Tab::Groups;
            e.group = e.ed.scn.units.iter().find(|u| u.kind != 0).map(|u| u.unk_b as i16);
            e.selected = e.ed.scn.units.iter().position(|u| u.kind != 0);
            let (groups, _) = e.canvas(1200, 800);
            crate::ui::write_ppm(&groups, &e.palette, &Path::new(&out).join("editor-groups.ppm")).unwrap();
        }
        unsafe { std::env::remove_var("GBOH_SAVES") };
        let _ = std::fs::remove_dir_all(&saves);
    }
}
