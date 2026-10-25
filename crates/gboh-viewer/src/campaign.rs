//! The campaign's screens: the map (`CAMPAIGNMAP`, drawn on the `Campaign
//! Game` bitmap with the `Campaign Map` art of `CAMPAIGN.EIF`, as the
//! original's `FUN_00586277`) and the results (`CAMPAIGNRESULTS`, on the
//! `Campaign Victory` or `Defeat Screen` bitmap). Places are the original's
//! (its tables built at `0x588cc2`), in map pixels; the map shows through a
//! window 529 × 357 pixels at (35, 17) and scrolls across.

use crate::ui::{Font, cels, eif, first};
use gboh_game::campaign::{self, Campaign};
use gboh_render::{Canvas, Sprite};
use std::path::Path;

/// The map's window on the dialog: left, top, width, height.
const VIEW: (i32, i32, i32, i32) = (35, 17, 529, 357);

/// Per province: the shade and outline origin, name, unit icons, numerals
/// and army flag.
const PLACES: [[(i32, i32); 5]; campaign::PROVINCES] = [
    [(53, 18), (111, 38), (111, 57), (112, 27), (137, 16)],
    [(168, 18), (254, 93), (255, 112), (256, 82), (281, 71)],
    [(261, 161), (303, 246), (304, 264), (305, 234), (330, 223)],
    [(44, 212), (176, 282), (177, 301), (178, 271), (203, 260)],
    [(345, 69), (363, 134), (364, 153), (365, 123), (390, 112)],
    [(515, 121), (583, 233), (584, 252), (585, 222), (610, 211)],
    [(495, 18), (572, 73), (573, 92), (574, 62), (599, 51)],
    [(781, 68), (822, 200), (823, 219), (824, 189), (849, 178)],
];

/// Where the `Scroll Ends` cels go on the dialog: their knobs (at (6, 1)
/// and (534, 1) in the cels) on the `Campaign Game` bitmap's (at (30, 3) and
/// (558, 3)). Their shadows (index 255) darken what is under them.
const SCROLL_ENDS: (i32, i32) = (24, 2);

/// The fleet's markers off Anatolia, the Levant and Egypt.
const NAVY: [(i32, i32); 3] = [(211, 170), (268, 203), (199, 219)];

/// The battles' labels (x, y, w, h; the label 2 pixels in).
const LABELS: [(i32, i32, i32, i32); 10] = [
    (79, 127, 74, 11),
    (186, 24, 43, 11),
    (49, 69, 36, 11),
    (183, 102, 55, 15),
    (318, 183, 23, 11),
    (428, 122, 74, 11),
    (677, 51, 82, 11),
    (769, 29, 62, 11),
    (803, 129, 47, 11),
    (839, 156, 59, 11),
];

/// The province each battle is fought in.
const BATTLE_PROVINCE: [usize; 10] = [0, 0, 0, 1, 2, 4, 6, 6, 7, 7];

/// The log box: left, top, width, height.
const LOG: (i32, i32, i32, i32) = (47, 374, 505, 43);

/// A campaign map's geometry: its window on the dialog and how far down the
/// map sits in it; per province where its shade (and outline and hit mask),
/// name, icons, numerals and flag go; the fleet's markers; whether battles
/// are labelled; the dissolve's cell size.
struct Places {
    view: (i32, i32, i32, i32),
    map_dy: i32,
    area: Vec<(i32, i32)>,
    names: Vec<Option<(i32, i32)>>,
    icons: Vec<(i32, i32)>,
    numerals: Vec<Option<(i32, i32)>>,
    flag: Vec<(i32, i32)>,
    navy: Vec<(usize, (i32, i32))>,
    labels: bool,
    cell: usize,
}

impl Places {
    fn alexander() -> Self {
        Self {
            view: VIEW,
            map_dy: 0,
            area: PLACES.iter().map(|p| p[0]).collect(),
            names: PLACES.iter().map(|p| Some(p[1])).collect(),
            icons: PLACES.iter().map(|p| p[2]).collect(),
            numerals: PLACES.iter().map(|p| Some(p[3])).collect(),
            flag: PLACES.iter().map(|p| p[4]).collect(),
            navy: NAVY.iter().enumerate().map(|(k, &at)| (k + 1, at)).collect(),
            labels: true,
            cell: 5,
        }
    }

    /// Hannibal's (`CampaignDlg`, static init `0x595f7e`): the window at
    /// (35, 8), the map 11 pixels down in it; no names (painted on the map),
    /// numerals or labels; 2 × 2 dissolve cells.
    fn hannibal() -> Self {
        const AREA: [(i32, i32); 10] = [(368, 227), (71, 235), (20, 103), (50, 46), (265, 1), (434, 10), (532, 4), (528, 99), (618, 139), (551, 216)];
        const ICONS: [(i32, i32); 10] = [(409, 254), (131, 268), (29, 162), (151, 117), (294, 43), (457, 45), (568, 35), (542, 130), (648, 168), (583, 235)];
        Self {
            view: (35, 8, 529, 357),
            map_dy: 11,
            area: AREA.to_vec(),
            names: vec![None; 10],
            icons: ICONS.to_vec(),
            numerals: vec![None; 10],
            flag: ICONS.iter().map(|&(x, y)| (x, y - 21)).collect(),
            navy: vec![(4, (405, 97)), (8, (602, 196)), (9, (555, 239))],
            labels: false,
            cell: 2,
        }
    }

