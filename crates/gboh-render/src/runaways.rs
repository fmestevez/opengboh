//! Riderless horses and runaway chariots.
//!
//! When a man dies in shock (`FUN_004eb617`), a cavalryman leaves a horse
//! running loose 15% of the time (a cataphracted horse for cataphracts), and
//! a chariot leaves a runaway 25% of the time: one dragging its crew in a
//! quarter of those (Persian or Indian), else an empty one. The runaway
//! sets off in a random direction and runs until it leaves the map
//! (`FUN_0050ae12`): at each hex it may keep on or veer one side either way,
//! never turning away from the nearest map edge, preferring a road, else at
//! random.

use crate::objects::{Drawable, anchor_at};
use crate::terrain::Layout;
use crate::units::UnitSprites;
use gboh_formats::map::Map;

/// Which loose animal or chariot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Runaway {
    Horse,
    CataphractHorse,
    RunningChariot,
    DraggingPersian,
    DraggingIndian,
}

impl Runaway {
    pub const ALL: [Runaway; 5] =
        [Runaway::Horse, Runaway::CataphractHorse, Runaway::RunningChariot, Runaway::DraggingPersian, Runaway::DraggingIndian];

    /// The EIF title of the art at full size.
    pub fn title(self) -> &'static str {
        match self {
            Runaway::Horse => "Horse",
            Runaway::CataphractHorse => "Cataphracted Horse",
            Runaway::RunningChariot => "Running Chariot",
            Runaway::DraggingPersian => "Chariot Dragging Persian",
            Runaway::DraggingIndian => "Chariot Dragging Indian",
        }
    }

    /// What a man of unit type `type_id` leaves when he dies in shock, given
    /// the original's two `rand() % 100` rolls (the second only for
    /// chariots).
    pub fn left_by(type_id: usize, roll: u32, second: u32) -> Option<Self> {
        // (Hannibal's cataphracts leave plain horses: its game never makes
        // a cataphracted horse; its chariots are Persian)
        let persian = type_id == 22 || type_id >= gboh_formats::units::HANNIBAL;
        match type_id {
            41 | 44 if roll < 15 => Some(Runaway::CataphractHorse),
            t if gboh_formats::units::chariot(t) && roll < 25 => Some(if second >= 25 {
                Runaway::RunningChariot
            } else if persian {
                Runaway::DraggingPersian
            } else {
                Runaway::DraggingIndian
            }),
            _ if roll < 15 && crate::units::Formation::of_type(type_id) == crate::units::Formation::Cavalry => {
                Some(Runaway::Horse)
            }
            _ => None,
        }
    }
}

/// The six screen directions a hex has neighbours in, clockwise (2 NE, 3 E,
/// 4 SE, 6 SW, 7 W, 8 NW).
const SIDES: [usize; 6] = [2, 3, 4, 6, 7, 8];

fn turn(dir: usize, by: i32) -> usize {
    let i = SIDES.iter().position(|&d| d == dir).unwrap_or(0) as i32;
    SIDES[(i + by).rem_euclid(6) as usize]
}

/// The screen neighbour of screen hex `(r, c)` in screen direction `dir`.
fn step(layout: &Layout, (r, c): (usize, usize), dir: usize) -> Option<(usize, usize)> {
    let [left, ul, ur, right] = layout.neighbours(r, c);
    let [ll, lr] = layout.lower(r, c);
    match dir {
        2 => ur,
        3 => right,
        4 => lr,
        6 => ll,
        7 => left,
        8 => ul,
        _ => None,
    }
}

