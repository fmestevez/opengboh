//! The rewrite's own screens, drawn without any game's art (they come
//! before any is imported): the content manager (import each title's data,
//! or import it again), the settings (whose look the menus take) and the
//! game selector (every imported title's battles and campaigns together).
//! An antique look of the rewrite's own: a parchment panel framed in dark
//! brown on carved stone, lettered in DejaVu Serif (the `dejavu` crate,
//! drawn with `fontdue`), stone buttons; in a palette of its own, with a
//! ramp of shades for each colour of lettering.

use crate::library::Title;
use gboh_formats::pal::Palette;
use gboh_render::Canvas;

/// The palette: plain colours, then ramps of 8 shades from a background to
/// a lettering colour.
const BLACK: u8 = 0;
const PARCHMENT: u8 = 1;
const FRAME: u8 = 2;
const LIGHT: u8 = 3;
const SHADOW: u8 = 4;
const FACE: u8 = 5;
const FACE_DOWN: u8 = 6;
const RULE: u8 = 7;
/// Stone: 16 shades for the background's grain.
const STONE: u8 = 16;
/// Lettering ramps: ink, faded ink, red and green on parchment; ink and
/// faded ink on a button.
const INK: u8 = 40;
const DIM: u8 = 48;
const RED: u8 = 56;
const GREEN: u8 = 64;
const BUTTON_INK: u8 = 72;
const BUTTON_DIM: u8 = 80;
const RAMP: u8 = 8;

const PARCHMENT_RGB: [u8; 3] = [232, 216, 176];
const FACE_RGB: [u8; 3] = [196, 186, 164];

/// The screens' palette.
pub fn palette() -> Palette {
    let mut colors = [[0u8; 3]; 256];
    let plain = [
        (BLACK, [0, 0, 0]),
        (PARCHMENT, PARCHMENT_RGB),
        (FRAME, [76, 50, 30]),
        (LIGHT, [240, 234, 216]),
        (SHADOW, [92, 82, 70]),
        (FACE, FACE_RGB),
        (FACE_DOWN, [176, 166, 144]),
        (RULE, [170, 140, 100]),
    ];
    for (i, c) in plain {
        colors[i as usize] = c;
    }
    for k in 0..16 {
        let v: u8 = 120 + k * 6;
        colors[(STONE + k) as usize] = [v, v - 6, v - 16];
    }
    let ramp = |colors: &mut [[u8; 3]; 256], base: u8, from: [u8; 3], to: [u8; 3]| {
        for k in 0..RAMP {
            let t = (k + 1) as f32 / RAMP as f32;
            colors[(base + k) as usize] = std::array::from_fn(|c| (from[c] as f32 + (to[c] as f32 - from[c] as f32) * t) as u8);
        }
    };
    ramp(&mut colors, INK, PARCHMENT_RGB, [48, 30, 18]);
    ramp(&mut colors, DIM, PARCHMENT_RGB, [128, 106, 80]);
    ramp(&mut colors, RED, PARCHMENT_RGB, [128, 28, 18]);
    ramp(&mut colors, GREEN, PARCHMENT_RGB, [36, 96, 40]);
    ramp(&mut colors, BUTTON_INK, FACE_RGB, [40, 28, 18]);
    ramp(&mut colors, BUTTON_DIM, FACE_RGB, [130, 120, 104]);
    Palette { name: "opengboh".into(), colors, remap: Vec::new() }
}

/// The two faces used.
fn fonts() -> &'static (fontdue::Font, fontdue::Font) {
    static FONTS: std::sync::OnceLock<(fontdue::Font, fontdue::Font)> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| {
        let load = |d: &'static [u8]| fontdue::Font::from_bytes(d, fontdue::FontSettings::default()).expect("DejaVu");
        (load(dejavu::serif::regular()), load(dejavu::serif::bold()))
    })
}

/// Sizes of lettering.
const BODY: f32 = 13.0;
const HEADING: f32 = 15.0;
const TITLE: f32 = 26.0;

fn fill(c: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), colour: u8) {
    for py in y.max(0)..(y + h).min(c.height as i32) {
        for px in x.max(0)..(x + w).min(c.width as i32) {
            c.pixels[py as usize * c.width + px as usize] = colour;
        }
    }
}

