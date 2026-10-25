//! Setting up a battle as the original does: the start screen, choosing the
//! scenario, and choosing which side the computer plays.
//!
//! The screens are drawn from the game's own art (`DIALOG.EIF`,
//! `DLGBTN.EIF`, `BACKGND.EIF`, `CAMPAIGN.EIF` and the `MSSANS.EIF` font)
//! laid out by the dialog templates in `alex.exe` (`STARTGAME`, `SCENARIO`,
//! `INTRODLG`), whose dialog units are 7/4 pixels across and 13/8 down (so
//! the start screen fills its 640 × 480 bitmap). Battle names and
//! descriptions come from the executable's string table; the battles' places
//! on the scenario map are the original's (`FUN_0059d56b`).

use gboh_formats::pe::{Dialog, Resources};
use gboh_render::{Canvas, Sprite};
use std::path::Path;

/// The battles in the original's order, with their scenario files.
pub const SCENARIOS: [&str; 11] = [
    "CHAERONE.SCN",
    "LYGINUS.SCN",
    "PELIUM.SCN",
    "GRANICUS.SCN",
    "ISSUS.SCN",
    "GAUGAMEL.SCN",
    "SAMARKAN.SCN",
    "JAXARTES.SCN",
    "ARIGAEUM.SCN",
    "HYDASPES.SCN",
    "PARAIT.SCN",
];

/// Where each battle's name is on the scenario map, `(x, y, w, h)` in
/// pixels (the original's table).
const PLACES: [(i32, i32, i32, i32); 11] = [
    (38, 80, 79, 11),
    (50, 38, 56, 11),
    (29, 55, 61, 11),
    (64, 67, 67, 11),
    (119, 86, 41, 11),
    (162, 88, 79, 11),
    (242, 41, 82, 11),
    (274, 30, 67, 11),
    (283, 64, 68, 11),
    (297, 76, 70, 11),
    (211, 135, 91, 11),
];

/// Where each battle's label cel (`CAMPAIGN.EIF` strip `Battles`) lies in
/// its place: it matches the name painted on the map there exactly.
const LABEL_AT: [(i32, i32); 10] = [(9, 2), (9, 2), (10, 2), (9, 2), (9, 2), (9, 2), (2, 2), (1, 2), (1, 2), (2, 2)];

/// String table ids of the battle names, and of their description lines.
const NAME_IDS: [u16; 11] = [93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 80];
const DESCRIPTION_IDS: [&[u16]; 11] =
    [&[127, 128], &[129, 130], &[131, 132], &[133, 134], &[135], &[136, 137], &[138], &[139], &[140, 141], &[142], &[81, 82]];

/// A game's battles for the scenario screen: files, where each one's name
/// is on the map (pixels), and the string ids of its name and description.
struct BattleTable {
    files: &'static [&'static str],
    places: &'static [(i32, i32, i32, i32)],
    names: &'static [u16],
    descriptions: &'static [&'static [u16]],
}

const ALEXANDER_BATTLES: BattleTable = BattleTable { files: &SCENARIOS, places: &PLACES, names: &NAME_IDS, descriptions: &DESCRIPTION_IDS };

/// Hannibal's (the original's table, less its unused Beneventum and
/// custom slots; Dertosa only where its scenario is installed).
const HANNIBAL_BATTLES: BattleTable = BattleTable {
    files: &[
        "METAURUS.SCN",
        "BAGRADAS.SCN",
        "CANNAE.SCN",
        "CYNO.SCN",
        "ZAMA.SCN",
        "TREBBIA.SCN",
        "BAECULA.SCN",
        "ILIPA.SCN",
        "GPLAINS.SCN",
        "MAGNESIA.SCN",
        "TRASIMEN.SCN",
        "DERTOSA.SCN",
    ],
    places: &[
        (266, 73, 79, 11),
        (93, 188, 77, 11),
        (307, 123, 60, 11),
        (270, 148, 119, 11),
        (219, 193, 44, 11),
        (176, 63, 67, 11),
        (24, 123, 72, 11),
        (37, 147, 47, 11),
        (215, 181, 108, 11),
        (376, 164, 78, 11),
        (158, 98, 125, 11),
        (84, 102, 67, 11),
    ],
    names: &[93, 94, 95, 96, 97, 98, 99, 100, 103, 113, 114, 176],
    descriptions: &[&[127], &[129], &[131], &[133], &[135], &[137], &[139], &[141], &[161], &[163], &[165], &[177]],
};

/// Caesar's (the original's table, less its custom slot; Rhine, the
/// add-on's, only where its scenario is installed). Each description is
/// two strings joined.
const CAESAR_BATTLES: BattleTable = BattleTable {
    files: &[
        "DYRRHACH.SCN",
        "PHARSAL.SCN",
        "RUSPINA.SCN",
        "THAPSUS.SCN",
        "MUNDA.SCN",
        "VERCELL.SCN",
        "CHAERON.SCN",
        "BIBRAC.SCN",
        "SABIS.SCN",
        "RHINE.SCN",
    ],
    places: &[
        (290, 95, 86, 11),
        (328, 127, 70, 11),
        (194, 146, 51, 11),
        (190, 166, 55, 11),
        (62, 126, 44, 11),
        (199, 44, 68, 11),
        (307, 113, 73, 11),
        (141, 61, 60, 11),
        (175, 14, 36, 11),
        (297, 15, 37, 11),
    ],
    names: &[93, 94, 95, 96, 97, 98, 99, 85, 86, 180],
    descriptions: &[&[127, 100], &[129, 101], &[131, 103], &[133, 113], &[135, 114], &[137, 141], &[139, 146], &[83, 161], &[84, 163], &[181, 182]],
};

