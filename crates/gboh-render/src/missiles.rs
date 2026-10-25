//! Missiles in flight: arrows, javelins, bolts and sling stones.
//!
//! The original gives each shooting figure its own missile sprite
//! (`FUN_0052ee77`): hidden while the man draws, then shown with cel 0 of
//! the strip for his facing (`N`…`NW`) on the way up and cel 1 on the way
//! down (due south keeps cel 0), from a point by the man over a raised
//! midpoint to the target with a little random scatter. Stones have one
//! strip for every direction. 🟡 The flight timing belongs to the
//! original's sprite motion engine, which is not traced: the rewrite moves
//! missiles at a fixed speed along a parabola.

use crate::Sprite;
use crate::objects::Drawable;
use gboh_formats::eif::Eif;

/// Which missile art (the original's sets, in its order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missile {
    Arrow,
    Javelin,
    Bolt,
    Stone,
    /// Hannibal's: thrown from horseback (and elephants), by light
    /// infantry, and the legions' pila.
    MountedJavelin,
    LightJavelin,
    Pilum,
    /// Caesar's barbarians' thrown axes.
    Axe,
}

impl Missile {
    pub const ALL: [Missile; 8] = [
        Missile::Arrow,
        Missile::Javelin,
        Missile::Bolt,
        Missile::Stone,
        Missile::MountedJavelin,
        Missile::LightJavelin,
        Missile::Pilum,
        Missile::Axe,
    ];

    /// Caesar's missile art table's entry `k`.
    pub fn caesar(k: u8) -> Option<Self> {
        Some(match k {
            0 => Missile::Arrow,
            1 => Missile::Bolt,
            2 => Missile::Javelin,
            3 => Missile::Axe,
            4 => Missile::MountedJavelin,
            5 => Missile::LightJavelin,
            6 => Missile::Pilum,
            7 => Missile::Stone,
            _ => return None,
        })
    }

    /// The EIF title of the art at full size.
    pub fn title(self) -> &'static str {
        match self {
            Missile::Arrow => "Arrows",
            Missile::Javelin => "Javelins",
            Missile::Bolt => "Bolt",
            Missile::Stone => "Stone",
            Missile::MountedJavelin => "Mounted Javelins",
            Missile::LightJavelin => "Light Javelins",
            Missile::Pilum => "Pilum Javelins",
            Missile::Axe => "Axes",
        }
    }

    /// Whether it's a javelin of some kind.
    pub fn javelin(self) -> bool {
        matches!(self, Missile::Javelin | Missile::MountedJavelin | Missile::LightJavelin | Missile::Pilum | Missile::Axe)
    }
}

/// One missile set's cels per screen facing, with hotspots.
pub struct MissileSprites {
    dirs: Vec<Vec<(Sprite, (i32, i32))>>,
}

impl MissileSprites {
    pub fn load(eif: &Eif) -> Option<Self> {
        let cels = |name: &str| -> Option<Vec<(Sprite, (i32, i32))>> {
            let s = eif.strips.iter().find(|s| s.name == name)?;
            let sprites = crate::strip(eif, name)?;
            Some(sprites.into_iter().zip(s.cels.iter().map(|c| (c.hot_x as i32, c.hot_y as i32))).collect())
        };
        let dirs: Option<Vec<_>> = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"].iter().map(|d| cels(d)).collect();
        let dirs = dirs.or_else(|| cels("Stone").map(|s| vec![s; 8]))?;
        Some(Self { dirs })
    }
}

/// Spent missiles on the ground (`Missile Debris`: strips `Arrow`,
/// `Javelin`, `Stone`).
pub struct DebrisSprites {
    strips: Vec<(Missile, Vec<Cel>)>,
}

/// A sprite with its hotspot.
type Cel = (Sprite, (i32, i32));