fn frame(c: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), colour: u8) {
    fill(c, (x, y, w, 1), colour);
    fill(c, (x, y + h - 1, w, 1), colour);
    fill(c, (x, y, 1, h), colour);
    fill(c, (x + w - 1, y, 1, h), colour);
}

/// A raised (or, `down`, sunken) bevel around `r`.
fn bevel(c: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), down: bool) {
    let (tl, br) = if down { (SHADOW, LIGHT) } else { (LIGHT, SHADOW) };
    fill(c, (x, y, w, 1), tl);
    fill(c, (x, y, 1, h), tl);
    fill(c, (x, y + h - 1, w, 1), br);
    fill(c, (x + w - 1, y, 1, h), br);
}

/// The width of `text` at `size`.
fn width(text: &str, size: f32, bold: bool) -> i32 {
    let f = if bold { &fonts().1 } else { &fonts().0 };
    text.chars().map(|ch| f.metrics(ch, size).advance_width).sum::<f32>().round() as i32
}

/// Letter `text` with its baseline at `y`, in lettering ramp `ramp`.
fn text(c: &mut Canvas, s: &str, x: i32, y: i32, size: f32, bold: bool, ramp: u8) {
    let f = if bold { &fonts().1 } else { &fonts().0 };
    let mut pen = x as f32;
    for ch in s.chars() {
        let (m, cov) = f.rasterize(ch, size);
        let (gx, gy) = (pen.round() as i32 + m.xmin, y - m.height as i32 - m.ymin);
        for row in 0..m.height {
            for col in 0..m.width {
                let a = cov[row * m.width + col] as u32;
                if a < 24 {
                    continue;
                }
                let (px, py) = (gx + col as i32, gy + row as i32);
                if px >= 0 && py >= 0 && (px as usize) < c.width && (py as usize) < c.height {
                    c.pixels[py as usize * c.width + px as usize] = ramp + ((a * RAMP as u32 / 256) as u8).min(RAMP - 1);
                }
            }
        }
        pen += m.advance_width;
    }
}

/// The background: stone, grained (a deterministic value noise).
fn stone(c: &mut Canvas) {
    let hash = |x: i32, y: i32| -> f32 {
        let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263);
        h = (h ^ (h >> 13)).wrapping_mul(1274126177);
        (h >> 16) as f32 / 65535.0
    };
    let noise = |x: f32, y: f32, cell: f32| {
        let (gx, gy) = ((x / cell).floor(), (y / cell).floor());
        let (fx, fy) = (x / cell - gx, y / cell - gy);
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let (a, b) = (hash(gx as i32, gy as i32), hash(gx as i32 + 1, gy as i32));
        let (cc, d) = (hash(gx as i32, gy as i32 + 1), hash(gx as i32 + 1, gy as i32 + 1));
        (a + (b - a) * sx) * (1.0 - sy) + (cc + (d - cc) * sx) * sy
    };
    for y in 0..c.height {
        for x in 0..c.width {
            let v = 0.55 * noise(x as f32, y as f32, 48.0) + 0.3 * noise(x as f32, y as f32, 12.0) + 0.15 * hash(x as i32, y as i32);
            c.pixels[y * c.width + x] = STONE + ((v * 15.0) as u8).min(15);
        }
    }
}

/// A tick in ramp `ramp`'s darkest shade, `size` pixels square.
fn tick(c: &mut Canvas, x: i32, y: i32, size: i32, ramp: u8) {
    let colour = ramp + RAMP - 1;
    for i in 0..size / 3 {
        fill(c, (x + i, y + size / 2 + i, 2, 2), colour);
    }
    for i in 0..size * 2 / 3 {
        fill(c, (x + size / 3 + i, y + size / 2 + size / 3 - i, 2, 2), colour);
    }
}

/// A clickable button: its id and rectangle.
pub type Button = (u16, (i32, i32, i32, i32));

