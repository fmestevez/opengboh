//! Terrain objects: trees and stones scattered over hexes (see
//! `docs/formats.md`, "Terrain objects").
//!
//! The original creates these once per battle as physical objects, the same
//! kind as units, so they are depth sorted with everything else rather than
//! baked into the terrain. Positions are generated in zoom-in pixels and
//! scaled down for the other zooms.

use crate::terrain::{Layout, Zoom};
use crate::{BorlandRand, Canvas, Sprite, strip};
use gboh_formats::eif::Eif;
use gboh_formats::map::Map;

/// Map hex feature codes that scatter objects.
const TREES: u16 = 1;
const STONES: u16 = 2;

/// Zoom-in hex face the scatter offsets are generated in.
const FACE: (i32, i32) = (130, 64);

/// Which overlay strip an object is drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectArt {
    Trees,
    Stones,
    /// `Stones (Green)`, used on maps with `unk_b == 2`.
    GreenStones,
}

impl ObjectArt {
    pub fn strip_name(self) -> &'static str {
        match self {
            ObjectArt::Trees => "Trees",
            ObjectArt::Stones => "Stones",
            ObjectArt::GreenStones => "Stones (Green)",
        }
    }
}

/// One scattered object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// Map hex.
    pub hex: (usize, usize),
    /// Offset from the hex centre in zoom-in pixels.
    pub offset: (i32, i32),
    pub art: ObjectArt,
    pub cel: usize,
}

/// Number of cels in every object strip.
const CELS: u32 = 7;

fn below(rng: &mut BorlandRand, n: i32) -> i32 {
    if n == 0 { 0 } else { (rng.next() as i32) % n }
}

/// Scatter the objects of every hex the way the original does when it
/// builds the landscape (`FUN_0044f689`), consuming `rand()` in the same
/// order per hex. Stones in elevation-0 hexes become block overlays in the
/// original and are not produced here.
pub fn scatter(map: &Map, rng: &mut BorlandRand) -> Vec<Placement> {
    let (w, h) = FACE;
    let base = (-w / 2, 0);
    let stones = if map.unk_b == 2 { ObjectArt::GreenStones } else { ObjectArt::Stones };
    let mut out = Vec::new();
    for x in 0..map.width {
        for y in 0..map.height {
            let hex = (x, y);
            for &feature in map.hex_features(x, y) {
                match feature {
                    TREES => {
                        // up to six thirds-of-the-face slots, then random ones
                        let (sw, sh) = (w / 3, h / 4);
                        let extra = rng.next() as i32 % 3 + 6;
                        for n in 0..=extra {
                            let slot = if n > 5 { (below(rng, 6) + n) % 6 } else { n };
                            let ox = below(rng, sw) + (slot % 3) * sw;
                            let oy = below(rng, sh) + (slot / 3) * (h / 3);
                            let cel = rng.next() % CELS;
                            out.push(Placement {
                                hex,
                                offset: (base.0 + ox, base.1 + oy),
                                art: ObjectArt::Trees,
                                cel: cel as usize,
                            });
                        }
                        out.push(Placement { hex, offset: (0, h / 3), art: ObjectArt::Trees, cel: 0 });
                    }
                    STONES if map.elevation_at(x, y) > 0 => {
                        let extra = rng.next() as i32 % 4 + 4;
                        for _ in 0..=extra {
                            let ox = below(rng, w) + w / 4;
                            let oy = below(rng, h / 2);
                            let cel = rng.next() % CELS;
                            out.push(Placement { hex, offset: (base.0 + ox, base.1 + oy), art: stones, cel: cel as usize });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// A sprite with its hotspot.
type Cel = (Sprite, (i32, i32));

/// Overlay art for one zoom (`LANDOVER`/`LANDOVR2`/`LANDOVR4`), with each
/// cel's hotspot.
pub struct ObjectSprites {
    strips: Vec<(ObjectArt, Vec<Cel>)>,
}

impl ObjectSprites {
    pub fn load(eif: &Eif) -> Self {
        let strips = [ObjectArt::Trees, ObjectArt::Stones, ObjectArt::GreenStones]
            .into_iter()
            .filter_map(|art| {
                let s = eif.strips.iter().find(|s| s.name == art.strip_name())?;
                let sprites = strip(eif, art.strip_name())?;
                let hot = s.cels.iter().map(|c| (c.hot_x as i32, c.hot_y as i32));
                Some((art, sprites.into_iter().zip(hot).collect()))
            })
            .collect();
        Self { strips }
    }

    fn get(&self, art: ObjectArt, cel: usize) -> Option<&Cel> {
        self.strips.iter().find(|(a, _)| *a == art)?.1.get(cel)
    }
}

impl Zoom {
    /// The terrain object art file.
    pub fn objects_file(self) -> &'static str {
        match self {
            Zoom::In => "LANDOVER.EIF",
            Zoom::Normal => "LANDOVR2.EIF",
            Zoom::Out => "LANDOVR4.EIF",
        }
    }

    /// Divisor from zoom-in pixels.
    fn scale(self) -> i32 {
        match self {
            Zoom::In => 1,
            Zoom::Normal => 2,
            Zoom::Out => 4,
        }
    }
}

/// Screen anchor (where a sprite's hotspot goes) for a physical object on
/// map hex `hex` at `offset` zoom-in pixels from the hex centre: the centre
/// of the hex's block plus the offset scaled to the zoom.
pub fn anchor_at(map: &Map, layout: &Layout, hex: (usize, usize), offset: (i32, i32)) -> (i32, i32) {
    let zoom = layout.zoom;
    let (w, h, _) = zoom.block();
    let (x, y) = hex;
    let (r, c) = layout.to_tile(x, y);
    let (bx, by) = layout.block_origin(r, c);
    let z = map.elevation_at(x, y) as i32;
    let s = zoom.scale();
    (bx + w / 2 + offset.0 / s, by - z * zoom.level_step() + h / 2 + offset.1 / s)
}

/// One sprite to draw in the depth-sorted object pass.
pub struct Drawable<'a> {
    pub anchor: (i32, i32),
    /// Tie-break among sprites on the same row (lower first).
    pub depth: i32,
    pub sprite: &'a Sprite,
    pub hot: (i32, i32),
}

/// Drawables for scattered terrain objects.
pub fn drawables<'a>(
    map: &Map,
    layout: &Layout,
    sprites: &'a ObjectSprites,
    objects: &[Placement],
) -> Vec<Drawable<'a>> {
    objects
        .iter()
        .filter_map(|p| {
            let (sprite, hot) = sprites.get(p.art, p.cel)?;
            Some(Drawable { anchor: anchor_at(map, layout, p.hex, p.offset), depth: 0, sprite, hot: *hot })
        })
        .collect()
}

/// Draw back to front: by anchor row, then depth, then list order. Shadow
/// pixels go through `shadow`. (The original's exact ordering rule is not
/// yet confirmed.)
pub fn draw(canvas: &mut Canvas, mut items: Vec<Drawable>, shadow: &[u8; 256]) {
    items.sort_by_key(|d| (d.anchor.1, d.depth));
    for d in items {
        canvas.blit_shadowed(d.sprite, d.anchor.0 - d.hot.0, d.anchor.1 - d.hot.1, shadow);
    }
}