/// A runaway's way from map hex `start`, setting off in screen direction
/// `dir` (1 N … 8 NW), with `rand` the random numbers: the map hexes it
/// runs through, each with the screen direction it leaves by, up to the
/// last hex before the edge.
pub fn path(map: &Map, layout: &Layout, start: (usize, usize), mut dir: usize, rand: &mut impl FnMut() -> u32) -> Vec<((usize, usize), usize)> {
    let mut at = layout.to_tile(start.0, start.1);
    let mut out = Vec::new();
    if dir == 1 || dir == 5 {
        // due north or south: veer to one side, and take a first step
        dir = if rand().is_multiple_of(2) { dir + 1 } else { (dir + 6) % 8 + 1 };
        out.push((layout.to_map(at.0, at.1), dir));
        match step(layout, at, dir) {
            Some(n) => at = n,
            None => return out,
        }
    }
    let road = |(r, c): (usize, usize)| {
        let (x, y) = layout.to_map(r, c);
        map.hex_features(x, y).iter().any(|&f| f == 12 || f == 13)
    };
    loop {
        let dirs = [dir, turn(dir, -1), turn(dir, 1)];
        let mut next: [Option<(usize, usize)>; 3] = dirs.map(|d| step(layout, at, d));
        if next.iter().any(|n| n.is_none()) {
            out.push((layout.to_map(at.0, at.1), dir));
            break;
        }
        // never away from the nearest edge
        let (r, c) = (at.0 as i32, at.1 as i32);
        let (left, right, top, bottom) = (c, layout.cols as i32 - c, r, layout.rows as i32 - r);
        let away: &[usize] = if left < right && left < top && left < bottom {
            &[3]
        } else if right < left && right < top && right < bottom {
            &[7]
        } else if top < right && top < left && top < bottom {
            &[6, 4]
        } else {
            &[8, 2]
        };
        for (i, d) in dirs.iter().enumerate() {
            if away.contains(d) {
                next[i] = None;
            }
        }
        // a road if there is one
        if let Some(i) = (0..3).find(|&i| next[i].is_some_and(road)) {
            for (j, n) in next.iter_mut().enumerate() {
                if j != i {
                    *n = None;
                }
            }
        }
        let mut k = (rand() % 4) as usize;
        let mut tries = 0;
        while next[k % 3].is_none() && tries < 4 {
            k = k % 3 + 1;
            tries += 1;
        }
        let Some(n) = next[k % 3] else {
            out.push((layout.to_map(at.0, at.1), dir));
            break;
        };
        dir = dirs[k % 3];
        // each hex with the way it leaves by
        out.push((layout.to_map(at.0, at.1), dir));
        at = n;
    }
    out
}

/// A runaway being drawn: what, its way, how many hexes along it is and
/// its walking frame.
pub type Runner<'a> = (Runaway, &'a [((usize, usize), usize)], f32, usize);

/// A runaway on its way: `along` hexes from the start of `path`, on
/// walking frame `frame`.
pub fn drawable<'a>(map: &Map, layout: &Layout, art: &'a UnitSprites, path: &[((usize, usize), usize)], along: f32, frame: usize) -> Option<Drawable<'a>> {
    let i = (along as usize).min(path.len().checked_sub(1)?);
    let (hex, dir) = path[i];
    let frac = if i + 1 < path.len() { along - i as f32 } else { 0.0 };
    let a = anchor_at(map, layout, hex, (0, 0));
    let b = path.get(i + 1).map_or(a, |&(h, _)| anchor_at(map, layout, h, (0, 0)));
    let anchor = (a.0 + ((b.0 - a.0) as f32 * frac) as i32, a.1 + ((b.1 - a.1) as f32 * frac) as i32);
    let (sprite, hot) = art.walk_cel(dir, frame)?;
    Some(Drawable { anchor, depth: 0, sprite, hot: *hot })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::Zoom;

    #[test]
    fn runs_to_the_edge() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let map = Map::parse(&std::fs::read(std::path::Path::new(&dir).join("GAUGAMEL.MAP")).unwrap()).unwrap();
        let layout = Layout::new(&map, Zoom::Normal);
        for d in 1..=8 {
            let mut seed = d as u32;
            let mut rand = || {
                seed = seed.wrapping_mul(22695477).wrapping_add(1);
                (seed >> 16) & 0x7fff
            };
            let p = path(&map, &layout, (16, 22), d, &mut rand);
            assert!(p.len() > 3, "direction {d}: {p:?}");
            // each step is to a neighbour, and it ends by the edge
            for w in p.windows(2) {
                let (a, b) = (w[0].0, w[1].0);
                let (ra, ca) = layout.to_tile(a.0, a.1);
                let next = step(&layout, (ra, ca), w[0].1).map(|(r, c)| layout.to_map(r, c));
                assert_eq!(next, Some(b), "direction {d}");
            }
            let &((x, y), last) = p.last().unwrap();
            let (r, c) = layout.to_tile(x, y);
            assert!([last, turn(last, -1), turn(last, 1)].iter().any(|&d| step(&layout, (r, c), d).is_none()));
        }
        assert_eq!(Runaway::left_by(41, 10, 0), Some(Runaway::CataphractHorse));
        assert_eq!(Runaway::left_by(9, 14, 0), Some(Runaway::Horse));
        assert_eq!(Runaway::left_by(9, 15, 0), None);
        assert_eq!(Runaway::left_by(22, 20, 10), Some(Runaway::DraggingPersian));
        assert_eq!(Runaway::left_by(23, 20, 50), Some(Runaway::RunningChariot));
        assert_eq!(Runaway::left_by(2, 0, 0), None);
    }
}