/// The button ids.
pub const IMPORT: u16 = 100; // + title index
pub const CONTINUE: u16 = 1;
pub const QUIT: u16 = 2;
pub const BACK: u16 = 3;
pub const THEME: u16 = 200; // + title index
pub const RULES: u16 = 8;
pub const OPTION: u16 = 300; // + option index
pub const CONTENT: u16 = 4;
pub const ENTRY: u16 = 1000; // + entry index
pub const SCROLL_UP: u16 = 5;
pub const SCROLL_DOWN: u16 = 6;
/// The scroll bar's track, a button per place the list can scroll to.
pub const SCROLL_AT: u16 = 500; // + scroll position
pub const PREFERENCES: u16 = 7;

/// One line of the game selector: a campaign or a battle of a title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub title: Title,
    /// The scenario file, or `None` for the title's campaign.
    pub scenario: Option<String>,
    pub name: String,
    /// What tells it from another of the same name: its variant file's
    /// description, "standard" for the original of a battle with variants,
    /// else its file name.
    pub detail: Option<String>,
}

/// The selector's list rows that fit a `h` tall window.
pub fn selector_rows(h: i32) -> usize {
    let panel_h = (h - 32).min(SELECTOR_HEIGHT);
    ((panel_h - 84 - 76) / ROW).max(1) as usize
}

/// The selector's panel height (at most) and row pitch.
const SELECTOR_HEIGHT: i32 = 560;
const ROW: i32 = 22;

/// Which of the rewrite's screens is up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Content,
    Settings,
    /// The rule options.
    Rules,
    Selector {
        entries: Vec<Entry>,
        scroll: usize,
    },
}

/// What the screens show besides their layout.
pub struct View<'a> {
    pub imported: &'a [Title],
    pub theme: Option<Title>,
    /// The rule options set.
    pub options: gboh_game::options::Options,
    /// A line of news (an import's progress, or what went wrong), and
    /// whether it is bad news.
    pub status: (String, bool),
    /// An import under way, which buttons wait on.
    pub busy: bool,
    pub pressed: Option<(i32, i32)>,
}