impl DebrisSprites {
    pub fn load(eif: &Eif) -> Self {
        let cels = |name: &str| -> Vec<Cel> {
            let Some(s) = eif.strips.iter().find(|s| s.name == name) else { return Vec::new() };
            let sprites = crate::strip(eif, name).unwrap_or_default();
            sprites.into_iter().zip(s.cels.iter().map(|c| (c.hot_x as i32, c.hot_y as i32))).collect()
        };
        let strips = vec![
            (Missile::Arrow, cels("Arrow")),
            (Missile::Bolt, cels("Arrow")),
            (Missile::Javelin, cels("Javelin")),
            (Missile::Stone, cels("Stone")),
            (Missile::MountedJavelin, cels("Javelin")),
            (Missile::LightJavelin, cels("Javelin")),
            (Missile::Pilum, cels("Javelin")),
            (Missile::Axe, cels("Javelin")),
        ];
        Self { strips }
    }

    /// The drawable for a spent `missile` lying at canvas point `anchor`,
    /// cel `cel` (cycling).
    pub fn drawable(&self, missile: Missile, cel: usize, anchor: (i32, i32)) -> Option<Drawable<'_>> {
        let cels = &self.strips.iter().find(|(m, _)| *m == missile)?.1;
        let (sprite, hot) = cels.get(cel % cels.len().max(1))?;
        Some(Drawable { anchor, depth: -1, sprite, hot: *hot })
    }
}

/// Where a spent missile lies in the hex it fell on, from the hex's anchor
/// in zoom-in pixels (block 130 × 76), given two random numbers: up and to
/// the left by a fifth of the width plus up to a tenth, and a quarter of
/// the height plus up to an eighth (javelins: a third plus up to a sixth)
/// (`FUN_005079e1`; none falls on water).
pub fn debris_offset(missile: Missile, r1: u32, r2: u32) -> (i32, i32) {
    let (w, h) = (130, 76);
    let dx = (r1 % (w / 10)) as i32 + w as i32 / 5;
    let dy = match missile {
        m if m.javelin() => (r2 % (h / 6)) as i32 + h as i32 / 3,
        _ => (r2 % (h / 8)) as i32 + h as i32 / 4,
    };
    (-dx, -dy)
}

/// A missile on its way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    pub missile: Missile,
    /// Screen facing of the shooter (1 N … 8 NW).
    pub facing: usize,
    /// Canvas points it leaves from and lands on.
    pub from: (i32, i32),
    pub to: (i32, i32),
    /// How far the midpoint is raised, in canvas pixels.
    pub lift: i32,
    /// How far along, 0 to 1.
    pub t: f32,
}

impl Flight {
    /// The point reached at `t`, on the parabola through the raised midpoint.
    pub fn at(&self) -> (i32, i32) {
        let t = self.t.clamp(0.0, 1.0);
        let lerp = |a: i32, b: i32| a as f32 + (b - a) as f32 * t;
        let rise = 4.0 * t * (1.0 - t) * self.lift as f32;
        (lerp(self.from.0, self.to.0) as i32, (lerp(self.from.1, self.to.1) - rise) as i32)
    }

    /// Its sprite: rising cel, then falling cel.
    pub fn drawable<'a>(&self, art: &'a MissileSprites) -> Option<Drawable<'a>> {
        let cels = art.dirs.get(self.facing.checked_sub(1)?)?;
        let cel = if self.t < 0.5 || self.facing == 5 { 0 } else { 1 };
        let (sprite, hot) = cels.get(cel.min(cels.len().checked_sub(1)?))?;
        Some(Drawable { anchor: self.at(), depth: 0, sprite, hot: *hot })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flies_over_the_midpoint() {
        let mut f = Flight { missile: Missile::Arrow, facing: 3, from: (0, 100), to: (200, 100), lift: 40, t: 0.0 };
        assert_eq!(f.at(), (0, 100));
        f.t = 0.5;
        assert_eq!(f.at(), (100, 60));
        f.t = 1.0;
        assert_eq!(f.at(), (200, 100));
    }
}
