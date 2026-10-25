//! The menu bar over the battle window: the original's menus (`CHILDMENU`
//! of the player's `alex.exe`, read at run time), drawn as Windows drew
//! them, in the system colours of the game's palette (light grey 7, dark
//! grey 248, white 255, navy 4 for the highlight; black is the palette's
//! darkest colour other than 0, which the overlay leaves see-through).

use crate::ui::Font;
use gboh_formats::pe::MenuItem;
use gboh_render::Canvas;
use std::path::Path;

/// Height of the bar.
pub const MENU_HEIGHT: i32 = 19;
/// Height of a command in a menu, and of a separator.
const ITEM: i32 = 17;
const SEPARATOR: i32 = 8;
/// Room left of a command (for its check mark) and right of it.
const LEFT: i32 = 18;
const RIGHT: i32 = 18;

const FACE: u8 = 7;
const SHADOW: u8 = 248;
const LIGHT: u8 = 255;
const HIGHLIGHT: u8 = 4;

/// Which menu is open and which of its commands the mouse is over.
pub type Open = (usize, Option<usize>);

pub struct MenuBar {
    pub menus: Vec<MenuItem>,
    font: Font,
    black: u8,
}

/// `text` without its `&` marks, the index of the character after the
/// first (the mnemonic, drawn underlined), and the accelerator after a tab.
fn label(text: &str) -> (String, Option<usize>, String) {
    let (main, accel) = text.split_once('\t').unwrap_or((text, ""));
    let mut out = String::new();
    let mut mnemonic = None;
    let mut chars = main.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            if chars.peek() == Some(&'&') {
                chars.next();
                out.push('&');
            } else if mnemonic.is_none() {
                mnemonic = Some(out.chars().count());
            }
            continue;
        }
        out.push(c);
    }
    (out, mnemonic, accel.to_string())
}

fn fill(canvas: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), c: u8) {
    for yy in y.max(0)..(y + h).min(canvas.height as i32) {
        for xx in x.max(0)..(x + w).min(canvas.width as i32) {
            canvas.pixels[yy as usize * canvas.width + xx as usize] = c;
        }
    }
}

impl MenuBar {
    /// The menu `name` of the executable in `data_dir`'s parent; none if
    /// it cannot be read.
    pub fn load(data_dir: &Path, name: &str) -> Self {
        let menus = crate::ui::exe_resources(data_dir).and_then(|r| r.menu(name)).unwrap_or_default();
        let font = Font::small(data_dir);
        let black = font.ink().unwrap_or(1);
        Self { menus, font, black }
    }

    /// Each top-level menu's x and width on the bar.
    fn bar_places(&self) -> Vec<(i32, i32)> {
        let mut x = 4;
        self.menus
            .iter()
            .map(|m| {
                let w = self.font.width(&label(&m.text).0) + 12;
                let at = (x, w);
                x += w;
                at
            })
            .collect()
    }

    /// The top-level menu at `(x, y)`, if on the bar.
    pub fn bar_at(&self, x: i32, y: i32) -> Option<usize> {
        if !(0..MENU_HEIGHT).contains(&y) {
            return None;
        }
        self.bar_places().iter().position(|&(bx, bw)| x >= bx && x < bx + bw)
    }

    /// Where menu `k` drops down: its rectangle.
    fn drop_rect(&self, k: usize) -> (i32, i32, i32, i32) {
        let (x, _) = self.bar_places().get(k).copied().unwrap_or((0, 0));
        let items = &self.menus[k].items;
        let widest = items
            .iter()
            .map(|i| {
                let (t, _, a) = label(&i.text);
                self.font.width(&t) + if a.is_empty() { 0 } else { self.font.width(&a) + 16 }
            })
            .max()
            .unwrap_or(0);
        let h: i32 = items.iter().map(|i| if i.text.is_empty() { SEPARATOR } else { ITEM }).sum();
        (x, MENU_HEIGHT, widest + LEFT + RIGHT + 6, h + 6)
    }

    /// The command of open menu `k` at `(x, y)`: its index, or `None` off
    /// the commands (on a separator, or outside the menu).
    pub fn item_at(&self, k: usize, x: i32, y: i32) -> Option<usize> {
        let (mx, my, mw, mh) = self.drop_rect(k);
        if x < mx || x >= mx + mw || y < my || y >= my + mh {
            return None;
        }
        let mut top = my + 3;
        for (i, item) in self.menus[k].items.iter().enumerate() {
            let h = if item.text.is_empty() { SEPARATOR } else { ITEM };
            if y >= top && y < top + h {
                return (!item.text.is_empty()).then_some(i);
            }
            top += h;
        }
        None
    }

    /// The command id of item `i` of menu `k`.
    pub fn id(&self, k: usize, i: usize) -> Option<u16> {
        self.menus.get(k)?.items.get(i).map(|i| i.id)
    }

    fn text(&self, canvas: &mut Canvas, text: &str, x: i32, y: i32, ink: u8, disabled: bool) {
        let (t, mnemonic, _) = label(text);
        if disabled {
            // greyed: embossed in white under dark grey
            self.font.draw_in(canvas, &t, x + 1, y + 1, LIGHT);
            self.font.draw_in(canvas, &t, x, y, SHADOW);
            return;
        }
        self.font.draw_in(canvas, &t, x, y, ink);
        if let Some(m) = mnemonic {
            let before: String = t.chars().take(m).collect();
            let c: String = t.chars().skip(m).take(1).collect();
            let ux = x + self.font.width(&before);
            fill(canvas, (ux, y + 12, self.font.width(&c).max(2) - 1, 1), ink);
        }
    }