    /// Caesar's (`CampaignDlg`, static init `0x596d00`): the window at
    /// (35, 15); per province its outline and hit mask's origin and its
    /// flags' place; the fleets' markers (Numidia's, Graecia's).
    fn caesar() -> Self {
        const AREA: [(i32, i32); 13] =
            [(61, 82), (134, 21), (282, 21), (319, 73), (379, 160), (406, 21), (479, 90), (540, 97), (668, 173), (539, 253), (362, 235), (223, 173), (39, 187)];
        const FLAG: [(i32, i32); 13] =
            [(82, 124), (186, 59), (328, 24), (359, 103), (361, 169), (451, 35), (496, 127), (634, 161), (698, 253), (568, 296), (396, 302), (285, 252), (64, 266)];
        Self {
            view: (35, 15, 529, 357),
            map_dy: 0,
            area: AREA.to_vec(),
            names: vec![None; 13],
            icons: FLAG.to_vec(),
            numerals: vec![None; 13],
            flag: FLAG.to_vec(),
            navy: vec![(0, (376, 190)), (1, (461, 142))],
            labels: false,
            cell: 2,
        }
    }
}

/// Unit icon values, largest first, and their cels (`One`, `Five`, `Ten`,
/// `Fifty`).
const VALUES: [u16; 4] = [50, 10, 5, 1];
const VALUE_CEL: [usize; 4] = [3, 2, 1, 0];

/// What is being dragged across the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dragging {
    /// The army's flag.
    Flag,
    /// A garrison icon: from which province, its kind and value.
    Unit { from: usize, kind: usize, value: u16 },
    /// The map itself, from this scroll and mouse x.
    Map { scroll: i32, x: i32 },
}

/// The map screen's state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MapState {
    pub scroll: i32,
    /// What is dragged and where the mouse is (dialog pixels).
    pub dragging: Option<(Dragging, (i32, i32))>,
    /// The province the dragged thing would drop on.
    pub hover: Option<usize>,
    /// Battle and Subjugate enabled.
    pub battle: bool,
    pub subjugate: bool,
    /// The log's lines, as worded.
    pub log: Vec<String>,
    /// The provinces whose hold changed at the round's opening, each
    /// dissolving from its old shading in turn (`FUN_0058842c`): province,
    /// old level; and when the first began.
    pub changes: std::collections::VecDeque<(usize, u8)>,
    pub change_start: Option<std::time::Instant>,
    /// Where the left button is held down (window pixels), if it is.
    pub pressed: Option<(i32, i32)>,
    /// Garrison icons the player split or merged (double-click): per
    /// province and kind, how many of each value (50, 10, 5, 1); dropped
    /// when that province's garrison changes, which regroups it.
    pub split: std::collections::HashMap<(usize, usize), [u16; 4]>,
}

/// How long a change takes: a pause, the dissolve, a pause (the original
/// sleeps 500 ms before and after; 🟡 its dissolve's pace is not traced).
pub const CHANGE: f32 = 2.0;
const DISSOLVE: (f32, f32) = (0.5, 1.5);

impl MapState {
    /// The change under way and how far its dissolve has got (0 to 1).
    fn changing(&self) -> Option<(usize, u8, f32)> {
        let &(p, old) = self.changes.front()?;
        let t = self.change_start.map_or(0.0, |s| s.elapsed().as_secs_f32());
        Some((p, old, ((t - DISSOLVE.0) / (DISSOLVE.1 - DISSOLVE.0)).clamp(0.0, 1.0)))
    }

    /// Move on once a change is done; returns whether one is still under
    /// way (to keep redrawing).
    pub fn advance(&mut self) -> bool {
        if self.changes.is_empty() {
            return false;
        }
        let start = *self.change_start.get_or_insert_with(std::time::Instant::now);
        if start.elapsed().as_secs_f32() >= CHANGE {
            self.changes.pop_front();
            self.change_start = (!self.changes.is_empty()).then(std::time::Instant::now);
        }
        true
    }
}

/// Times a province's shade is drawn at a subjugation level.
fn shade_times(level: u8) -> usize {
    match level {
        campaign::FREE => 2,
        campaign::PARTIAL => 1,
        _ => 0,
    }
}

/// `s` with only the `cell` × `cell` squares of the first `n` of a
/// shuffled order kept (`keep`), or only the others.
fn cells(s: &Sprite, progress: f32, keep: bool, cell: usize) -> Sprite {
    let (cw, ch) = (s.width.div_ceil(cell), s.height.div_ceil(cell));
    let count = cw * ch;
    let mut order: Vec<usize> = (0..count).collect();
    let mut seed = 0x2545_f491u32;
    for i in (1..count).rev() {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        order.swap(i, (seed >> 8) as usize % (i + 1));
    }
    let mut new = vec![false; count];
    for &c in &order[..((progress * count as f32) as usize).min(count)] {
        new[c] = true;
    }
    let mut out = s.clone();
    for y in 0..s.height {
        for x in 0..s.width {
            if new[(y / cell) * cw + x / cell] != keep {
                out.pixels[y * s.width + x] = 0;
            }
        }
    }
    out
}

pub struct CampaignArt {
    kind: campaign::Kind,
    places: Places,
    /// Hannibal's results picture (the Victory or Defeat strip's second
    /// cel).
    victory_picture: Option<Sprite>,
    defeat_picture: Option<Sprite>,
    font: Font,
    small: Font,
    background: Option<Sprite>,
    victory: Option<Sprite>,
    defeat: Option<Sprite>,
    map: Option<Sprite>,
    scroll_ends: Vec<Sprite>,
    shadows: Vec<Sprite>,
    blacked: Vec<Sprite>,
    names: Vec<Sprite>,
    outlines: Vec<Sprite>,
    units: [Vec<Sprite>; 2],
    navy: Option<Sprite>,
    flag: Option<Sprite>,
    numerals: Vec<Sprite>,
    labels: Vec<Sprite>,
    button: Vec<Sprite>,
    /// Caesar's flags: the Pompeian armies' and Pompey's.
    pompeian: Option<Sprite>,
    pompey: Option<Sprite>,
    /// The provinces' names (the art's cel names), for the log.
    pub province_names: Vec<String>,
    dialog: Option<gboh_formats::pe::Dialog>,
    strings: Option<gboh_formats::pe::Resources>,
    darken: [u8; 256],
}

