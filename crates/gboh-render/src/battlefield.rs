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
/// art's first strip; and for the reversed view, the second strip's.
struct Town {
    title: &'static str,
    hex: (usize, usize),
    offsets: &'static [(i32, i32)],
    reversed: ((usize, usize), &'static [(i32, i32)]),
}

/// Towns by battle (see [`battle_index`]).
fn town(map_file: &str) -> Option<Town> {
    match battle_index(map_file)? {
        0 => Some(Town { title: "Chaeronea", hex: (0, 16), offsets: &[(-65, 79)], reversed: ((2, 2), &[(380, 51)]) }),
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
            // the table at 0x618334
            reversed: (
                (6, 2),
                &[(26, 42), (209, 39), (310, 39), (366, 86), (259, 151), (194, 197), (64, 202), (-1, 154), (-66, 106), (180, 124), (212, 104)],
            ),
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
    /// The landscape background (`LANDBACK.EIF`) and where its tiling
    /// starts on the terrain: it carries on past the battlefield's edges.
    pub background: Option<(crate::Sprite, (i32, i32))>,
    objects: ObjectSprites,
    placed: Vec<objects::Placement>,
    town: Option<TownArt>,
    unit_art: HashMap<usize, Option<UnitSprites>>,
    flag_art: Option<units::FlagSprites>,
    missile_art: Vec<(crate::missiles::Missile, crate::missiles::MissileSprites)>,
    runaway_art: Vec<(crate::runaways::Runaway, UnitSprites)>,
    /// `Generic Rout Debris`: arms for units whose art has none.
    rout_debris: Option<units::DebrisSprites>,
    missile_debris: Option<crate::missiles::DebrisSprites>,
    /// `WALKANIM.EIF`: how each man walks.
    pub walk: Option<gboh_formats::walkanim::WalkAnim>,
    /// The hex grid (`Hex Grid` art: 16 cels by the block's neighbour
    /// mask), and whether it shows.
    grid: Vec<crate::Sprite>,
    pub grid_on: bool,
    /// The flags' pennants' animation cel.
    pub flag_frame: usize,
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
        Self::build_view(map_file, map, zoom, false, pal, seed, types, load, load_title)
    }

    /// As [`Scene::build`], in the reversed view if `reversed`.
    #[allow(clippy::too_many_arguments)]
    pub fn build_view(
        map_file: &str,
        map: &Map,
        zoom: Zoom,
        reversed: bool,
        pal: &Palette,
        seed: u32,
        types: &[usize],
        load: &mut Loader,
        load_title: &mut Loader,
    ) -> Result<Self, Error> {
        let layout = if reversed { Layout::reversed(map, zoom) } else { Layout::new(map, zoom) };
        let blocks = eif(load, zoom.blocks_file())?;
        let artwork = eif(load, zoom.artwork_file())?;
        let cliffs = if map.unk_b == 2 { eif(load, zoom.green_artwork_file())? } else { artwork.clone() };
        let mut art =
            TerrainArt::load(&blocks, &artwork, &cliffs).ok_or_else(|| Error::Strips(zoom.blocks_file().into()))?;
        let overlay = eif(load, zoom.objects_file())?;
        art.river_rocks = strip(&overlay, "River Impass").unwrap_or_default();
        let back = eif(load, "LANDBACK.EIF")?;
        art.background = strip(&back, "Blocks").and_then(|s| s.into_iter().next());
        let terrain = terrain::render_view(map, &art, layout, strip_offset(map_file, map), seed);
        let background = art.background.clone().map(|b| (b, layout.block_origin(0, 0)));
        let placed = objects::scatter(map, &mut BorlandRand::new(seed));
        let suffix = match zoom {
            Zoom::In => "",
            Zoom::Normal => " (Half size)",
            Zoom::Out => " (Quarter size)",
        };
        let town = town(map_file).and_then(|mut t| {
            let eif = Eif::parse(&load_title(&format!("{}{suffix}", t.title))?).ok()?;
            if reversed {
                (t.hex, t.offsets) = t.reversed;
            }
            let cels = &eif.strips.get(reversed as usize)?.cels;
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
        let missile_art = crate::missiles::Missile::ALL
            .into_iter()
            .filter_map(|m| {
                let eif = Eif::parse(&load_title(&format!("{}{suffix}", m.title()))?).ok()?;
                Some((m, crate::missiles::MissileSprites::load(&eif)?))
            })
            .collect();
        let runaway_art = crate::runaways::Runaway::ALL
            .into_iter()
            .filter_map(|r| {
                let eif = Eif::parse(&load_title(&format!("{}{suffix}", r.title()))?).ok()?;
                Some((r, UnitSprites::load(&eif)?))
            })
            .collect();
        let rout_debris = load_title(&format!("Generic Rout Debris{suffix}"))
            .and_then(|d| Eif::parse(&d).ok())
            .map(|e| units::DebrisSprites::load(&e));
        let missile_debris = load_title(&format!("Missile Debris{suffix}"))
            .and_then(|d| Eif::parse(&d).ok())
            .map(|e| crate::missiles::DebrisSprites::load(&e));
        let walk = load("WALKANIM.EIF").and_then(|d| gboh_formats::walkanim::WalkAnim::parse(&d).ok());
        let grid = load_title(&format!("Hex Grid{suffix}"))
            .and_then(|d| Eif::parse(&d).ok())
            .and_then(|e| e.strips.first().and_then(|s| strip(&e, &s.name)))
            .unwrap_or_default();
        Ok(Self {
            map: map.clone(),
            layout,
            terrain,
            background,
            objects: ObjectSprites::load(&overlay),
            placed,
            town,
            unit_art,
            flag_art,
            missile_art,
            runaway_art,
            rout_debris,
            missile_debris,
            walk,
            grid,
            grid_on: false,
            flag_frame: 0,
            seed,
            shadow: pal.remap[0],
        })
    }

    /// Milliseconds a step in screen direction `dir` takes a unit of type
    /// `type_id` facing screen direction `facing`, by the walking data.
    pub fn step_ms(&self, type_id: usize, facing: usize, dir: usize) -> Option<i32> {
        self.walk.as_ref()?.kind(units::walk_kind(type_id))?.step_ms(facing, dir)
    }

    /// As [`Scene::step_ms`], for a step from map hex `from` to `to` (the
    /// variant for going up, down or on the level).
    pub fn step_ms_between(&self, type_id: usize, facing: usize, dir: usize, from: (usize, usize), to: (usize, usize)) -> Option<i32> {
        let variant = gboh_formats::walkanim::Kind::variant(self.map.elevation_at(from.0, from.1), self.map.elevation_at(to.0, to.1));
        self.walk.as_ref()?.kind(units::walk_kind(type_id))?.step_ms_variant(facing, dir, variant)
    }

    /// The screen facing (1 N … 8 NW) a unit facing `facing` shows in this
    /// view.
    pub fn screen_facing(&self, facing: u8) -> usize {
        units::screen_facing(self.layout.facing(facing))
    }

    /// The sprite set of unit type `type_id`, if loaded.
    pub fn unit_sprites(&self, type_id: usize) -> Option<&UnitSprites> {
        self.unit_art.get(&type_id)?.as_ref()
    }

    /// The battlefield with `units` on it, depth sorted with trees, stones
    /// and the town; units whose id is in `flagged` show their activation
    /// flag.
    pub fn compose(&self, units: &[UnitView], flagged: &[usize]) -> Canvas {
        self.compose_with(units, flagged, &[])
    }

    /// As `compose`, with fallen men lying where they fell.
    pub fn compose_with(&self, units: &[UnitView], flagged: &[usize], corpses: &[Corpse]) -> Canvas {
        self.compose_all(units, flagged, corpses, &[], &[])
    }

    /// As `compose_with`, with runaways (each its path, how many hexes
    /// along and its walking frame) and missiles in flight drawn over
    /// everything.
    pub fn compose_all(
        &self,
        units: &[UnitView],
        flagged: &[usize],
        corpses: &[Corpse],
        flights: &[crate::missiles::Flight],
        runners: &[crate::runaways::Runner],
    ) -> Canvas {
        let (map, layout) = (&self.map, &self.layout);
        let mut canvas = self.terrain.clone();
        if self.grid_on && self.grid.len() >= 16 {
            // 🟡 the cel by the block's neighbour mask, as the blocks pick
            // theirs (the original's choice is not traced)
            let step = layout.zoom.level_step();
            for r in 0..layout.rows {
                for c in 0..layout.cols {
                    if c == 0 && !Layout::shifted(r) {
                        continue;
                    }
                    let mask = crate::terrain::block_mask(map, layout, r, c).unwrap_or(0);
                    let (x, y) = layout.block_origin(r, c);
                    let (mx, my) = layout.to_map(r, c);
                    canvas.blit(&self.grid[mask], x, y - map.elevation_at(mx, my) as i32 * step);
                }
            }
        }
        let mut items = objects::drawables(map, layout, &self.objects, &self.placed);
        for c in corpses {
            if c.kind >= units::MISSILE {
                let missile = crate::missiles::Missile::ALL[(c.kind - units::MISSILE).min(3)];
                if let Some(d) = self.missile_debris.as_ref().and_then(|art| {
                    art.drawable(missile, c.cel, objects::anchor_at(map, layout, c.hex, c.offset))
                }) {
                    items.push(d);
                }
                continue;
            }
            if let Some(Some(art)) = self.unit_art.get(&c.type_id)
                && let Some(d) = units::corpse_drawable_with(map, layout, c, art, self.rout_debris.as_ref())
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
                let kind = self.walk.as_ref().and_then(|w| w.kind(units::walk_kind(u.type_id)));
                items.extend(units::drawables_with(map, layout, u, sprites, kind));
            }
        }
        for &(kind, path, along, frame) in runners {
            if let Some((_, art)) = self.runaway_art.iter().find(|(r, _)| *r == kind)
                && let Some(d) = crate::runaways::drawable(map, layout, art, path, along, frame)
            {
                items.push(d);
            }
        }
        if let Some(art) = &self.flag_art {
            // every unit draws its flag's sideways jitter when it is built,
            // in scenario order
            let mut rng = BorlandRand::new(self.seed);
            let last = units.iter().map(|u| u.id).max().map_or(0, |m| m + 1);
            let jitters: Vec<i32> = (0..last).map(|_| 5 - (rng.next() % 11) as i32).collect();
            for u in units.iter().filter(|u| flagged.contains(&u.id)) {
                items.extend(units::flag_drawables(map, layout, u, jitters[u.id], art, self.flag_frame));
            }
        }
        objects::draw(&mut canvas, items, &self.shadow);
        let missiles: Vec<_> = flights
            .iter()
            .filter_map(|f| f.drawable(&self.missile_art.iter().find(|(m, _)| *m == f.missile)?.1))
            .collect();
        objects::draw(&mut canvas, missiles, &self.shadow);
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