/// Hannibal's scenario screen: the box under the map where the chosen
/// battle's overview picture and description go (pixels).
const INFO_BOX: (i32, i32, i32, i32) = (33, 246, 393, 114);

/// Caesar's.
const CAESAR_INFO_BOX: (i32, i32, i32, i32) = (41, 200, 382, 145);

/// Dialog units to pixels.
fn px((x, y): (i16, i16)) -> (i32, i32) {
    (x as i32 * 7 / 4, y as i32 * 13 / 8)
}

/// Which screen is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Start,
    Scenario { selected: Option<usize> },
    /// Who plays each side (home, visitor): the computer or not.
    Sides { battle: usize, computer: [bool; 2] },
    /// The armies' rout levels (home, visitor) and the difficulty for the
    /// home army (0 easy, 1 normal, 2 hard, 3 custom).
    Rout { battle: usize, computer: [bool; 2], levels: [u16; 2], difficulty: usize },
}

/// The armies' rout levels for a difficulty, from the scenario's (home,
/// visitor): easy gives the player's army a quarter more and the other a
/// quarter less, hard the other way round (measured in the original:
/// Issus' 135 and 135 become 168 and 101). The player's army is the home
/// one unless the computer plays it.
pub fn rout_levels(base: [u16; 2], difficulty: usize, computer: [bool; 2]) -> [u16; 2] {
    let more = |l: u16| (l as u32 * 5 / 4) as u16;
    let less = |l: u16| (l as u32 * 3 / 4) as u16;
    let player = if computer[0] && !computer[1] { 1 } else { 0 };
    let mut levels = base;
    match difficulty {
        0 => {
            levels[player] = more(base[player]);
            levels[1 - player] = less(base[1 - player]);
        }
        2 => {
            levels[player] = less(base[player]);
            levels[1 - player] = more(base[1 - player]);
        }
        _ => {}
    }
    levels
}

/// What a click did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Redraw,
    Quit,
    /// Play this scenario, the computer taking the sides marked (home,
    /// visitor), with these rout levels.
    Play { scenario: String, computer: [bool; 2], levels: [u16; 2] },
    /// Load Saved Game.
    Load,
    /// Start Campaign, Multiplayer, View Tutorial, Historical Video.
    Campaign,
    Multiplayer,
    Tutorial,
    History,
    /// More...: choose a scenario file of the player's own.
    Custom,
    /// Tell the player these strings of the executable's string table.
    Message(Vec<u16>),
    /// The rewrite's own: the content manager, its settings, the battles of
    /// every imported title.
    Content,
    Settings,
    Selector,
}

/// The rewrite's buttons on the start screen, top left (dialog units: a
/// big push button each).
const EXTRA: [&str; 3] = ["Content Manager", "Settings", "All Battles"];

fn extra_rect(start: &Option<Dialog>, k: usize) -> (i16, i16, i16, i16) {
    // (a control placed off the dialog is hidden: Hannibal's history video)
    let rects: Vec<_> = start.iter().flat_map(|d| &d.controls).map(|c| c.rect).filter(|r| r.0 >= 0).collect();
    let left = rects.iter().map(|r| r.0).min().unwrap_or(0);
    let right = rects.iter().map(|r| r.0 + r.2).max().unwrap_or(0);
    let bottom = rects.iter().map(|r| r.1 + r.3).max().unwrap_or(0);
    // the game's buttons in one column (Hannibal's): ours go under them, as
    // wide; in rows (Alexander's): in the top left corner
    let column = !rects.is_empty() && right - left < 140;
    if column {
        (left, bottom + 10 + 16 * k as i16, right - left, 14)
    } else {
        (4, 4 + 16 * k as i16, 90, 14)
    }
}

pub struct Setup {
    pub screen: Screen,
    /// Where the left button is held down on the screen, if it is.
    pub pressed: Option<(i32, i32)>,
    font: crate::ui::Font,
    /// The brightest yellow of the palette, for the chosen battle.
    yellow: u8,
    /// The frames' colour.
    dark: u8,
    button: Vec<Sprite>,
    big_button: Vec<Sprite>,
    /// Caesar's start screen's buttons (`Big Stone Pushbutton`: its `Up`
    /// cel is a 1 × 1 placeholder, so its `Down` art serves up and down;
    /// 🟡 the original's look not checked).
    stone_button: Vec<Sprite>,
    radio: Vec<Sprite>,
    checkbox: Vec<Sprite>,
    /// Hannibal's rout dialog options: the battles' turn limits, and the
    /// armies placed as the scenario has them (else the player deploys
    /// his).
    pub turn_limits: bool,
    pub default_deployment: bool,
    start_art: Option<Sprite>,
    map_art: Option<Sprite>,
    marble: Option<Sprite>,
    labels: Vec<Sprite>,
    /// The game, its battles' places on the map, their overview pictures
    /// (Hannibal's), the small font and white (for the chosen battle).
    game: gboh_formats::game::Game,
    places: Vec<(i32, i32, i32, i32)>,
    overviews: Vec<Option<Sprite>>,
    /// Where the chosen battle's picture and description go (Hannibal's and
    /// Caesar's scenario screens).
    info_box: (i32, i32, i32, i32),
    small: crate::ui::Font,
    white: u8,
    names: Vec<String>,
    descriptions: Vec<Vec<String>>,
    /// The two sides' names (home, visitor) of each battle, and their rout
    /// levels.
    sides: Vec<[String; 2]>,
    rout: Vec<[u16; 2]>,
    /// Each battle's scenario file (the built-in ones, then any the player
    /// chose with More...).
    files: Vec<String>,
    /// The nations' names, by nation byte.
    nations: Vec<String>,
    start: Option<Dialog>,
    scenario: Option<Dialog>,
    intro: Option<Dialog>,
    rout_dialog: Option<Dialog>,
}

