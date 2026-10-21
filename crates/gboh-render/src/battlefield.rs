//! A whole battlefield (terrain, terrain objects and units) drawn from the
//! original data files.

use crate::objects::{self, ObjectSprites};
use crate::units::{self, Corpse, UnitSprites, UnitView};
use crate::terrain::{self, Layout, TerrainArt, Zoom};
use crate::{BorlandRand, Canvas, strip};
use gboh_formats::eif::Eif;
use gboh_formats::map::Map;
use gboh_formats::pal::Palette;
use gboh_formats::scn::{Unit, battle_index};
use gboh_formats::units::ART_TITLES;
use std::collections::HashMap;

/// Block strip offset the original picks per battle (`FUN_00506e5e`).
pub fn strip_offset(map_file: &str, map: &Map) -> usize {
    match battle_index(map_file) {
        _ if map.unk_b == 2 => 0,
        Some(0 | 1) => 1,
        Some(5) => 3,
        Some(8) => 4,
        _ => 0,
    }
}

/// A town the original places as physical objects in one battle: art
/// title, hex (map `x`, `y`), and the zoom-in offset of each cel of the
/// art's first strip (the second strip is the reversed view).
struct Town {
    title: &'static str,
    hex: (usize, usize),
    offsets: &'static [(i32, i32)],
}

/// Towns by battle (see [`battle_index`]).
fn town(map_file: &str) -> Option<Town> {
    match battle_index(map_file)? {
        0 => Some(Town { title: "Chaeronea", hex: (0, 16), offsets: &[(-65, 79)] }),
        2 => Some(Town {
            title: "Pelium",
            hex: (1, 3),
            offsets: &[
                (-119, 142),
                (-66, 63),
                (4, 53),
                (130, 81),
                (193, 152),
                (227, 191),
                (-196, 195),
                (-145, 242),
                (-66, 237),
                (64, 237),
                (-108, 170),
                (4, 131),
            ],
        }),
        _ => None,
    }
}

/// Why a battlefield could not be built.
#[derive(Debug)]
pub enum Error {
    /// A data file could not be read.
    Missing(String),
    /// A data file did not parse.
    Format(String, gboh_formats::Error),
    /// A file lacks strips the renderer needs.
    Strips(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Missing(name) => write!(f, "{name} not found"),
            Error::Format(name, e) => write!(f, "{name}: {e}"),
            Error::Strips(name) => write!(f, "{name}: missing terrain strips"),
        }
    }
}

impl std::error::Error for Error {}

/// Reads a data file by name (case-insensitive in the original's `DATA`
/// directory), or an EIF file by title.
pub type Loader<'a> = dyn FnMut(&str) -> Option<Vec<u8>> + 'a;

/// Index the EIF files in a directory listing by title (the first 128
/// bytes, NUL-terminated). The first file wins when titles repeat.
pub fn eif_titles<'a>(files: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (name, head) in files {
        let title: String = head.iter().take(128).take_while(|&&b| b != 0).map(|&b| b as char).collect();
        out.entry(title).or_insert_with(|| name.to_string());
    }
    out
}

fn eif(load: &mut Loader, name: &str) -> Result<Eif, Error> {
    let data = load(name).ok_or_else(|| Error::Missing(name.into()))?;
    Eif::parse(&data).map_err(|e| Error::Format(name.into(), e))
}

/// The game palette as used on the battlefield: `GAME.PAL` with water
/// animation frame 0 applied.
pub fn palette(load: &mut Loader) -> Result<Palette, Error> {
    let data = load("GAME.PAL").ok_or_else(|| Error::Missing("GAME.PAL".into()))?;
    let mut pal = Palette::parse(&data).map_err(|e| Error::Format("GAME.PAL".into(), e))?;
    crate::animate_water(&mut pal, 0);
    Ok(pal)
}

/// A town's placement and its cels with hotspots.
type TownArt = (Town, Vec<(crate::Sprite, (i32, i32))>);

/// A battlefield ready to draw: its terrain (drawn once) and the art for
/// the objects drawn over it each frame.
pub struct Scene {
    pub map: Map,
    pub layout: Layout,
    pub terrain: Canvas,
    objects: ObjectSprites,
    placed: Vec<objects::Placement>,
    town: Option<TownArt>,
    unit_art: HashMap<usize, Option<UnitSprites>>,
    flag_art: Option<units::FlagSprites>,
    seed: u32,
    shadow: [u8; 256],
}