/// A screen drawn on a `w` × `h` canvas, and its buttons.
pub fn draw(screen: &Screen, v: &View, w: usize, h: usize) -> (Canvas, Vec<Button>) {
    let mut c = Canvas::new(w, h, PARCHMENT);
    stone(&mut c);
    let mut buttons = Vec::new();
    let (w, h) = (w as i32, h as i32);
    // the parchment panel, framed twice in dark brown
    let panel_w = (w - 40).min(if *screen == Screen::Rules { 720 } else { 520 });
    // as tall as the screen's contents (the selector: the whole window),
    // in the middle of the window
    let panel_h = match screen {
        Screen::Content => 370,
        Screen::Settings => 300,
        Screen::Rules => 130 + 28 * gboh_game::options::ALL.len() as i32 + 40,
        // the heading, a row per entry and the buttons
        // a fixed height: the list scrolls
        Screen::Selector { .. } => SELECTOR_HEIGHT,
    }
    .min(h - 32);
    let panel = ((w - panel_w) / 2, ((h - panel_h) / 2).max(16), panel_w, panel_h);
    fill(&mut c, panel, PARCHMENT);
    frame(&mut c, panel, FRAME);
    frame(&mut c, (panel.0 + 3, panel.1 + 3, panel.2 - 6, panel.3 - 6), FRAME);
    let left = panel.0 + 22;
    let inner = panel.2 - 44;
    let heading = |c: &mut Canvas, sub: &str| {
        let t = "opengboh";
        text(c, t, panel.0 + (panel.2 - width(t, TITLE, true)) / 2, panel.1 + 40, TITLE, true, RED);
        text(c, sub, panel.0 + (panel.2 - width(sub, HEADING, false)) / 2, panel.1 + 62, HEADING, false, DIM);
        fill(c, (left, panel.1 + 72, inner, 1), RULE);
    };
    let button = |c: &mut Canvas, buttons: &mut Vec<Button>, id: u16, label: &str, x: i32, y: i32, enabled: bool| {
        let r = (x, y, width(label, BODY, true) + 24, 22);
        let down = enabled && v.pressed.is_some_and(|(px, py)| px >= r.0 && px < r.0 + r.2 && py >= r.1 && py < r.1 + r.3);
        fill(c, r, if down { FACE_DOWN } else { FACE });
        frame(c, (r.0 - 1, r.1 - 1, r.2 + 2, r.3 + 2), FRAME);
        bevel(c, r, down);
        let o = down as i32;
        text(c, label, x + 12 + o, y + 16 + o, BODY, true, if enabled { BUTTON_INK } else { BUTTON_DIM });
        if enabled {
            buttons.push((id, r));
        }
        r.0 + r.2
    };
    let status_y = panel.1 + panel.3 - 16;
    match screen {
        Screen::Content => {
            heading(&mut c, "Content manager");
            text(&mut c, "Import the games you own: choose the folder where each is installed.", left, panel.1 + 96, BODY, false, INK);
            for (k, &t) in Title::ALL.iter().enumerate() {
                let y = panel.1 + 112 + k as i32 * 58;
                let done = v.imported.contains(&t);
                if done {
                    tick(&mut c, left + 2, y + 6, 16, GREEN);
                } else {
                    frame(&mut c, (left + 3, y + 9, 13, 13), FRAME);
                }
                text(&mut c, t.name(), left + 28, y + 20, HEADING, true, INK);
                let note = match (done, t.playable()) {
                    (true, true) => "Imported",
                    (true, false) => "Imported; its battles are not playable yet",
                    (false, true) => "Not imported",
                    (false, false) => "Not imported; its battles are not playable yet",
                };
                text(&mut c, note, left + 28, y + 38, BODY, false, if done { GREEN } else { DIM });
                let label = if done { "Re-import" } else { "Import..." };
                let bx = left + inner - width(label, BODY, true) - 24;
                button(&mut c, &mut buttons, IMPORT + k as u16, label, bx, y + 8, !v.busy);
                fill(&mut c, (left, y + 50, inner, 1), RULE);
            }
            let y = panel.1 + 112 + 3 * 58 + 10;
            let x = button(&mut c, &mut buttons, CONTINUE, "Continue", left, y, !v.imported.is_empty() && !v.busy);
            button(&mut c, &mut buttons, QUIT, "Quit", x + 12, y, true);
        }
        Screen::Settings => {
            heading(&mut c, "Settings");
            text(&mut c, "Theme: whose intro, start screen and art the menus take", left, panel.1 + 96, BODY, false, INK);
            for (k, &t) in Title::ALL.iter().enumerate() {
                let y = panel.1 + 108 + k as i32 * 30;
                let have = v.imported.contains(&t);
                let on = v.theme == Some(t);
                let r = (left, y, inner, 26);
                if on {
                    fill(&mut c, r, FACE);
                    bevel(&mut c, r, true);
                    tick(&mut c, left + 6, y + 4, 14, BUTTON_INK);
                }
                let ramp = match (on, have) {
                    (true, _) => BUTTON_INK,
                    (false, true) => INK,
                    (false, false) => DIM,
                };
                text(&mut c, t.name(), left + 28, y + 18, BODY, true, ramp);
                if !have {
                    text(&mut c, "not imported", left + inner - width("not imported", BODY, false) - 6, y + 18, BODY, false, DIM);
                } else {
                    buttons.push((THEME + k as u16, r));
                }
            }
            let y = panel.1 + 108 + 3 * 30 + 16;
            let x = button(&mut c, &mut buttons, BACK, "Back", left, y, true);
            let x = button(&mut c, &mut buttons, PREFERENCES, "Game preferences", x + 12, y, !v.imported.is_empty());
            let x = button(&mut c, &mut buttons, RULES, "Rules", x + 12, y, true);
            button(&mut c, &mut buttons, CONTENT, "Content manager", x + 12, y, true);
        }
        Screen::Rules => {
            heading(&mut c, "Rules");
            text(&mut c, "Off: the games as they shipped (their plain bugs fixed). New battles use these.", left, panel.1 + 96, BODY, false, INK);
            for (k, (key, label)) in gboh_game::options::ALL.iter().enumerate() {
                let y = panel.1 + 108 + k as i32 * 28;
                let r = (left, y, inner, 24);
                let on = v.options.get(key);
                frame(&mut c, (left + 3, y + 5, 14, 14), FRAME);
                if on {
                    tick(&mut c, left + 2, y + 3, 16, GREEN);
                }
                text(&mut c, label, left + 28, y + 17, BODY, false, INK);
                buttons.push((OPTION + k as u16, r));
            }
            let y = panel.1 + 108 + gboh_game::options::ALL.len() as i32 * 28 + 12;
            button(&mut c, &mut buttons, BACK, "Back", left, y, true);
        }
        Screen::Selector { entries, scroll } => {
            heading(&mut c, "Choose a battle or a campaign");
            let top = panel.1 + 84;
            let row = ROW;
            let rows = ((status_y - 40 - top) / row).max(1) as usize;
            // the scroll bar: a track the list's height, a thumb as long as
            // the share shown
            let bar = 12;
            let list_w = inner - bar - 6;
            let last = entries.len().saturating_sub(rows);
            let track = (left + inner - bar, top, bar, rows as i32 * row - 2);
            fill(&mut c, track, FACE_DOWN);
            bevel(&mut c, track, true);
            if !entries.is_empty() {
                let thumb_h = (track.3 * rows.min(entries.len()) as i32 / entries.len() as i32).max(10);
                let thumb_y = track.1 + if last == 0 { 0 } else { (track.3 - thumb_h) * *scroll as i32 / last as i32 };
                let thumb = (track.0 + 1, thumb_y, bar - 2, thumb_h);
                fill(&mut c, thumb, FACE);
                bevel(&mut c, thumb, false);
                for k in 0..rows {
                    let y = track.1 + track.3 * k as i32 / rows as i32;
                    let to = if rows > 1 { last * k / (rows - 1) } else { 0 };
                    buttons.push((SCROLL_AT + to as u16, (track.0, y, bar, (track.3 / rows as i32).max(1))));
                }
            }
            for (k, e) in entries.iter().enumerate().skip(*scroll).take(rows) {
                let y = top + (k - scroll) as i32 * row;
                let r = (left, y, list_w, row - 2);
                let playable = e.title.playable();
                let down = playable && v.pressed.is_some_and(|(px, py)| px >= r.0 && px < r.0 + r.2 && py >= r.1 && py < r.1 + r.3);
                if down {
                    fill(&mut c, r, FACE_DOWN);
                    bevel(&mut c, r, true);
                }
                let game = match e.title {
                    Title::Alexander => "Alexander",
                    Title::Hannibal => "Hannibal",
                    Title::Caesar => "Caesar",
                };
                text(&mut c, game, left + 6, y + 15, BODY, true, RED);
                text(&mut c, &e.name, left + 96, y + 15, BODY, false, if playable { INK } else { DIM });
                if let Some(d) = &e.detail {
                    // as much of it as fits
                    let x = left + 96 + width(&e.name, BODY, false) + 10;
                    let room = left + list_w - x - 4;
                    let mut shown: String = d.clone();
                    while !shown.is_empty() && width(&shown, BODY, false) > room {
                        shown.pop();
                    }
                    if shown.len() < d.len() && shown.len() > 1 {
                        shown.pop();
                        shown.push_str("...");
                    }
                    text(&mut c, &shown, x, y + 15, BODY, false, DIM);
                }
                fill(&mut c, (left, y + row - 2, list_w, 1), RULE);
                if playable {
                    buttons.push((ENTRY + k as u16, r));
                }
            }
            let y = top + rows as i32 * row + 8;
            button(&mut c, &mut buttons, BACK, "Back", left, y, true);
        }
    }
    let (line, bad) = &v.status;
    text(&mut c, line, left, status_y, BODY, false, if *bad { RED } else { DIM });
    (c, buttons)
}