impl Setup {
    /// The setup screens, from the game's `DATA` directory (and `alex.exe`
    /// beside it, for text and layouts).
    pub fn new(data_dir: &Path) -> Self {
        use crate::ui::{cels, eif, first};
        let dialog = eif(data_dir, "DIALOG.EIF");
        let buttons = eif(data_dir, "DLGBTN.EIF");
        let campaign = eif(data_dir, "CAMPAIGN.EIF");
        let back = eif(data_dir, "BACKGND.EIF");
        let yellow = crate::palette_path(data_dir)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| gboh_formats::pal::Palette::parse(&d).ok())
            .map_or(251, |pal| {
                (10..246).min_by_key(|&i| {
                    let [r, g, b] = pal.colors[i];
                    (255 - r as i32).pow(2) + (230 - g as i32).pow(2) + (b as i32).pow(2)
                }).unwrap_or(251) as u8
            });
        let white = crate::palette_path(data_dir)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| gboh_formats::pal::Palette::parse(&d).ok())
            .map_or(255, |pal| (1..=255usize).max_by_key(|&i| pal.colors[i].iter().map(|&c| c as u32).sum::<u32>()).unwrap_or(255) as u8);
        let dark = crate::palette_path(data_dir)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| gboh_formats::pal::Palette::parse(&d).ok())
            .map_or(0xf8, |pal| (1..=255usize).min_by_key(|&i| pal.colors[i].iter().map(|&c| c as u32).sum::<u32>()).unwrap_or(0xf8) as u8);
        let exe = crate::ui::exe_path(data_dir).and_then(|p| std::fs::read(p).ok());
        let res = exe.and_then(|d| Resources::parse(&d).ok());
        let string = |id: u16| res.as_ref().and_then(|r| r.string(id));
        let game = gboh_formats::game::Game::detect(data_dir).unwrap_or_default();
        use gboh_formats::game::Game;
        let table = match game {
            Game::Hannibal => &HANNIBAL_BATTLES,
            Game::Caesar => &CAESAR_BATTLES,
            Game::Alexander => &ALEXANDER_BATTLES,
        };
        // the battles whose scenarios are there (Hannibal's Dertosa came
        // later)
        let battles: Vec<usize> = (0..table.files.len())
            .filter(|&k| game == gboh_formats::game::Game::Alexander || crate::find_ci(data_dir, table.files[k]).is_some())
            .collect();
        let files: Vec<&str> = battles.iter().map(|&k| table.files[k]).collect();
        let names: Vec<String> = battles
            .iter()
            .map(|&k| string(table.names[k]).unwrap_or_else(|| table.files[k].trim_end_matches(".SCN").to_string()))
            .collect();
        let descriptions = battles
            .iter()
            .map(|&k| {
                let parts: Vec<String> = table.descriptions[k].iter().filter_map(|&id| string(id)).collect();
                // (Caesar's two strings make one text)
                if game == Game::Caesar { vec![parts.concat()] } else { parts }
            })
            .collect();
        let overview = eif(data_dir, "OVERVIEW.EIF");
        // (Caesar's add-on battle's picture is in its own file)
        let add_on = eif(data_dir, "GB-OVER.EIF");
        // (an overview strip is named after its battle; the cel is the
        // half-size picture, the bare map that fits the box)
        let overviews = names
            .iter()
            .map(|n| {
                let file = [&overview, &add_on].into_iter().find(|f| f.as_ref().is_some_and(|e| e.strips.iter().any(|s| s.name == *n)))?;
                let strip = file.as_ref()?.strips.iter().find(|s| s.name == *n)?;
                let k = strip.cels.iter().position(|c| c.name.contains("Half")).unwrap_or(strip.cels.len().saturating_sub(1));
                cels(file, n).into_iter().nth(k)
            })
            .collect();
        let places = battles.iter().map(|&k| table.places[k]).collect();
        // nation bytes 0 Greek, 1 Persian, 2 Macedonian, 3 Indian, 4
        // Danubian, 5 Scythian; the string table has Greek, Macedonian,
        // Persian, Danubian, Indian, Scythian from string 31
        // (Hannibal's Romans and Carthaginians at 79 and 80; its Seleucids
        // have no name of their own: 🟡 Persian, as they're drawn)
        // (Caesar's by its own table: Mithridates' army in the Greek slot,
        // Roman, Caesarian, Pompeian, Tribal)
        let nation = |n: u8| {
            const ORDER: [u16; 9] = [31, 33, 32, 35, 34, 36, 79, 80, 32];
            const CAESAR: [(u8, u16); 5] = [(0, 31), (6, 79), (9, 176), (10, 177), (11, 178)];
            let id = if game == Game::Caesar { CAESAR.iter().find(|c| c.0 == n).map(|c| c.1) } else { ORDER.get(n as usize).copied() };
            id.and_then(string).unwrap_or_else(|| format!("Nation {n}"))
        };
        let scenarios: Vec<_> = files
            .iter()
            .map(|f| {
                crate::find_ci(data_dir, f)
                    .and_then(|p| std::fs::read(p).ok())
                    .and_then(|d| gboh_formats::scn::Scenario::parse(&d).ok())
            })
            .collect();
        let sides = scenarios
            .iter()
            .map(|scn| scn.as_ref().map_or_else(|| ["Home".into(), "Visitor".into()], |s| [nation(s.home.nation), nation(s.visitor.nation)]))
            .collect();
        let rout = scenarios
            .iter()
            .map(|scn| scn.as_ref().map_or([100, 100], |s| [s.home.rout_level, s.visitor.rout_level]))
            .collect();
        Self {
            screen: Screen::Start,
            pressed: None,
            // Hannibal's font and frames in its darkest colour (Alexander's
            // glyphs are dark as they are)
            font: if game == gboh_formats::game::Game::Alexander { crate::ui::Font::load(data_dir) } else { crate::ui::Font::load(data_dir).in_ink(dark) },
            dark: if game == gboh_formats::game::Game::Alexander { 0xf8 } else { dark },
            yellow,
            button: cels(&buttons, "Pushbutton"),
            big_button: cels(&buttons, "Big Pushbutton"),
            stone_button: if game == Game::Caesar { cels(&buttons, "Big Stone Pushbutton").into_iter().skip(1).take(1).collect() } else { Vec::new() },
            radio: cels(&buttons, "Radio"),
            checkbox: cels(&buttons, "CheckBox"),
            turn_limits: false,
            default_deployment: true,
            start_art: first(&dialog, "Start Game"),
            // Hannibal's with the add-on's map (Dertosa's scenario there)
            // Hannibal's with the add-on's map (Dertosa's scenario there);
            // Caesar's likewise (Rhine's), else its own scenario map
            map_art: match game {
                Game::Hannibal if crate::find_ci(data_dir, "DERTOSA.SCN").is_some() => first(&eif(data_dir, "GB-DLG.EIF"), "Scenario Map"),
                Game::Caesar if crate::find_ci(data_dir, "RHINE.SCN").is_some() => first(&eif(data_dir, "GB-DLG.EIF"), "Scenario Map"),
                Game::Caesar => first(&dialog, "Scenario Map"),
                _ => None,
            }
            .or_else(|| first(&dialog, "Campaign Map")),
            info_box: if game == Game::Caesar { CAESAR_INFO_BOX } else { INFO_BOX },
            marble: first(&back, "Blocks"),
            labels: cels(&campaign, "Battles"),
            game,
            places,
            overviews,
            small: crate::ui::Font::small(data_dir),
            white,
            names,
            descriptions,
            sides,
            rout,
            files: files.iter().map(|f| f.to_string()).collect(),
            nations: (0..12).map(nation).collect(),
            start: res.as_ref().and_then(|r| r.dialog("STARTGAME")),
            scenario: res.as_ref().and_then(|r| r.dialog("SCENARIO")),
            intro: res.as_ref().and_then(|r| r.dialog("INTRODLG")),
            rout_dialog: res.as_ref().and_then(|r| r.dialog("ARMYROUTDLG")),
        }
    }

    /// The scenario files in `data_dir` that are not the built-in battles'
    /// (what More... offers).
    pub fn custom_files(data_dir: &Path) -> Vec<String> {
        let mut files: Vec<String> = std::fs::read_dir(data_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| {
                let builtin = SCENARIOS.iter().chain(HANNIBAL_BATTLES.files).chain(CAESAR_BATTLES.files).any(|b| b.eq_ignore_ascii_case(n));
                // (Hannibal hides its two early drafts)
                let draft = n.eq_ignore_ascii_case("ILIPA2.SCN") || n.eq_ignore_ascii_case("CANNAE2.SCN");
                n.to_ascii_lowercase().ends_with(".scn") && !builtin && !draft
            })
            // (only what reads as a scenario: Caesar's editor leaves its
            // own `Last.scn`)
            .filter(|n| std::fs::read(data_dir.join(n)).ok().is_some_and(|d| gboh_formats::scn::Scenario::parse(&d).is_ok()))
            .collect();
        files.sort();
        files
    }

    /// Go on with scenario `file` as a battle of its own (the original's
    /// index 11, "Custom"): who plays it is asked next.
    pub fn choose_custom(&mut self, data_dir: &Path, file: &str) -> bool {
        let Some(scn) = crate::find_ci(data_dir, file).and_then(|p| std::fs::read(p).ok()).and_then(|d| gboh_formats::scn::Scenario::parse(&d).ok()) else {
            return false;
        };
        let nation = |n: u8| self.nations.get(n as usize).cloned().unwrap_or_default();
        self.sides.push([nation(scn.home.nation), nation(scn.visitor.nation)]);
        self.rout.push([scn.home.rout_level, scn.visitor.rout_level]);
        self.files.push(file.to_string());
        self.screen = Screen::Sides { battle: self.files.len() - 1, computer: [false, true] };
        true
    }

    /// Whether checkbox `id` of the rout dialog is ticked.
    fn checked(&self, id: u16) -> bool {
        if id == TURN_LIMITS { self.turn_limits } else { self.default_deployment }
    }

    fn text_width(&self, text: &str) -> i32 {
        self.font.width(text)
    }

    fn text(&self, canvas: &mut Canvas, text: &str, x: i32, y: i32) {
        self.font.draw(canvas, text, x, y);
    }

    /// A push button at `rect` (dialog units) with its caption (the `&`
    /// marking the shortcut left out).
    fn button(&self, canvas: &mut Canvas, rect: (i16, i16, i16, i16), caption: &str) {
        let (x, y) = px((rect.0, rect.1));
        let (w, h) = px((rect.2, rect.3));
        // (the stone for the game's own buttons, not ours: 115 units wide)
        let art = if self.screen == Screen::Start && !self.stone_button.is_empty() && rect.2 > 100 {
            &self.stone_button
        } else if w > 120 {
            &self.big_button
        } else {
            &self.button
        };
        // pressed while the left button is held on it
        let down = self.pressed.is_some_and(|(px, py)| px >= x && px < x + w && py >= y && py < y + h) as i32;
        if let Some(b) = art.get(down as usize).or(art.first()) {
            canvas.blit(b, x + (w - b.width as i32) / 2, y + (h - b.height as i32) / 2);
        }
        let caption: String = caption.chars().filter(|&c| c != '&').collect();
        let (tx, ty) = (x + (w - self.text_width(&caption)) / 2 + down, y + (h - 11) / 2 + down);
        if std::ptr::eq(art, &self.stone_button) {
            // (on the stone's dark middle: light, shadowed; 🟡 the
            // original's ink not checked)
            self.font.draw_in(canvas, &caption, tx + 1, ty + 1, self.dark);
            self.font.draw_in(canvas, &caption, tx, ty, self.white);
        } else {
            self.text(canvas, &caption, tx, ty);
        }
    }

    /// The control of `dialog` under pixel `(x, y)`, by id.
    fn control_at(dialog: &Option<Dialog>, x: i32, y: i32) -> Option<u16> {
        dialog.as_ref()?.controls.iter().find_map(|c| {
            let (cx, cy) = px((c.rect.0, c.rect.1));
            let (w, h) = px((c.rect.2, c.rect.3));
            (x >= cx && x < cx + w && y >= cy && y < cy + h).then_some(c.id)
        })
    }

    /// A `w` × `h` canvas of the screens' marble, to set a screen on.
    pub fn backdrop(&self, w: usize, h: usize) -> Canvas {
        let mut canvas = Canvas::new(w, h, 0);
        if let Some(m) = &self.marble {
            canvas.tile(m, 0, 0);
        }
        canvas
    }

    /// A marble canvas the size of `dialog`.
    fn marble_canvas(&self, dialog: &Option<Dialog>, fallback: (i16, i16)) -> Canvas {
        let (w, h) = px(dialog.as_ref().map_or(fallback, |d| d.size));
        let mut canvas = Canvas::new(w as usize, h as usize, 0);
        if let Some(m) = &self.marble {
            canvas.tile(m, 0, 0);
        }
        canvas
    }

    /// The screen as it looks now.
    pub fn canvas(&self) -> Canvas {
        match self.screen {
            Screen::Start => {
                let mut canvas = Canvas::new(640, 480, 0);
                if let Some(a) = &self.start_art {
                    canvas.blit(a, 0, 0);
                }
                for c in self.start.iter().flat_map(|d| &d.controls) {
                    self.button(&mut canvas, c.rect, &c.text);
                }
                // the rewrite's own: back to the content manager, its
                // settings, and every imported title's battles
                for (k, label) in EXTRA.iter().enumerate() {
                    self.button(&mut canvas, extra_rect(&self.start, k), label);
                }
                canvas
            }
            Screen::Scenario { selected } if self.game != gboh_formats::game::Game::Alexander => {
                let (w, h) = self.scenario.as_ref().map_or((460, 422), |d| px(d.size));
                let mut canvas = Canvas::new(w as usize, h as usize, 0);
                if let Some(a) = &self.map_art {
                    canvas.blit(a, 0, 0);
                }
                if let Some(k) = selected {
                    // the chosen battle framed (🟡 the original's mark not
                    // traced), its picture and description in the box
                    let (x, y, pw, ph) = self.places[k];
                    frame_in(&mut canvas, (x - 1, y - 1, pw + 2, ph + 2), self.white);
                    let (bx, by, bw, _) = self.info_box;
                    let pic_w = self.overviews[k].as_ref().map_or(0, |p| {
                        canvas.blit(p, bx, by);
                        p.width as i32
                    });
                    let mut line_y = by + 2;
                    for line in &self.descriptions[k] {
                        for part in wrap(line, bw - pic_w - 4, |t| self.small.width(t)) {
                            self.small.draw(&mut canvas, &part, bx + pic_w + 4, line_y);
                            line_y += 12;
                        }
                    }
                }
                for c in self.scenario.iter().flat_map(|d| &d.controls) {
                    // `More…` lies outside the dialog
                    if (c.rect.0 as i32) < self.scenario.as_ref().map_or(263, |d| d.size.0 as i32) {
                        self.button(&mut canvas, c.rect, &c.text);
                    }
                }
                canvas
            }
            Screen::Scenario { selected } => {
                let mut canvas = Canvas::new(405, 396, 0);
                if let Some(a) = &self.map_art {
                    canvas.blit(a, 0, 0);
                }
                // the battle not painted on the map has its name written
                let (x, y, _, _) = PLACES[10];
                self.text(&mut canvas, &self.names[10].to_uppercase(), x, y);
                if let Some(k) = selected {
                    // the chosen battle's name stands out (🟡 how the
                    // original marks it is not traced: here in yellow)
                    let (x, y, _, _) = PLACES[k];
                    if let (Some(l), Some(&(dx, dy))) = (self.labels.get(k), LABEL_AT.get(k)) {
                        let mut lit = l.clone();
                        for p in lit.pixels.iter_mut().filter(|p| **p != 0) {
                            *p = self.yellow;
                        }
                        canvas.blit(&lit, x + dx, y + dy);
                    } else if k == 10 {
                        self.text(&mut canvas, &self.names[10].to_uppercase(), x + 1, y);
                    }
                    // and its description is on the scroll
                    self.text(&mut canvas, &self.names[k], 70, 196);
                    let mut line_y = 214;
                    for line in &self.descriptions[k] {
                        for part in wrap(line, 255, |t| self.text_width(t)) {
                            self.text(&mut canvas, &part, 70, line_y);
                            line_y += 12;
                        }
                    }
                }
                for c in self.scenario.iter().flat_map(|d| &d.controls) {
                    // `More…` and `Units…` lie outside the dialog
                    if c.rect.0 < 229 {
                        self.button(&mut canvas, c.rect, &c.text);
                    }
                }
                canvas
            }
            Screen::Sides { battle, computer } => {
                let mut canvas = self.marble_canvas(&self.intro, (195, 115));
                for c in self.intro.iter().flat_map(|d| &d.controls) {
                    let (x, y) = px((c.rect.0, c.rect.1));
                    match c.id {
                        1 => self.button(&mut canvas, c.rect, &c.text),
                        101 | 102 => self.text(&mut canvas, &self.sides[battle][(c.id - 101) as usize], x, y),
                        103..=106 => {
                            let side = ((c.id - 103) / 2) as usize;
                            let on = computer[side] == (c.id % 2 == 1);
                            if let Some(r) = self.radio.get(on as usize) {
                                canvas.blit(r, x, y);
                            }
                            self.text(&mut canvas, &c.text, x + 16, y);
                        }
                        _ => self.text(&mut canvas, &c.text, x, y),
                    }
                }
                canvas
            }
            Screen::Rout { battle, levels, difficulty, computer } => {
                let mut canvas = self.marble_canvas(&self.rout_dialog, (184, 168));
                // the armies' names and spinners, laid out as in the
                // original (drawn by its code, not the template)
                for (side, &centre) in ROUT_COLUMNS.iter().enumerate() {
                    let name = &self.sides[battle][side];
                    self.text(&mut canvas, name, centre - self.text_width(name) / 2, 16);
                    let (x, y, w, h) = spinner(centre);
                    frame_in(&mut canvas, (x, y, w, h), self.dark);
                    let value = levels[side].to_string();
                    self.text(&mut canvas, &value, x + (w - 16 - self.text_width(&value)) / 2, y + (h - 11) / 2);
                    frame_in(&mut canvas, (x + w - 16, y, 16, h / 2), self.dark);
                    frame_in(&mut canvas, (x + w - 16, y + h / 2, 16, h - h / 2), self.dark);
                    self.text(&mut canvas, "+", x + w - 11, y + 1);
                    self.text(&mut canvas, "-", x + w - 10, y + h / 2 + 1);
                }
                let player = if computer[0] && !computer[1] { 1 } else { 0 };
                let label = format!("{} Difficulty Level", self.sides[battle][player]);
                let width = canvas.width as i32;
                self.text(&mut canvas, &label, (width - self.text_width(&label)) / 2, 100);
                for c in self.rout_dialog.iter().flat_map(|d| &d.controls) {
                    let (x, y) = px((c.rect.0, c.rect.1));
                    match c.id {
                        10..=13 => {
                            let on = difficulty == (c.id - 10) as usize;
                            if let Some(r) = self.radio.get(on as usize) {
                                canvas.blit(r, x, y);
                            }
                            let caption: String = c.text.chars().filter(|&ch| ch != '&').collect();
                            self.text(&mut canvas, &caption, x + 16, y);
                        }
                        // Hannibal's checkboxes and the static beside them
                        _ if is_checkbox(c) => {
                            if let Some(b) = self.checkbox.get(self.checked(c.id) as usize) {
                                canvas.blit(b, x, y);
                            }
                            let caption: String = c.text.chars().filter(|&ch| ch != '&').collect();
                            self.text(&mut canvas, &caption, x + 16, y);
                        }
                        _ if c.class == gboh_formats::pe::Name::Id(0x82) => self.text(&mut canvas, &c.text, x, y),
                        _ => self.button(&mut canvas, c.rect, &c.text),
                    }
                }
                canvas
            }
        }
    }

    /// A click at pixel `(x, y)` of the screen.
    pub fn click(&mut self, x: i32, y: i32) -> Outcome {
        match self.screen {
            Screen::Start if (0..EXTRA.len()).any(|k| {
                let (rx, ry) = px((extra_rect(&self.start, k).0, extra_rect(&self.start, k).1));
                let (rw, rh) = px((extra_rect(&self.start, k).2, extra_rect(&self.start, k).3));
                x >= rx && x < rx + rw && y >= ry && y < ry + rh
            }) =>
            {
                let k = (0..EXTRA.len())
                    .find(|&k| {
                        let (rx, ry) = px((extra_rect(&self.start, k).0, extra_rect(&self.start, k).1));
                        let (rw, rh) = px((extra_rect(&self.start, k).2, extra_rect(&self.start, k).3));
                        x >= rx && x < rx + rw && y >= ry && y < ry + rh
                    })
                    .unwrap_or(0);
                [Outcome::Content, Outcome::Settings, Outcome::Selector][k].clone()
            }
            Screen::Start => match Self::control_at(&self.start, x, y) {
                Some(106) => {
                    self.screen = Screen::Scenario { selected: None };
                    Outcome::Redraw
                }
                Some(2) => Outcome::Quit,
                Some(101) => Outcome::Load,
                Some(102) => Outcome::Campaign,
                Some(103) => Outcome::Multiplayer,
                Some(104) => Outcome::Tutorial,
                Some(105) => Outcome::History,
                _ => Outcome::Nothing,
            },
            Screen::Scenario { selected } => {
                if let Some(k) = self.places.iter().position(|&(px, py, w, h)| x >= px && x < px + w && y >= py && y < py + h) {
                    self.screen = Screen::Scenario { selected: Some(k) };
                    return Outcome::Redraw;
                }
                match Self::control_at(&self.scenario, x, y) {
                    Some(1) if selected.is_some() => {
                        // the computer plays the visitor by default
                        self.screen = Screen::Sides { battle: selected.unwrap(), computer: [false, true] };
                        Outcome::Redraw
                    }
                    Some(2) => {
                        self.screen = Screen::Start;
                        Outcome::Redraw
                    }
                    // the original's Details and Units open its help, whose
                    // calls the GOG build has cut: a pointer to the manual
                    // (Paraitacene's Details: its description)
                    Some(10) if selected == Some(10) && self.game == gboh_formats::game::Game::Alexander => Outcome::Message(DESCRIPTION_IDS[10].to_vec()),
                    Some(9 | 10) if selected.is_some() => Outcome::Message(vec![83]),
                    Some(11) => Outcome::Custom,
                    _ => Outcome::Nothing,
                }
            }
            Screen::Sides { battle, mut computer } => match Self::control_at(&self.intro, x, y) {
                Some(id @ 103..=106) => {
                    computer[((id - 103) / 2) as usize] = id % 2 == 1;
                    self.screen = Screen::Sides { battle, computer };
                    Outcome::Redraw
                }
                Some(1) => {
                    self.screen = Screen::Rout { battle, computer, levels: rout_levels(self.rout[battle], 1, computer), difficulty: 1 };
                    Outcome::Redraw
                }
                _ => Outcome::Nothing,
            },
            Screen::Rout { battle, computer, mut levels, .. } => {
                // the spinners' arrows: a level up or down, making it custom
                for (side, &centre) in ROUT_COLUMNS.iter().enumerate() {
                    let (sx, sy, w, h) = spinner(centre);
                    if x >= sx + w - 16 && x < sx + w && y >= sy && y < sy + h {
                        levels[side] = if y < sy + h / 2 { levels[side].saturating_add(1) } else { levels[side].saturating_sub(1).max(1) };
                        self.screen = Screen::Rout { battle, computer, levels, difficulty: 3 };
                        return Outcome::Redraw;
                    }
                }
                match Self::control_at(&self.rout_dialog, x, y) {
                    Some(id @ 10..=13) => {
                        let difficulty = (id - 10) as usize;
                        if difficulty < 3 {
                            levels = rout_levels(self.rout[battle], difficulty, computer);
                        }
                        self.screen = Screen::Rout { battle, computer, levels, difficulty };
                        Outcome::Redraw
                    }
                    Some(id) if self.rout_dialog.iter().flat_map(|d| &d.controls).any(|c| c.id == id && is_checkbox(c)) => {
                        if id == TURN_LIMITS {
                            self.turn_limits = !self.turn_limits;
                        } else {
                            self.default_deployment = !self.default_deployment;
                        }
                        Outcome::Redraw
                    }
                    Some(1) => Outcome::Play { scenario: self.files[battle].clone(), computer, levels },
                    _ => Outcome::Nothing,
                }
            }
        }
    }
}

