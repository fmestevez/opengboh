//! What the screens share: the game's bitmap font (`MSSANS.EIF`, MS Sans
//! Serif 8) and its art files.

use gboh_formats::eif::Eif;
use gboh_render::{Canvas, Sprite, strip};
use std::path::Path;

/// An EIF file of the game's `DATA` directory.
pub fn eif(dir: &Path, name: &str) -> Option<Eif> {
    let path = crate::find_ci(dir, name)?;
    Eif::parse(&std::fs::read(path).ok()?).ok()
}

/// All cels of strip `name` of `e`.
pub fn cels(e: &Option<Eif>, name: &str) -> Vec<Sprite> {
    e.as_ref().and_then(|e| strip(e, name)).unwrap_or_default()
}

/// The first cel of strip `name` of `e`.
pub fn first(e: &Option<Eif>, name: &str) -> Option<Sprite> {
    cels(e, name).into_iter().next()
}

/// A bitmap font: glyphs from a first character on, each with its
/// baseline (hotspot row).
pub struct Font {
    glyphs: Vec<(Sprite, i32)>,
    /// The character of the first glyph.
    first: usize,
    /// A colour to draw every glyph pixel in, if not the glyphs' own.
    ink: Option<u8>,
    /// A character the font has no glyph for (those after it come one
    /// glyph earlier).
    missing: Option<char>,
    /// `(` and `)` when the font has them in one glyph.
    brackets: Option<((Sprite, i32), (Sprite, i32))>,
}

impl Font {
    pub fn load(data_dir: &Path) -> Self {
        Self::load_from(data_dir, "MSSANS.EIF", "Alphanumeric", '!')
    }

    /// The smaller `MS8TEXT.EIF` ("MS Sans Serif 8"), close to the system
    /// font the original's dialogs write in.
    pub fn small(data_dir: &Path) -> Self {
        // its 93 glyphs run from `!` with both brackets in the one glyph of
        // `(` and none for `)`; drawn in the palette's darkest colour, as the
        // original's dialogs write in black
        let ink = crate::find_ci(data_dir, "GAME.PAL")
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| gboh_formats::pal::Palette::parse(&d).ok())
            .and_then(|pal| (1..=255u8).min_by_key(|&i| pal.colors[i as usize].iter().map(|&c| c as u32).sum::<u32>()));
        let mut font = Self { ink, missing: Some(')'), ..Self::load_from(data_dir, "MS8TEXT.EIF", "Strip #1", '!') };
        // split the bracket glyph into `(` and `)`
        if let Some((both, base)) = font.glyphs.get(('(' as usize) - ('!' as usize)).cloned() {
            let half = |from: usize, to: usize| {
                let w = to - from;
                let pixels = (0..both.height).flat_map(|y| both.pixels[y * both.width + from..y * both.width + to].to_vec()).collect();
                (Sprite { width: w, height: both.height, pixels }, base)
            };
            let mid = both.width / 2;
            font.brackets = Some((half(0, mid), half(mid, both.width)));
        }
        font
    }

    fn load_from(data_dir: &Path, file: &str, strip_name: &str, first: char) -> Self {
        let glyphs = eif(data_dir, file)
            .and_then(|e| {
                let s = e.strips.iter().find(|s| s.name == strip_name)?;
                let sprites = strip(&e, strip_name)?;
                Some(sprites.into_iter().zip(s.cels.iter().map(|c| c.hot_y as i32)).collect())
            })
            .unwrap_or_default();
        Self { glyphs, first: first as usize, ink: None, missing: None, brackets: None }
    }

    fn glyph(&self, c: char) -> Option<&(Sprite, i32)> {
        if let Some((open, close)) = &self.brackets {
            match c {
                '(' => return Some(open),
                ')' => return Some(close),
                _ => {}
            }
        }
        if Some(c) == self.missing {
            return None;
        }
        let skip = self.missing.is_some_and(|m| c > m) as usize;
        self.glyphs.get((c as usize).checked_sub(self.first + skip)?)
    }

    /// The width of `text`.
    pub fn width(&self, text: &str) -> i32 {
        text.chars().map(|c| self.glyph(c).map_or(3, |g| g.0.width as i32)).sum()
    }

    /// Draw `text` with its top-left at `(x, y)`, glyphs on a common
    /// baseline 10 rows down; the font's second colour (index 252, a shadow
    /// to the lower right) is left out (🟡 what the original maps it to is
    /// not traced).
    pub fn draw(&self, canvas: &mut Canvas, text: &str, x: i32, y: i32) {
        let mut x = x;
        for c in text.chars() {
            let Some((g, base)) = self.glyph(c) else {
                x += 3;
                continue;
            };
            let mut plain = g.clone();
            for p in plain.pixels.iter_mut() {
                if *p == 252 {
                    *p = 0;
                } else if let (Some(ink), true) = (self.ink, *p != 0) {
                    *p = ink;
                }
            }
            canvas.blit(&plain, x, y + 10 - base);
            x += g.width as i32;
        }
    }
}
