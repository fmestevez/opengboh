//! Faithful software renderer for the original 8-bit battlefield view.
//!
//! Everything is drawn into an indexed [`Canvas`] the way the original drew
//! into its WinG bitmap; colours are only applied at the end with
//! [`Canvas::to_rgba`]. Game logic lives elsewhere: this crate only turns map
//! data and sprites into pixels.

pub mod battlefield;
pub mod hanland;
pub mod missiles;
pub mod objects;
pub mod runaways;
pub mod terrain;
pub mod units;

use gboh_formats::eif::{Eif, Image};
use gboh_formats::pal::Palette;

/// A decoded sprite: palette indices, 0 = transparent.
#[derive(Debug, Clone)]
pub struct Sprite {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl From<Image> for Sprite {
    fn from(img: Image) -> Self {
        Self { width: img.width, height: img.height, pixels: img.pixels }
    }
}

/// All cels of one named strip, decoded.
pub fn strip(eif: &Eif, name: &str) -> Option<Vec<Sprite>> {
    let s = eif.strips.iter().find(|s| s.name == name)?;
    s.cels.iter().map(|c| c.decode().ok().map(Sprite::from)).collect()
}

/// As [`strip`], the RLE cels' effect pixels (stored without colours)
/// made shadow pixels (index 255): Hannibal's shading cels are drawn so.
pub fn strip_with_shadows(eif: &Eif, name: &str) -> Option<Vec<Sprite>> {
    let s = eif.strips.iter().find(|s| s.name == name)?;
    s.cels
        .iter()
        .map(|c| {
            let img = c.decode().ok()?;
            let pixels = img.pixels.iter().zip(&img.effect).map(|(&p, &e)| if p == 0 && e != 0 { 255 } else { p }).collect();
            Some(Sprite { width: img.width, height: img.height, pixels })
        })
        .collect()
}

/// An 8-bit indexed framebuffer.
#[derive(Debug, Clone)]
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, fill: u8) -> Self {
        Self { width, height, pixels: vec![fill; width * height] }
    }

    /// Fill the whole canvas with `tile` repeated from `(x0, y0)` (no
    /// transparency).
    pub fn tile(&mut self, tile: &Sprite, x0: i32, y0: i32) {
        let (tw, th) = (tile.width as i32, tile.height as i32);
        for y in 0..self.height as i32 {
            let ty = (y - y0).rem_euclid(th) as usize;
            for x in 0..self.width as i32 {
                let tx = (x - x0).rem_euclid(tw) as usize;
                self.pixels[y as usize * self.width + x as usize] = tile.pixels[ty * tile.width + tx];
            }
        }
    }

    /// Draw `s` with its top-left corner at `(x, y)`, skipping index 0.
    pub fn blit(&mut self, s: &Sprite, x: i32, y: i32) {
        for sy in 0..s.height {
            let dy = y + sy as i32;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            let row = &s.pixels[sy * s.width..(sy + 1) * s.width];
            let out = dy as usize * self.width;
            for (sx, &p) in row.iter().enumerate() {
                let dx = x + sx as i32;
                if p != 0 && dx >= 0 && dx < self.width as i32 {
                    self.pixels[out + dx as usize] = p;
                }
            }
        }
    }

    /// Like [`blit`](Self::blit), but index 255 is a shadow: the pixel
    /// underneath is passed through `remap` instead of being overwritten.
    pub fn blit_shadowed(&mut self, s: &Sprite, x: i32, y: i32, remap: &[u8; 256]) {
        for sy in 0..s.height {
            let dy = y + sy as i32;
            if dy < 0 || dy >= self.height as i32 {
                continue;
            }
            let out = dy as usize * self.width;
            for (sx, &p) in s.pixels[sy * s.width..(sy + 1) * s.width].iter().enumerate() {
                let dx = x + sx as i32;
                if p == 0 || dx < 0 || dx >= self.width as i32 {
                    continue;
                }
                let d = &mut self.pixels[out + dx as usize];
                *d = if p == 255 { remap[*d as usize] } else { p };
            }
        }
    }

    /// Like [`blit`](Self::blit) but only the columns `from..to` of the
    /// sprite (clipped to its width).
    pub fn blit_columns(&mut self, s: &Sprite, x: i32, y: i32, from: usize, to: usize) {
        let to = to.min(s.width);
        if from >= to {
            return;
        }
        let part = Sprite { width: to - from, height: s.height, pixels: s.pixels.chunks(s.width).flat_map(|r| r[from..to].iter().copied()).collect() };
        self.blit(&part, x + from as i32, y);
    }

    pub fn to_rgba(&self, pal: &Palette) -> Vec<u8> {
        self.pixels
            .iter()
            .flat_map(|&p| {
                let [r, g, b] = pal.colors[p as usize];
                [r, g, b, 255]
            })
            .collect()
    }
}

/// Water colours the original installs over `GAME.PAL` during battles,
/// cycled through four frames by its palette animator: per frame, the RGB of
/// indices 202, 205, 207, 208 and 210 (203, 204, 206 and 209 keep their
/// `GAME.PAL` colours).
const WATER_FRAMES: [[(u8, [u8; 3]); 5]; 4] = {
    const A: [u8; 3] = [85, 115, 145];
    const B: [u8; 3] = [70, 100, 130];
    const C: [u8; 3] = [75, 105, 135];
    const D: [u8; 3] = [80, 110, 140];
    [
        [(202, [150, 190, 235]), (205, A), (207, B), (208, D), (210, C)],
        [(202, [145, 175, 215]), (205, B), (207, C), (208, A), (210, D)],
        [(202, [140, 160, 195]), (205, C), (207, D), (208, B), (210, A)],
        [(202, [145, 175, 215]), (205, D), (207, A), (208, C), (210, B)],
    ]
};

/// Apply frame `frame` (mod 4) of `game`'s battle water animation to
/// `pal`: Hannibal and Caesar cycle the same colours through indices
/// 41..=45 (in the order of Alexander's 202, 205, 207, 208, 210).
pub fn animate_water(pal: &mut Palette, frame: usize, game: gboh_formats::game::Game) {
    use gboh_formats::game::Game;
    for (k, &(i, rgb)) in WATER_FRAMES[frame % 4].iter().enumerate() {
        let i = match game {
            Game::Alexander => i,
            Game::Hannibal | Game::Caesar => 41 + k as u8,
        };
        pal.colors[i as usize] = rgb;
    }
}

pub use gboh_formats::crt::BorlandRand;