    /// Draw the bar across the top of `canvas`, and the open menu: commands
    /// that `enabled` rejects greyed, those `checked` accepts ticked.
    pub fn draw(&self, canvas: &mut Canvas, open: Option<Open>, enabled: impl Fn(u16) -> bool, checked: impl Fn(u16) -> bool) {
        let w = canvas.width as i32;
        fill(canvas, (0, 0, w, MENU_HEIGHT), FACE);
        fill(canvas, (0, MENU_HEIGHT - 1, w, 1), SHADOW);
        for (k, (m, (x, bw))) in self.menus.iter().zip(self.bar_places()).enumerate() {
            let is_open = open.is_some_and(|(o, _)| o == k);
            if is_open {
                fill(canvas, (x, 1, bw, MENU_HEIGHT - 2), HIGHLIGHT);
            }
            self.text(canvas, &m.text, x + 6, 3, if is_open { LIGHT } else { self.black }, false);
        }
        let Some((k, hover)) = open else { return };
        let Some(menu) = self.menus.get(k) else { return };
        let (mx, my, mw, mh) = self.drop_rect(k);
        fill(canvas, (mx, my, mw, mh), FACE);
        // a raised frame: white and light grey in, dark grey and black out
        fill(canvas, (mx, my, mw, 1), FACE);
        fill(canvas, (mx, my, 1, mh), FACE);
        fill(canvas, (mx + 1, my + 1, mw - 2, 1), LIGHT);
        fill(canvas, (mx + 1, my + 1, 1, mh - 2), LIGHT);
        fill(canvas, (mx, my + mh - 1, mw, 1), self.black);
        fill(canvas, (mx + mw - 1, my, 1, mh), self.black);
        fill(canvas, (mx + 1, my + mh - 2, mw - 2, 1), SHADOW);
        fill(canvas, (mx + mw - 2, my + 1, 1, mh - 2), SHADOW);
        let mut top = my + 3;
        for (i, item) in menu.items.iter().enumerate() {
            if item.text.is_empty() {
                fill(canvas, (mx + 3, top + 3, mw - 6, 1), SHADOW);
                fill(canvas, (mx + 3, top + 4, mw - 6, 1), LIGHT);
                top += SEPARATOR;
                continue;
            }
            let on = enabled(item.id);
            let lit = hover == Some(i);
            if lit {
                fill(canvas, (mx + 3, top, mw - 6, ITEM), HIGHLIGHT);
            }
            let ink = if lit { LIGHT } else { self.black };
            let greyed = !on && !lit;
            self.text(canvas, &item.text, mx + 3 + LEFT, top + 2, if on { ink } else { SHADOW }, greyed);
            let (_, _, accel) = label(&item.text);
            if !accel.is_empty() {
                let ax = mx + mw - 3 - RIGHT - self.font.width(&accel);
                if greyed {
                    self.font.draw_in(canvas, &accel, ax + 1, top + 3, LIGHT);
                }
                self.font.draw_in(canvas, &accel, ax, top + 2, if on { ink } else { SHADOW });
            }
            if checked(item.id) {
                // a tick
                let (cx, cy) = (mx + 3 + 5, top + 8);
                for (dx, dy) in [(0, 0), (1, 1), (2, 2), (3, 1), (4, 0), (5, -1), (6, -2)] {
                    fill(canvas, (cx + dx, cy + dy, 1, 3), ink);
                }
            }
            top += ITEM;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(label("&Save\tCtrl+S"), ("Save".into(), Some(0), "Ctrl+S".into()));
        assert_eq!(label("Save &As..."), ("Save As...".into(), Some(5), String::new()));
        assert_eq!(label("A && B"), ("A & B".into(), None, String::new()));
    }

    /// Draws the battle window's menus; runs only with `GBOH_DATA` set (and
    /// writes them to `GBOH_SHOTS`, if set).
    #[test]
    fn draws_the_menus() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let bar = MenuBar::load(Path::new(&dir), "CHILDMENU");
        assert_eq!(bar.menus.len(), 6);
        let k = bar.bar_at(10, 5).unwrap();
        assert_eq!(k, 0);
        let (mx, my, _, _) = bar.drop_rect(0);
        let save = bar.item_at(0, mx + 30, my + 3 + ITEM + 5).unwrap();
        assert_eq!(bar.id(0, save), Some(226));
        let mut c = Canvas::new(500, 260, 0);
        bar.draw(&mut c, Some((0, Some(save))), |id| id != 227, |id| id == 229);
        if let Ok(out) = std::env::var("GBOH_SHOTS") {
            let pal = crate::palette_path(Path::new(&dir)).unwrap();
            let pal = gboh_formats::pal::Palette::parse(&std::fs::read(pal).unwrap()).unwrap();
            crate::ui::write_ppm(&c, &pal, &Path::new(&out).join("menu.ppm")).unwrap();
        }
    }
}