/// The centres of the two armies' columns on the rout level dialog, and
/// where their spinners are (pixels, as measured on the original).
const ROUT_COLUMNS: [i32; 2] = [88, 245];

fn spinner(centre: i32) -> (i32, i32, i32, i32) {
    (centre - 28, 49, 56, 35)
}

/// Hannibal's rout dialog checkboxes: turn limits (and, the other one,
/// default deployments).
const TURN_LIMITS: u16 = 14;

/// Whether a dialog control is a checkbox (a button styled `BS_CHECKBOX` or
/// `BS_AUTOCHECKBOX`).
fn is_checkbox(c: &gboh_formats::pe::Control) -> bool {
    c.class == gboh_formats::pe::Name::Id(0x80) && matches!(c.style & 0xf, 2 | 3)
}

/// A one-pixel frame around `rect` in `colour`.
fn frame_in(canvas: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), colour: u8) {
    let mut put = |px: i32, py: i32| {
        if px >= 0 && py >= 0 && (px as usize) < canvas.width && (py as usize) < canvas.height {
            canvas.pixels[py as usize * canvas.width + px as usize] = colour;
        }
    };
    for i in x..x + w {
        put(i, y);
        put(i, y + h - 1);
    }
    for j in y..y + h {
        put(x, j);
        put(x + w - 1, j);
    }
}