/// The button at `(x, y)`, if any.
pub fn button_at(buttons: &[Button], x: i32, y: i32) -> Option<u16> {
    buttons.iter().find(|(_, (bx, by, bw, bh))| x >= *bx && x < bx + bw && y >= *by && y < by + bh).map(|(id, _)| *id)
}

/// The game selector's lines: each imported title's campaign and its
/// scenarios, by the battle's name in the scenario.
pub fn entries(imported: &[Title]) -> Vec<Entry> {
    let mut out = Vec::new();
    for &t in imported {
        let Some(dir) = crate::library::data_dir(t) else { continue };
        out.push(Entry { title: t, scenario: None, name: "Campaign".into(), detail: None });
        let mut files: Vec<(String, String)> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.to_ascii_lowercase().ends_with(".scn"))
            // (Hannibal's early drafts, which its game hides)
            .filter(|n| !n.eq_ignore_ascii_case("ILIPA2.SCN") && !n.eq_ignore_ascii_case("CANNAE2.SCN"))
            .filter_map(|n| {
                let scn = gboh_formats::scn::Scenario::parse(&std::fs::read(dir.join(&n)).ok()?).ok()?;
                let name = if scn.title.trim().is_empty() { n.clone() } else { scn.title.trim().to_string() };
                Some((name, n))
            })
            .collect();
        files.sort();
        let variants: std::collections::HashMap<String, String> =
            crate::setup::variant_files(&dir).into_iter().map(|(_, file, description)| (file.to_ascii_lowercase(), description)).collect();
        let repeated = |name: &str| files.iter().filter(|(n, _)| n == name).count() > 1;
        let entries: Vec<Entry> = files
            .iter()
            .map(|(name, file)| {
                let detail = variants.get(&file.to_ascii_lowercase()).cloned().or_else(|| {
                    repeated(name).then(|| {
                        // the original of a battle whose variants have a
                        // description, else its file name
                        let has_variant = files.iter().any(|(n, f)| n == name && variants.contains_key(&f.to_ascii_lowercase()));
                        if has_variant { "standard".to_string() } else { file.clone() }
                    })
                });
                Entry { title: t, scenario: Some(file.clone()), name: name.clone(), detail }
            })
            .collect();
        out.extend(entries);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_the_screens() {
        let v = View {
            imported: &[Title::Alexander],
            theme: Some(Title::Alexander),
            options: Default::default(),
            status: ("Importing: 40%".into(), false),
            busy: false,
            pressed: None,
        };
        let (c, buttons) = draw(&Screen::Content, &v, 1000, 700);
        // Re-import for Alexander, Import for the others, Continue and Quit
        assert!(buttons.iter().any(|b| b.0 == IMPORT) && buttons.iter().any(|b| b.0 == IMPORT + 2));
        assert!(buttons.iter().any(|b| b.0 == CONTINUE));
        let (cx, cy, cw, ch) = buttons.iter().find(|b| b.0 == QUIT).unwrap().1;
        assert_eq!(button_at(&buttons, cx + cw / 2, cy + ch / 2), Some(QUIT));
        assert!(c.pixels.contains(&(GREEN + RAMP - 1)), "the tick");
        // nothing imported: no Continue
        let none = View { imported: &[], ..v };
        let (_, buttons) = draw(&Screen::Content, &none, 1000, 700);
        assert!(!buttons.iter().any(|b| b.0 == CONTINUE));
        let (_, buttons) = draw(
            &Screen::Settings,
            &View {
                imported: &[Title::Alexander],
                theme: Some(Title::Alexander),
                options: Default::default(),
                status: (String::new(), false),
                busy: false,
                pressed: None,
            },
            1000,
            700,
        );
        assert!(buttons.iter().any(|b| b.0 == THEME) && !buttons.iter().any(|b| b.0 == THEME + 1));
        // the rules: a toggle per option, a tick on those set
        let opts = gboh_game::options::Options { roads: true, ..Default::default() };
        let (rules, buttons) = draw(
            &Screen::Rules,
            &View { imported: &[Title::Alexander], theme: Some(Title::Alexander), options: opts, status: (String::new(), false), busy: false, pressed: None },
            1000,
            700,
        );
        assert_eq!(buttons.iter().filter(|b| b.0 >= OPTION).count(), gboh_game::options::ALL.len());
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            crate::ui::write_ppm(&rules, &palette(), &std::path::Path::new(&out).join("rules.ppm")).unwrap();
        }
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let (c, _) = draw(
                &Screen::Content,
                &View {
                    imported: &[Title::Alexander],
                    theme: None,
                    options: Default::default(),
                    status: ("Importing: 40%".into(), false),
                    busy: false,
                    pressed: None,
                },
                1000,
                700,
            );
            crate::ui::write_ppm(&c, &palette(), &std::path::Path::new(&out).join("content.ppm")).unwrap();
        }
    }
}