impl CampaignArt {
    pub fn new(data_dir: &Path) -> Self {
        let art = eif(data_dir, "CAMPAIGN.EIF");
        let dialogs = eif(data_dir, "DIALOG.EIF");
        let strings = crate::ui::exe_resources(data_dir);
        match gboh_formats::game::Game::detect(data_dir) {
            Some(gboh_formats::game::Game::Hannibal) => return Self::hannibal(data_dir, art, strings),
            Some(gboh_formats::game::Game::Caesar) => return Self::caesar(data_dir, art, strings),
            _ => {}
        }
        let province_names = art
            .as_ref()
            .and_then(|e| e.strips.iter().find(|s| s.name == "Area Names"))
            .map(|s| s.cels.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        Self {
            kind: campaign::Kind::Alexander,
            places: Places::alexander(),
            victory_picture: None,
            defeat_picture: None,
            font: Font::load(data_dir),
            small: Font::small(data_dir),
            background: first(&dialogs, "Campaign Game"),
            victory: first(&dialogs, "Campaign Victory"),
            defeat: first(&dialogs, "Defeat Screen"),
            map: first(&art, "Map"),
            scroll_ends: cels(&art, "Scroll Ends"),
            shadows: cels(&art, "Shadowed Areas"),
            blacked: cels(&art, "Blacked Out Areas"),
            names: cels(&art, "Area Names"),
            outlines: cels(&art, "Area Outlines"),
            units: [cels(&art, "Units"), cels(&art, "Dark Units")],
            navy: first(&art, "Navy"),
            flag: first(&art, "Flags"),
            numerals: cels(&art, "Numerals"),
            labels: cels(&art, "Battles"),
            button: cels(&eif(data_dir, "DLGBTN.EIF"), "Pushbutton"),
            pompeian: None,
            pompey: None,
            province_names,
            dialog: strings.as_ref().and_then(|r| r.dialog("CAMPAIGNMAP")),
            strings,
            darken: crate::palette_path(data_dir)
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|d| gboh_formats::pal::Palette::parse(&d).ok())
                .and_then(|p| p.remap.first().copied())
                .unwrap_or(std::array::from_fn(|i| i as u8)),
        }
    }