/// `text` broken into lines no wider than `width`.
fn wrap(text: &str, width: i32, measure: impl Fn(&str) -> i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if measure(&candidate) > width && !line.is_empty() {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Goes through the screens; runs only with `GBOH_DATA` set.
    /// Hannibal's scenario screen: its battles on the map, Cannae chosen
    /// with its picture and description; runs only with `GBOH_HANNIBAL`.
    #[test]
    fn hannibal_scenario_screen() {
        let Ok(dir) = std::env::var("GBOH_HANNIBAL") else { return };
        let dir = Path::new(&dir);
        let mut s = Setup::new(dir);
        assert!(s.files.len() >= 11, "{:?}", s.files);
        s.screen = Screen::Scenario { selected: None };
        // a click on Cannae's name chooses it
        assert_eq!(s.click(307 + 5, 123 + 5), Outcome::Redraw);
        assert_eq!(s.screen, Screen::Scenario { selected: Some(2) });
        let c = s.canvas();
        assert!(s.overviews[2].is_some());
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(crate::palette_path(dir).unwrap()).unwrap()).unwrap();
            crate::ui::write_ppm(&c, &pal, &Path::new(&out).join("han_scenario.ppm")).unwrap();
            s.screen = Screen::Rout { battle: 2, computer: [false, true], levels: s.rout[2], difficulty: 1 };
            crate::ui::write_ppm(&s.canvas(), &pal, &Path::new(&out).join("han_rout.ppm")).unwrap();
        }
    }

    /// Caesar's scenario screen: its ten battles (Rhine's with the
    /// add-on), Pharsalus chosen by its name, its picture and its sides.
    #[test]
    fn caesar_scenario_screen() {
        let Ok(dir) = std::env::var("GBOH_CAESAR") else { return };
        let dir = Path::new(&dir);
        let mut s = Setup::new(dir);
        assert_eq!(s.files.len(), 10, "{:?}", s.files);
        s.screen = Screen::Scenario { selected: None };
        assert_eq!(s.click(328 + 5, 127 + 5), Outcome::Redraw);
        assert_eq!(s.screen, Screen::Scenario { selected: Some(1) });
        let c = s.canvas();
        assert!(s.overviews.iter().all(|o| o.is_some()));
        assert_eq!(s.descriptions[1].len(), 1);
        assert!(s.sides.iter().all(|[a, b]| !a.starts_with("Nation") && !b.starts_with("Nation")));
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(crate::palette_path(dir).unwrap()).unwrap()).unwrap();
            crate::ui::write_ppm(&c, &pal, &Path::new(&out).join("cae_scenario.ppm")).unwrap();
            s.screen = Screen::Start;
            crate::ui::write_ppm(&s.canvas(), &pal, &Path::new(&out).join("cae_start.ppm")).unwrap();
        }
    }

    #[test]
    fn sets_up_a_battle() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let mut s = Setup::new(Path::new(&dir));
        let start = s.canvas();
        assert_eq!((start.width, start.height), (640, 480));
        // Start Battle: (93, 257) dialog units, 90 × 14
        let (x, y) = px((93 + 45, 257 + 7));
        assert_eq!(s.click(x, y), Outcome::Redraw);
        assert_eq!(s.screen, Screen::Scenario { selected: None });
        // Issus on the map, then OK
        assert_eq!(s.click(130, 90), Outcome::Redraw);
        assert_eq!(s.screen, Screen::Scenario { selected: Some(4) });
        let _ = s.canvas();
        let (x, y) = px((34 + 25, 221 + 7));
        assert_eq!(s.click(x, y), Outcome::Redraw);
        let _ = s.canvas();
        // OK on Leader Control, then Easy and OK on the rout levels
        let (x, y) = px((70 + 25, 94 + 7));
        assert_eq!(s.click(x, y), Outcome::Redraw);
        assert_eq!(s.screen, Screen::Rout { battle: 4, computer: [false, true], levels: [135, 135], difficulty: 1 });
        let (x, y) = px((72 + 5, 76 + 6));
        assert_eq!(s.click(x, y), Outcome::Redraw);
        let (x, y) = px((34 + 25, 148 + 7));
        assert_eq!(s.click(x, y), Outcome::Play { scenario: "ISSUS.SCN".into(), computer: [false, true], levels: [168, 101] });
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            for (name, screen) in [
                ("start", Screen::Start),
                ("scenario", Screen::Scenario { selected: Some(4) }),
                ("sides", Screen::Sides { battle: 4, computer: [false, true] }),
                ("rout", Screen::Rout { battle: 4, computer: [false, true], levels: [168, 101], difficulty: 0 }),
            ] {
                s.screen = screen;
                let c = s.canvas();
                let pal = gboh_formats::pal::Palette::parse(&std::fs::read(Path::new(&dir).join("GAME.PAL")).unwrap()).unwrap();
                let rgba = c.to_rgba(&pal);
                let mut ppm = format!("P6 {} {} 255\n", c.width, c.height).into_bytes();
                for p in rgba.chunks(4) {
                    ppm.extend_from_slice(&p[..3]);
                }
                std::fs::write(format!("{out}/setup_{name}.ppm"), ppm).unwrap();
            }
        }
    }
}