impl Scene {
    /// Load the art and draw the terrain of `map` (loaded from `map_file`)
    /// at `zoom`, with the sprite sets of unit types `types`. Shadows use
    /// `pal`'s remap table 0; `seed` drives the random art choices.
    /// `load_title` reads an EIF file by its title.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        map_file: &str,
        map: &Map,
        zoom: Zoom,
        pal: &Palette,
        seed: u32,
        types: &[usize],
        load: &mut Loader,
        load_title: &mut Loader,
    ) -> Result<Self, Error> {
        let blocks = eif(load, zoom.blocks_file())?;
        let artwork = eif(load, zoom.artwork_file())?;
        let cliffs = if map.unk_b == 2 { eif(load, zoom.green_artwork_file())? } else { artwork.clone() };
        let mut art =
            TerrainArt::load(&blocks, &artwork, &cliffs).ok_or_else(|| Error::Strips(zoom.blocks_file().into()))?;
        let overlay = eif(load, zoom.objects_file())?;
        art.river_rocks = strip(&overlay, "River Impass").unwrap_or_default();
        let back = eif(load, "LANDBACK.EIF")?;
        art.background = strip(&back, "Blocks").and_then(|s| s.into_iter().next());
        let terrain = terrain::render(map, &art, zoom, strip_offset(map_file, map), seed);
        let placed = objects::scatter(map, &mut BorlandRand::new(seed));
        let suffix = match zoom {
            Zoom::In => "",
            Zoom::Normal => " (Half size)",
            Zoom::Out => " (Quarter size)",
        };
        let town = town(map_file).and_then(|t| {
            let eif = Eif::parse(&load_title(&format!("{}{suffix}", t.title))?).ok()?;
            let cels = &eif.strips.first()?.cels;
            let sprites: Vec<_> = cels
                .iter()
                .filter_map(|c| Some((crate::Sprite::from(c.decode().ok()?), (c.hot_x as i32, c.hot_y as i32))))
                .collect();
            Some((t, sprites))
        });
        let mut unit_art = HashMap::new();
        for &ty in types {
            unit_art.entry(ty).or_insert_with(|| {
                let title = format!("{}{suffix}", ART_TITLES[ty]);
                load_title(&title).and_then(|d| Eif::parse(&d).ok()).and_then(|e| UnitSprites::load(&e))
            });
        }
        let flag_art = load_title(&format!("Activation Unit Flags{suffix}"))
            .and_then(|d| Eif::parse(&d).ok())
            .and_then(|e| units::FlagSprites::load(&e));
        Ok(Self {
            map: map.clone(),
            layout: Layout::new(map, zoom),
            terrain,
            objects: ObjectSprites::load(&overlay),
            placed,
            town,
            unit_art,
            flag_art,
            seed,
            shadow: pal.remap[0],
        })
    }

    /// The battlefield with `units` on it, depth sorted with trees, stones
    /// and the town; units whose id is in `flagged` show their activation
    /// flag.
    pub fn compose(&self, units: &[UnitView], flagged: &[usize]) -> Canvas {
        self.compose_with(units, flagged, &[])
    }

    /// As `compose`, with fallen men lying where they fell.
    pub fn compose_with(&self, units: &[UnitView], flagged: &[usize], corpses: &[Corpse]) -> Canvas {
        let (map, layout) = (&self.map, &self.layout);
        let mut canvas = self.terrain.clone();
        let mut items = objects::drawables(map, layout, &self.objects, &self.placed);
        for c in corpses {
            if let Some(Some(art)) = self.unit_art.get(&c.type_id)
                && let Some(d) = units::corpse_drawable(map, layout, c, art)
            {
                items.push(d);
            }
        }
        if let Some((t, sprites)) = &self.town {
            for ((sprite, hot), &offset) in sprites.iter().zip(t.offsets) {
                items.push(objects::Drawable { anchor: objects::anchor_at(map, layout, t.hex, offset), depth: 0, sprite, hot: *hot });
            }
        }
        for u in units {
            if let Some(Some(sprites)) = self.unit_art.get(&u.type_id) {
                items.extend(units::drawables(map, layout, u, sprites));
            }
        }
        if let Some(art) = &self.flag_art {
            // every unit draws its flag's sideways jitter when it is built,
            // in scenario order
            let mut rng = BorlandRand::new(self.seed);
            let last = units.iter().map(|u| u.id).max().map_or(0, |m| m + 1);
            let jitters: Vec<i32> = (0..last).map(|_| 5 - (rng.next() % 11) as i32).collect();
            for u in units.iter().filter(|u| flagged.contains(&u.id)) {
                items.extend(units::flag_drawables(map, layout, u, jitters[u.id], art));
            }
        }
        objects::draw(&mut canvas, items, &self.shadow);
        canvas
    }
}

/// Draw `map` (loaded from `map_file`) at `zoom`: terrain, then trees,
/// stones, the battle's town (if any) and `units` depth sorted together.
/// Units whose index is in `flagged` get their activation flag raised.
#[allow(clippy::too_many_arguments)]
pub fn render(
    map_file: &str,
    map: &Map,
    zoom: Zoom,
    pal: &Palette,
    seed: u32,
    units: &[Unit],
    flagged: &[usize],
    load: &mut Loader,
    load_title: &mut Loader,
) -> Result<Canvas, Error> {
    let views: Vec<UnitView> = units.iter().enumerate().filter_map(|(i, u)| UnitView::of_scenario(map, i, u)).collect();
    let types: Vec<usize> = views.iter().map(|v| v.type_id).collect();
    let scene = Scene::build(map_file, map, zoom, pal, seed, &types, load, load_title)?;
    Ok(scene.compose(&views, flagged))
}