    /// Hannibal's art (all in `CAMPAIGN.EIF`): its own background, no names
    /// strip (the provinces' names from the executable's, its table at
    /// `0x655114`), the Area Blackout cels used only to tell where the mouse
    /// is.
    fn hannibal(data_dir: &Path, art: Option<gboh_formats::eif::Eif>, strings: Option<gboh_formats::pe::Resources>) -> Self {
        let exe = crate::ui::exe_path(data_dir).and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
        // (the table's pointers are set as the game starts: the names follow
        // one another from 0x655500)
        let mut va = 0x655500;
        let province_names = (0..10)
            .map(|_| {
                let name = gboh_formats::pe::string_at(&exe, va).unwrap_or_default();
                va += name.len() as u32 + 1;
                name
            })
            .collect();
        let pictures = |strip: &str| cels(&art, strip);
        let (victory, defeat) = (pictures("Victory"), pictures("Defeat"));
        let font = Font::small(data_dir);
        Self {
            kind: campaign::Kind::Hannibal,
            places: Places::hannibal(),
            victory: victory.first().cloned(),
            defeat: defeat.first().cloned(),
            victory_picture: victory.get(1).cloned(),
            defeat_picture: defeat.get(1).cloned(),
            small: Font::small(data_dir),
            font,
            background: first(&art, "Dialog"),
            map: first(&art, "Map"),
            scroll_ends: cels(&art, "Scroll Ends"),
            // (its shading is stored as RLE effect runs)
            shadows: art.as_ref().and_then(|e| gboh_render::strip_with_shadows(e, "Area Shadows")).unwrap_or_default(),
            blacked: cels(&art, "Area Blackout"),
            names: Vec::new(),
            outlines: cels(&art, "Area Outlines"),
            units: [cels(&art, "Units"), cels(&art, "Dark Units")],
            navy: first(&art, "Navy"),
            flag: first(&art, "Flags"),
            numerals: Vec::new(),
            labels: Vec::new(),
            button: cels(&eif(data_dir, "DLGBTN.EIF"), "Pushbutton"),
            pompeian: None,
            pompey: None,
            province_names,
            dialog: strings.as_ref().and_then(|r| r.dialog("CAMPAIGNMAP")),
            strings,
            darken: crate::palette_path(data_dir)
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|d| gboh_formats::pal::Palette::parse(&d).ok())
                .and_then(|p| p.remap.first().copied())
                .unwrap_or(std::array::from_fn(|i| i as u8)),
        }
    }

    /// Caesar's art (`CAMPAIGN.EIF`): Hannibal's layout less the shading,
    /// units and names; three flags; the provinces' names from the
    /// executable (one after another from `0x637379`).
    fn caesar(data_dir: &Path, art: Option<gboh_formats::eif::Eif>, strings: Option<gboh_formats::pe::Resources>) -> Self {
        let mut c = Self::hannibal(data_dir, art.clone(), strings);
        let exe = crate::ui::exe_path(data_dir).and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
        let mut va = 0x637379;
        c.province_names = (0..13)
            .map(|_| {
                let name = gboh_formats::pe::string_at(&exe, va).unwrap_or_default();
                va += name.len() as u32 + 1;
                name
            })
            .collect();
        let flags = cels(&art, "Flags");
        c.kind = campaign::Kind::Caesar;
        c.places = Places::caesar();
        c.shadows = Vec::new();
        c.units = [Vec::new(), Vec::new()];
        c.flag = flags.first().cloned();
        c.pompeian = flags.get(1).cloned();
        c.pompey = flags.get(2).cloned();
        c
    }

    /// Whose campaign the art is for.
    pub fn kind(&self) -> campaign::Kind {
        self.kind
    }

    /// The map dialog's size and where it lies in a `width` × `height`
    /// window.
    fn origin(&self, width: usize, height: usize) -> (i32, i32) {
        let (w, h) = self.background.as_ref().map_or((600, 450), |b| (b.width as i32, b.height as i32));
        ((width as i32 - w) / 2, (height as i32 - h) / 2)
    }

    /// A map position on the dialog.
    fn on_dialog(&self, pos: (i32, i32), scroll: i32) -> (i32, i32) {
        let v = self.places.view;
        (v.0 + pos.0 - scroll, v.1 + self.places.map_dy + pos.1)
    }

    /// The scroll that centres province `p` in the map's window.
    pub fn centre_on(&self, p: usize) -> i32 {
        let x = match self.places.names.get(p).copied().flatten() {
            Some((x, _)) => x,
            // (Hannibal's: the area's middle)
            None => self.places.area.get(p).map_or(0, |a| a.0) + self.shadows.get(p).or(self.outlines.get(p)).map_or(0, |s| s.width as i32 / 2),
        };
        (x - self.places.view.2 / 2).clamp(0, self.max_scroll())
    }

    /// The furthest the map scrolls.
    pub fn max_scroll(&self) -> i32 {
        self.map.as_ref().map_or(0, |m| (m.width as i32 - self.places.view.2).max(0))
    }

    /// Each garrison icon of `province` (kind, value, cel, dialog place),
    /// infantry row then cavalry 17 pixels down.
    fn icons(&self, c: &Campaign, province: usize, scroll: i32, m_split: &std::collections::HashMap<(usize, usize), [u16; 4]>) -> Vec<(usize, u16, usize, (i32, i32))> {
        let mut out = Vec::new();
        for kind in [campaign::INFANTRY, campaign::CAVALRY] {
            let (mut x, y) = self.on_dialog(self.places.icons[province], scroll);
            let y = y + 17 * kind as i32;
            let counts = m_split.get(&(province, kind)).copied().unwrap_or_else(|| Campaign::icons(c.units[province][kind]));
            for (k, n) in counts.into_iter().enumerate() {
                let cel = VALUE_CEL[k];
                let w = self.units[kind].get(cel).map_or(15, |s| s.width as i32);
                for _ in 0..n {
                    out.push((kind, VALUES[k], cel, (x, y)));
                    x += w + 1;
                }
                if n > 0 {
                    x += 3;
                }
            }
        }
        out
    }

    /// The province whose area is at dialog pixel `(x, y)`.
    fn province_at(&self, scroll: i32, x: i32, y: i32) -> Option<usize> {
        let v = self.places.view;
        if x < v.0 || x >= v.0 + v.2 || y < v.1 || y >= v.1 + v.3 {
            return None;
        }
        (0..self.places.area.len()).find(|&p| {
            let (ox, oy) = self.on_dialog(self.places.area[p], scroll);
            self.blacked.get(p).is_some_and(|s| {
                let (sx, sy) = (x - ox, y - oy);
                sx >= 0 && sy >= 0 && (sx as usize) < s.width && (sy as usize) < s.height && s.pixels[sy as usize * s.width + sx as usize] != 0
            })
        })
    }

    /// The province a drag dropped at `(x, y)` would land on, if it may.
    pub fn drop_target(&self, c: &Campaign, m: &MapState, x: i32, y: i32) -> Option<usize> {
        let p = self.province_at(m.scroll, x, y)?;
        match m.dragging?.0 {
            Dragging::Flag => c.can_march(p).then_some(p),
            Dragging::Unit { from, .. } => (p != from && c.status[p] == campaign::CONQUERED).then_some(p),
            Dragging::Map { .. } => None,
        }
    }

    /// What a press at window pixel `(x, y)` picks up: the flag, an icon,
    /// or the map to pan.
    pub fn press(&self, c: &Campaign, m: &MapState, width: usize, height: usize, x: i32, y: i32) -> Option<Dragging> {
        let (ox, oy) = self.origin(width, height);
        let (x, y) = (x - ox, y - oy);
        let v = self.places.view;
        if x < v.0 || x >= v.0 + v.2 || y < v.1 || y >= v.1 + v.3 {
            return None;
        }
        let inside = |s: Option<&Sprite>, (sx, sy): (i32, i32)| s.is_some_and(|s| x >= sx && y >= sy && x < sx + s.width as i32 && y < sy + s.height as i32);
        if c.current < c.provinces() && inside(self.flag.as_ref(), self.on_dialog(self.places.flag[c.current], m.scroll)) {
            return Some(Dragging::Flag);
        }
        for p in 0..c.provinces() {
            for (kind, value, cel, at) in self.icons(c, p, m.scroll, &m.split) {
                if inside(self.units[kind].get(cel), at) {
                    return Some(Dragging::Unit { from: p, kind, value });
                }
            }
        }
        Some(Dragging::Map { scroll: m.scroll, x })
    }

    /// A double-click at window pixel `(x, y)` on a garrison icon
    /// (`FUN_005951c3`): five Ones merge into a Five; a larger icon splits
    /// into the next smaller (a Five into five Ones, a Ten into two Fives, a
    /// Fifty into five Tens) if there are none of those. Returns whether one
    /// did.
    pub fn split(&self, c: &Campaign, m: &mut MapState, width: usize, height: usize, x: i32, y: i32) -> bool {
        let (ox, oy) = self.origin(width, height);
        let (x, y) = (x - ox, y - oy);
        for p in 0..c.provinces() {
            for (kind, value, cel, at) in self.icons(c, p, m.scroll, &m.split) {
                let hit = self.units[kind].get(cel).is_some_and(|s| x >= at.0 && y >= at.1 && x < at.0 + s.width as i32 && y < at.1 + s.height as i32);
                if !hit {
                    continue;
                }
                let mut counts = m.split.get(&(p, kind)).copied().unwrap_or_else(|| Campaign::icons(c.units[p][kind]));
                let d = VALUES.iter().position(|&v| v == value).unwrap_or(3);
                if d == 3 {
                    if counts[3] < 5 {
                        return false;
                    }
                    counts[3] -= 5;
                    counts[2] += 1;
                } else {
                    if counts[d + 1] != 0 {
                        return false;
                    }
                    counts[d] -= 1;
                    counts[d + 1] += VALUES[d] / VALUES[d + 1];
                }
                m.split.insert((p, kind), counts);
                return true;
            }
        }
        false
    }

    /// Window pixels to dialog pixels.
    pub fn to_dialog(&self, width: usize, height: usize, x: i32, y: i32) -> (i32, i32) {
        let (ox, oy) = self.origin(width, height);
        (x - ox, y - oy)
    }

    /// The button at window pixel `(x, y)`, by id (10 Battle, 11 Subjugate,
    /// 2 Cancel, 9 Help).
    pub fn button_at(&self, width: usize, height: usize, x: i32, y: i32) -> Option<u16> {
        let (x, y) = self.to_dialog(width, height, x, y);
        self.dialog.as_ref()?.controls.iter().find_map(|c| {
            let (bx, by, bw, bh) = du(c.rect);
            (x >= bx && x < bx + bw && y >= by && y < by + bh).then_some(c.id)
        })
    }

    fn button(&self, canvas: &mut Canvas, (bx, by, bw, bh): (i32, i32, i32, i32), caption: &str, on: bool, pressed: Option<(i32, i32)>) {
        let down = (on && pressed.is_some_and(|(px, py)| px >= bx && px < bx + bw && py >= by && py < by + bh)) as i32;
        if let Some(b) = self.button.get(down as usize).or(self.button.first()) {
            canvas.blit(b, bx + (bw - b.width as i32) / 2, by + (bh - b.height as i32) / 2);
        }
        let caption: String = caption.chars().filter(|&ch| ch != '&').collect();
        // a disabled button's caption is engraved (light under dark)
        if on {
            self.font.draw(canvas, &caption, bx + (bw - self.font.width(&caption)) / 2 + down, by + (bh - 11) / 2 + down);
        } else {
            let (x, y) = (bx + (bw - self.font.width(&caption)) / 2, by + (bh - 11) / 2);
            self.font.draw_in(canvas, &caption, x + 1, y + 1, 255);
            self.font.draw_in(canvas, &caption, x, y, 248);
        }
    }

    /// The map screen over a clear `width` × `height` overlay.
    pub fn draw_map(&self, c: &Campaign, m: &MapState, width: usize, height: usize) -> Canvas {
        let mut canvas = Canvas::new(width, height, 0);
        let (ox, oy) = self.origin(width, height);
        let (dw, dh) = self.background.as_ref().map_or((600, 450), |b| (b.width, b.height));
        let mut d = Canvas::new(dw, dh, 0);
        if let Some(b) = &self.background {
            d.blit(b, 0, 0);
        }
        // the map, through its window
        let pl = &self.places;
        let v = pl.view;
        let mut view = Canvas::new(v.2 as usize, v.3 as usize, 0);
        let shift = |(x, y): (i32, i32)| (x - v.0, y - v.1);
        if let Some(map) = &self.map {
            view.blit(map, -m.scroll, pl.map_dy);
        }
        let hannibal = self.kind == campaign::Kind::Hannibal;
        for p in (0..c.provinces().min(pl.area.len())).rev() {
            let at = shift(self.on_dialog(pl.area[p], m.scroll));
            // (Hannibal's hidden provinces aren't blacked out: shaded as any)
            if c.status[p] == campaign::HIDDEN && !hannibal {
                if let Some(s) = self.blacked.get(p) {
                    view.blit(s, at.0, at.1);
                }
            } else if let Some(s) = self.shadows.get(p) {
                match m.changing().filter(|&(q, _, _)| q == p) {
                    // dissolving from the old shading to the new, cell by cell
                    Some((_, old, t)) => {
                        let (new_part, old_part) = (cells(s, t, true, pl.cell), cells(s, t, false, pl.cell));
                        for _ in 0..shade_times(c.subjugation[p]) {
                            view.blit_shadowed(&new_part, at.0, at.1, &self.darken);
                        }
                        for _ in 0..shade_times(old) {
                            view.blit_shadowed(&old_part, at.0, at.1, &self.darken);
                        }
                    }
                    None => {
                        for _ in 0..shade_times(c.subjugation[p]) {
                            view.blit_shadowed(s, at.0, at.1, &self.darken);
                        }
                    }
                }
            }
            if m.hover == Some(p) {
                if let Some(s) = self.outlines.get(p) {
                    view.blit(s, at.0, at.1);
                }
                if let (Some(s), Some(at)) = (self.names.get(p), pl.names[p]) {
                    let n = shift(self.on_dialog(at, m.scroll));
                    view.blit(s, n.0, n.1);
                }
            }
            // the garrison's total, then its icons
            let total = c.units[p][0] + c.units[p][1];
            if total > 0
                && let Some(at) = pl.numerals[p]
            {
                let (mut x, y) = shift(self.on_dialog(at, m.scroll));
                for ch in total.min(999).to_string().chars() {
                    if let Some(s) = ch.to_digit(10).and_then(|d| self.numerals.get(d as usize)) {
                        view.blit(s, x, y);
                        x += s.width as i32 + 1;
                    }
                }
            }
            for (kind, _, cel, at) in self.icons(c, p, m.scroll, &m.split) {
                if let Some(s) = self.units[kind].get(cel) {
                    let at = shift(at);
                    view.blit(s, at.0, at.1);
                }
            }
            if c.current == p
                && !matches!(m.dragging, Some((Dragging::Flag, _)))
                && let Some(s) = &self.flag
            {
                let at = shift(self.on_dialog(pl.flag[p], m.scroll));
                view.blit(s, at.0, at.1);
            }
        }
        for (b, &(x, y, w, h)) in LABELS.iter().enumerate().filter(|_| pl.labels) {
            if c.status[BATTLE_PROVINCE[b]] == campaign::HIDDEN {
                continue;
            }
            let (lx, ly) = shift(self.on_dialog((x, y), m.scroll));
            if let Some(s) = self.labels.get(b) {
                view.blit(s, lx + 2, ly + 2);
            }
            // the battles waiting to be fought, framed
            if c.pending.contains(&b) {
                frame(&mut view, (lx, ly, w, h), 255);
            }
        }
        // (Caesar's: the Pompeian fleets not yet sailed)
        let caesar = self.kind == campaign::Kind::Caesar;
        if caesar {
            // the armies' flags beside Caesar's, Pompey's over his army's
            let dx = self.flag.as_ref().map_or(31, |f| f.width as i32 * 5 / 4);
            for (k, at) in c.armies.iter().enumerate() {
                let (Some(p), Some(s)) = (at, if k == 4 { &self.pompey } else { &self.pompeian }) else { continue };
                let at = shift(self.on_dialog(pl.flag[*p], m.scroll));
                view.blit(s, at.0 + dx, at.1);
            }
        }
        for &(k, pos) in &pl.navy {
            if (if caesar { c.fleets[k] } else { c.navy[k] })
                && let Some(s) = &self.navy
            {
                let at = shift(self.on_dialog(pos, m.scroll));
                view.blit(s, at.0, at.1);
            }
        }
        d.blit(&Sprite { width: view.width, height: view.height, pixels: view.pixels }, v.0, v.1);
        // the rods, their knobs over the background's
        if let Some(s) = self.scroll_ends.get(((m.scroll / 5) % 3) as usize) {
            d.blit_shadowed(s, SCROLL_ENDS.0, SCROLL_ENDS.1, &self.darken);
        }
        // what is being dragged, kept on the map
        if let Some((what, (x, y))) = m.dragging {
            let s = match what {
                Dragging::Flag => self.flag.as_ref(),
                Dragging::Unit { kind, value, .. } => VALUES.iter().position(|&v| v == value).and_then(|k| self.units[kind].get(VALUE_CEL[k])),
                Dragging::Map { .. } => None,
            };
            if let Some(s) = s {
                let (w, h) = (s.width as i32, s.height as i32);
                let (top, bottom) = match self.kind {
                    campaign::Kind::Hannibal => (8, 359),
                    campaign::Kind::Caesar => (15, 366),
                    campaign::Kind::Alexander => (17, 368),
                };
                d.blit(s, (x - w / 2).clamp(45, 554 - w), (y - h / 2).clamp(top, bottom - h));
            }
        }
        // the log: its last lines
        fill(&mut d, LOG, 255);
        frame(&mut d, LOG, 248);
        let rows = ((LOG.3 - 4) / 12) as usize;
        for (k, line) in m.log.iter().rev().take(rows).rev().enumerate() {
            self.small.draw(&mut d, line, LOG.0 + 3, LOG.1 + 2 + 12 * k as i32);
        }
        if let Some(dl) = &self.dialog {
            for ctl in &dl.controls {
                let on = match ctl.id {
                    10 => m.battle,
                    11 => m.subjugate,
                    _ => true,
                };
                let pressed = m.pressed.map(|(x, y)| (x - ox, y - oy));
                self.button(&mut d, du(ctl.rect), &ctl.text, on, pressed);
            }
        }
        canvas.blit(&Sprite { width: d.width, height: d.height, pixels: d.pixels }, ox, oy);
        canvas
    }

    /// The results screen (`FUN_0052d629`, `VictoryDlg`): the victory or
    /// defeat picture, with the rounds played, provinces conquered and
    /// Alexander's rating (strings 44–46 and 38–43).
    pub fn draw_results(&self, r: &campaign::Results, width: usize, height: usize) -> Canvas {
        let mut canvas = Canvas::new(width, height, 0);
        let art = if r.victory { &self.victory } else { &self.defeat };
        let (w, h) = art.as_ref().map_or((600, 469), |a| (a.width as i32, a.height as i32));
        let (ox, oy) = ((width as i32 - w) / 2, (height as i32 - h) / 2);
        if let Some(a) = art {
            canvas.blit(a, ox, oy);
        }
        let string = |id: u16| self.strings.as_ref().and_then(|s| s.string(id)).unwrap_or_default();
        // Hannibal's (`EndOfCampaignDlg`): the Carthaginian's picture, and
        // four rows down the left: rounds, provinces conquered and wholly
        // subjugated, the rating
        if self.kind != campaign::Kind::Alexander {
            let picture = if r.victory { &self.victory_picture } else { &self.defeat_picture };
            if let Some(p) = picture {
                canvas.blit(p, ox + 174, oy + 89);
            }
            // (Caesar's: rounds and the rating only, 🟡 placed as Hannibal's)
            if self.kind == campaign::Kind::Caesar {
                for (id, y, value) in [(44, 141, r.rounds.to_string()), (136, 205, r.rating_string.map(string).unwrap_or_default())] {
                    self.font.draw(&mut canvas, &string(id), ox + 15, oy + y);
                    self.font.draw(&mut canvas, &value, ox + 15, oy + y + 15);
                }
                return canvas;
            }
            let rows = [
                (44, 141, r.rounds.to_string()),
                (45, 205, r.conquered.to_string()),
                (166, 269, r.subjugated.to_string()),
                (136, 335, r.rating_string.map(string).unwrap_or_default()),
            ];
            for (id, y, value) in rows {
                self.font.draw(&mut canvas, &string(id), ox + 15, oy + y);
                self.font.draw(&mut canvas, &value, ox + 15, oy + y + 15);
            }
            return canvas;
        }
        let rows = [
            (44, 186, r.rounds.to_string()),
            (45, 272, r.conquered.to_string()),
            (46, 380, string(38 + r.rating as u16)),
        ];
        // the rects (x 411) are pixels of the picture: its marble panel
        for (id, y, value) in rows {
            self.font.draw(&mut canvas, &string(id), ox + 411, oy + y);
            self.font.draw(&mut canvas, &value, ox + 411, oy + y + 15);
        }
        canvas
    }
}

