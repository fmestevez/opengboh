//! A whole battlefield (terrain, terrain objects and units) drawn from the
//! original data files.

use crate::objects::{self, ObjectSprites};
use crate::units::{self, UnitSprites};
use crate::terrain::{self, Layout, TerrainArt, Zoom};
use crate::{BorlandRand, Canvas, strip};
use gboh_formats::eif::Eif;
use gboh_formats::map::Map;
use gboh_formats::pal::Palette;
use gboh_formats::scn::Unit;
use gboh_formats::units::{ART_TITLES, unit_type};
use std::collections::HashMap;

/// Map files in the order of the original's battle table (`0x5e4394`).
const BATTLES: [&str; 11] = [
    "CHAERONE", "LYGINUS", "PELIUM", "GRANICUS", "ISSUS", "GAUGAMEL", "SAMARKAN", "JAXARTES",
    "ARIGAEUM", "HYDASPES", "PARAIT",
];

/// Block strip offset the original picks per battle (`FUN_00506e5e`).
pub fn strip_offset(map_file: &str, map: &Map) -> usize {
    let stem = map_file.split('.').next().unwrap_or("").to_ascii_uppercase();
    match BATTLES.iter().position(|b| *b == stem) {
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

/// Towns by battle (index into [`BATTLES`]).
fn town(map_file: &str) -> Option<Town> {
    let stem = map_file.split('.').next().unwrap_or("").to_ascii_uppercase();
    match BATTLES.iter().position(|b| *b == stem)? {
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

/// Draw `map` (loaded from `map_file`) at `zoom`: terrain, then trees,
/// stones, the battle's town (if any) and `units` depth sorted together. Shadows use `pal`'s remap table
/// 0. `seed` drives the random art choices. `load_title` reads an EIF file by
/// its title (unit sprite sets are named by title); units whose art is
/// missing are skipped.
#[allow(clippy::too_many_arguments)]
pub fn render(
    map_file: &str,
    map: &Map,
    zoom: Zoom,
    pal: &Palette,
    seed: u32,
    units: &[Unit],
    load: &mut Loader,
    load_title: &mut Loader,
) -> Result<Canvas, Error> {
    let blocks = eif(load, zoom.blocks_file())?;
    let artwork = eif(load, zoom.artwork_file())?;
    let cliffs = if map.unk_b == 2 { eif(load, zoom.green_artwork_file())? } else { artwork.clone() };
    let mut art =
        TerrainArt::load(&blocks, &artwork, &cliffs).ok_or_else(|| Error::Strips(zoom.blocks_file().into()))?;
    let overlay = eif(load, zoom.objects_file())?;
    art.river_rocks = strip(&overlay, "River Impass").unwrap_or_default();
    let back = eif(load, "LANDBACK.EIF")?;
    art.background = strip(&back, "Blocks").and_then(|s| s.into_iter().next());
    let mut canvas = terrain::render(map, &art, zoom, strip_offset(map_file, map), seed);
    let placed = objects::scatter(map, &mut BorlandRand::new(seed));
    let layout = Layout::new(map, zoom);
    let object_sprites = ObjectSprites::load(&overlay);
    let mut items = objects::drawables(map, &layout, &object_sprites, &placed);

    let suffix = match zoom {
        Zoom::In => "",
        Zoom::Normal => " (Half size)",
        Zoom::Out => " (Quarter size)",
    };
    let town_art = town(map_file).and_then(|t| {
        let eif = Eif::parse(&load_title(&format!("{}{suffix}", t.title))?).ok()?;
        let cels = &eif.strips.first()?.cels;
        let sprites: Vec<_> = cels
            .iter()
            .filter_map(|c| Some((crate::Sprite::from(c.decode().ok()?), (c.hot_x as i32, c.hot_y as i32))))
            .collect();
        Some((t, sprites))
    });
    if let Some((t, sprites)) = &town_art {
        for ((sprite, hot), &offset) in sprites.iter().zip(t.offsets) {
            items.push(objects::Drawable { anchor: objects::anchor_at(map, &layout, t.hex, offset), depth: 0, sprite, hot: *hot });
        }
    }
    let mut unit_art: HashMap<usize, Option<UnitSprites>> = HashMap::new();
    let typed: Vec<_> = units.iter().filter_map(|u| Some((u, unit_type(u)?))).collect();
    for &(_, ty) in &typed {
        unit_art.entry(ty).or_insert_with(|| {
            let title = format!("{}{suffix}", ART_TITLES[ty]);
            load_title(&title).and_then(|d| Eif::parse(&d).ok()).and_then(|e| UnitSprites::load(&e))
        });
    }
    for &(u, ty) in &typed {
        if let Some(Some(sprites)) = unit_art.get(&ty) {
            items.extend(units::drawables(map, &layout, u, ty, sprites));
        }
    }
    objects::draw(&mut canvas, items, &pal.remap[0]);
    Ok(canvas)
}