/// Dialog units to pixels.
fn du((x, y, w, h): (i16, i16, i16, i16)) -> (i32, i32, i32, i32) {
    (x as i32 * 7 / 4, y as i32 * 13 / 8, w as i32 * 7 / 4, h as i32 * 13 / 8)
}

fn fill(canvas: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), c: u8) {
    for yy in y.max(0)..(y + h).min(canvas.height as i32) {
        for xx in x.max(0)..(x + w).min(canvas.width as i32) {
            canvas.pixels[yy as usize * canvas.width + xx as usize] = c;
        }
    }
}

fn frame(canvas: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), c: u8) {
    fill(canvas, (x, y, w, 1), c);
    fill(canvas, (x, y + h - 1, w, 1), c);
    fill(canvas, (x, y, 1, h), c);
    fill(canvas, (x + w - 1, y, 1, h), c);
}

/// The log's line for a map event, in the rewrite's words (the original's
/// are in its data); `names` are the provinces'.
pub fn describe(e: &campaign::Event, names: &[String], kind: campaign::Kind) -> String {
    let name = |p: usize| names.get(p).cloned().unwrap_or_else(|| format!("Province {p}"));
    let (goal, power) = match kind {
        campaign::Kind::Alexander => ("conquer the world", "Macedon"),
        campaign::Kind::Hannibal => ("subdue Italy", "Carthage"),
        campaign::Kind::Caesar => ("win the civil war", "Rome"),
    };
    let kind = |k: usize| if k == campaign::INFANTRY { "infantry" } else { "cavalry" };
    match e {
        campaign::Event::Round(r) => {
            let left = campaign::ROUNDS + 1 - r;
            format!("--- Round {r} --- {left} round{} left to {goal}.", if left == 1 { "" } else { "s" })
        }
        campaign::Event::Final => "--- The results of the last round ---".to_string(),
        campaign::Event::TurnsLeft(_) => String::new(),
        campaign::Event::Defeated { province } => format!("Caesar beats the Pompeian forces in {}.", name(*province)),
        campaign::Event::Moved { province } => format!("Caesar marches into {}.", name(*province)),
        // (the game's own words, read by the caller)
        campaign::Event::Message(_) => String::new(),
        campaign::Event::Subjugation { province, level, rose: true } => {
            format!("{} is now {} subjugated.", name(*province), if *level == campaign::TOTAL { "wholly" } else { "partly" })
        }
        campaign::Event::Subjugation { province, level, rose: false } => {
            format!("{} is {} free of {power}.", name(*province), if *level == campaign::FREE { "wholly" } else { "partly" })
        }
        campaign::Event::Reinforcements { kind: k, count: 0 } => format!("No {} reinforcements this round.", kind(*k)),
        campaign::Event::Reinforcements { kind: k, count } => format!("{count} {} reinforcement{} gather in {}.", kind(*k), if *count == 1 { "" } else { "s" }, name(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hannibal's map and results; runs only with `GBOH_HANNIBAL` (and
    /// writes them to `GBOH_SHOTS`, if set).
    #[test]
    fn draws_hannibal_campaign() {
        let Ok(dir) = std::env::var("GBOH_HANNIBAL") else { return };
        let dir = Path::new(&dir);
        let art = CampaignArt::new(dir);
        assert_eq!(art.kind(), campaign::Kind::Hannibal);
        assert_eq!(art.province_names.len(), 10);
        assert!(art.province_names.iter().all(|n| !n.is_empty()), "{:?}", art.province_names);
        let mut c = Campaign::new_hannibal(3);
        c.after_battle(true);
        let events = c.opening();
        let m = MapState { scroll: art.centre_on(4), subjugate: true, log: events.iter().map(|e| describe(e, &art.province_names, c.kind)).collect(), ..Default::default() };
        let canvas = art.draw_map(&c, &m, 640, 480);
        // the flag over Gaul can be picked up and dropped on Cisalpine Gaul
        let (ox, oy) = art.origin(640, 480);
        let (fx, fy) = art.on_dialog(art.places.flag[4], m.scroll);
        assert_eq!(art.press(&c, &m, 640, 480, ox + fx + 5, oy + fy + 5), Some(Dragging::Flag));
        // Carthage's 12 foot (a Ten, two Ones): the Ten splits into two
        // Fives; a Five won't split while there are Ones
        let mut m = m;
        assert_eq!(c.units[0][campaign::INFANTRY], 12);
        let at = |m: &MapState, value: u16| art.icons(&c, 0, m.scroll, &m.split).into_iter().find(|i| i.0 == campaign::INFANTRY && i.1 == value).map(|i| (ox + i.3.0 + 2, oy + i.3.1 + 2));
        let (x, y) = at(&m, 10).unwrap();
        assert!(art.split(&c, &mut m, 640, 480, x, y));
        assert_eq!(m.split[&(0, campaign::INFANTRY)], [0, 0, 2, 2]);
        let (x, y) = at(&m, 5).unwrap();
        assert!(!art.split(&c, &mut m, 640, 480, x, y));
        let results = art.draw_results(&c.results(), 640, 480);
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(crate::palette_path(dir).unwrap()).unwrap()).unwrap();
            crate::ui::write_ppm(&canvas, &pal, &Path::new(&out).join("han_campaign.ppm")).unwrap();
            crate::ui::write_ppm(&results, &pal, &Path::new(&out).join("han_campaign_results.ppm")).unwrap();
        }
    }

    /// Caesar's map and results; runs only with `GBOH_CAESAR` (and writes
    /// them to `GBOH_SHOTS`, if set).
    #[test]
    fn draws_caesar_campaign() {
        let Ok(dir) = std::env::var("GBOH_CAESAR") else { return };
        let dir = Path::new(&dir);
        let art = CampaignArt::new(dir);
        assert_eq!(art.kind(), campaign::Kind::Caesar);
        assert_eq!(art.province_names.len(), 13);
        assert_eq!(art.province_names[3], "Roma");
        let mut c = Campaign::new_caesar(3);
        c.after_battle(true);
        let log: Vec<String> = c.log.iter().map(|e| describe(e, &art.province_names, c.kind)).filter(|l| !l.is_empty()).collect();
        let m = MapState { scroll: art.centre_on(c.current), battle: true, log, ..Default::default() };
        let canvas = art.draw_map(&c, &m, 640, 480);
        // Caesar's flag in Samnium can be picked up, and dropped on Roma
        let (ox, oy) = art.origin(640, 480);
        let (fx, fy) = art.on_dialog(art.places.flag[2], m.scroll);
        assert_eq!(art.press(&c, &m, 640, 480, ox + fx + 5, oy + fy + 5), Some(Dragging::Flag));
        let results = art.draw_results(&c.results(), 640, 480);
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(crate::palette_path(dir).unwrap()).unwrap()).unwrap();
            crate::ui::write_ppm(&canvas, &pal, &Path::new(&out).join("cae_campaign.ppm")).unwrap();
            crate::ui::write_ppm(&results, &pal, &Path::new(&out).join("cae_campaign_results.ppm")).unwrap();
        }
    }

    /// Draws the campaign map and results; runs only with `GBOH_DATA` set
    /// (and writes them to `GBOH_SHOTS`, if set).
    #[test]
    fn draws_the_campaign() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = Path::new(&dir);
        let art = CampaignArt::new(dir);
        assert_eq!(art.province_names.len(), 8);
        let mut c = Campaign::new(3);
        c.after_battle(true);
        let events = c.opening();
        c.end_round();
        c.after_battle(true);
        c.after_battle(true);
        c.opening();
        let m = MapState { scroll: 0, battle: false, subjugate: true, log: events.iter().map(|e| describe(e, &art.province_names, campaign::Kind::Alexander)).collect(), ..Default::default() };
        let canvas = art.draw_map(&c, &m, 640, 480);
        // the flag can be picked up and dropped on Anatolia
        let (ox, oy) = art.origin(640, 480);
        let (fx, fy) = art.on_dialog(PLACES[0][4], 0);
        assert_eq!(art.press(&c, &m, 640, 480, ox + fx + 5, oy + fy + 5), Some(Dragging::Flag));
        let dragging = MapState { dragging: Some((Dragging::Flag, (0, 0))), ..m.clone() };
        let (ax, ay) = art.on_dialog(PLACES[1][1], 0);
        assert_eq!(art.drop_target(&c, &dragging, ax + 10, ay + 5), Some(1));
        assert_eq!(art.button_at(640, 480, ox + 114 * 7 / 4 + 5, oy + 261 * 13 / 8 + 5), Some(11));
        // halfway through a change: part old shading, part new
        let mut changing = m.clone();
        changing.changes.push_back((1, campaign::TOTAL));
        changing.change_start = Some(std::time::Instant::now() - std::time::Duration::from_secs_f32(1.0));
        let (_, _, t) = changing.changing().unwrap();
        assert!(t > 0.3 && t < 0.7, "{t}");
        let mid = art.draw_map(&c, &changing, 640, 480);
        assert_ne!(mid.pixels, canvas.pixels);
        let results = art.draw_results(&c.results(), 640, 480);
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(dir.join("GAME.PAL")).unwrap()).unwrap();
            crate::ui::write_ppm(&canvas, &pal, &Path::new(&out).join("campaign.ppm")).unwrap();
            crate::ui::write_ppm(&results, &pal, &Path::new(&out).join("campaign_results.ppm")).unwrap();
        }
    }
}
