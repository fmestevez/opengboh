//! Command-line utilities for the original game data.
//!
//! * `extract <DATA dir> <out dir>`: convert every EIF sprite file into PNGs
//!   plus a `manifest.json` per file with names and hotspots.
//! * `maps <DATA dir>`: parse every battle map and print a summary.
//! * `scenarios <DATA dir>`: parse every scenario and check units fit the map.
//! * `units <DATA dir> <SCN file>`: list a scenario's units.
//! * `render <DATA dir> <MAP or SCN file> <out.png> [in|normal|out]`: draw a
//!   map (or a scenario's map with its units) with the faithful renderer.

use gboh_formats::{eif::Eif, map::Map, pal::Palette, scn::Scenario};
use gboh_formats::units::{ART_TITLES, unit_type};
use gboh_render::battlefield;
use gboh_render::terrain::Zoom;
use std::collections::BTreeMap;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct CelInfo {
    file: String,
    name: String,
    width: u16,
    height: u16,
    hot_x: u16,
    hot_y: u16,
    kind: u16,
}

#[derive(Serialize)]
struct StripInfo {
    name: String,
    cels: Vec<CelInfo>,
}

#[derive(Serialize)]
struct Manifest {
    title: String,
    strips: Vec<StripInfo>,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("extract") if args.len() == 4 => extract(Path::new(&args[2]), Path::new(&args[3])),
        Some("maps") if args.len() == 3 => maps(Path::new(&args[2])),
        Some("scenarios") if args.len() == 3 => scenarios(Path::new(&args[2])),
        Some("units") if args.len() == 4 => units(Path::new(&args[2]), &args[3]),
        Some("render") if (5..=6).contains(&args.len()) => render(
            Path::new(&args[2]),
            &args[3],
            Path::new(&args[4]),
            args.get(5).map_or("normal", String::as_str),
        ),
        _ => bail!(
            "usage: gboh-tools extract <DATA dir> <out dir> | maps <DATA dir> | scenarios <DATA dir> | units <DATA dir> <SCN file> \
             | render <DATA dir> <MAP|SCN file> <out.png> [in|normal|out]"
        ),
    }
}

/// Case-insensitive lookup, since the shipped files mix `*.EIF` and `*.eif`.
fn find_ci(dir: &Path, name: &str) -> Result<PathBuf> {
    for e in fs::read_dir(dir)? {
        let p = e?.path();
        if p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(name)) {
            return Ok(p);
        }
    }
    bail!("{name} not found in {}", dir.display())
}

fn files_with_ext(dir: &Path, ext: &str) -> Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)))
        .collect();
    paths.sort();
    Ok(paths)
}

fn maps(data_dir: &Path) -> Result<()> {
    for path in files_with_ext(data_dir, "map")? {
        let file = path.file_name().unwrap().to_string_lossy();
        match Map::parse(&fs::read(&path)?) {
            Ok(m) => {
                let mut features: BTreeMap<u16, usize> = BTreeMap::new();
                for f in m.cells.iter().flatten() {
                    *features.entry(*f).or_default() += 1;
                }
                let water = m.elevation.iter().filter(|&&e| e == 0).count();
                println!(
                    "{file:14} {:12} {:2}x{:2} origin={} unk={} water={water:4} features={features:?}",
                    m.name, m.width, m.height, m.hex_id_origin, m.unk_b
                );
            }
            Err(e) => println!("{file:14} not a battle map: {e}"),
        }
    }
    Ok(())
}

fn scenarios(data_dir: &Path) -> Result<()> {
    for path in files_with_ext(data_dir, "scn")? {
        let file = path.file_name().unwrap().to_string_lossy();
        let scn = Scenario::parse(&fs::read(&path)?).with_context(|| file.to_string())?;
        let map = Map::parse(&fs::read(find_ci(data_dir, &scn.map_file)?)?)?;
        let off_map = scn.units.iter().filter(|u| map.hex_from_id(u.hex).is_none()).count();
        let mut kinds: BTreeMap<u8, usize> = BTreeMap::new();
        for u in &scn.units {
            *kinds.entry(u.kind).or_default() += 1;
        }
        println!(
            "{file:13} {:28} map={:13} {}x{} leaders={:2} units={:3} off_map={off_map} ai={}+{} kinds={kinds:?}",
            scn.title,
            scn.map_file,
            map.width,
            map.height,
            scn.leaders.len(),
            scn.units.len(),
            scn.home_ai.len(),
            scn.visitor_ai.len()
        );
    }
    Ok(())
}

fn units(data_dir: &Path, scn_file: &str) -> Result<()> {
    let scn = Scenario::parse(&fs::read(find_ci(data_dir, scn_file)?)?)?;
    let map = Map::parse(&fs::read(find_ci(data_dir, &scn.map_file)?)?)?;
    for u in &scn.units {
        let hex = map.hex_from_id(u.hex);
        let art = unit_type(u).map_or("?", |t| ART_TITLES[t]);
        println!(
            "kind={:2} nation={} a={} b={} hex={} {:?} facing={} {:24} {art:36} {:?}",
            u.kind, u.nation, u.unk_a, u.unk_b, u.hex, hex, u.facing, u.name, u.extra
        );
    }
    Ok(())
}

fn sanitize(s: &str) -> String {
    let s: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if s.is_empty() { "_".into() } else { s }
}

fn extract(data_dir: &Path, out_dir: &Path) -> Result<()> {
    let pal = Palette::parse(&fs::read(find_ci(data_dir, "GAME.PAL")?)?)?;
    let mut rgba_pal = [[0u8; 4]; 256];
    for (i, c) in pal.colors.iter().enumerate() {
        rgba_pal[i] = [c[0], c[1], c[2], if i == 0 { 0 } else { 255 }];
    }

    let paths = files_with_ext(data_dir, "eif")?;

    let (mut files_ok, mut cels_ok, mut failures) = (0, 0, Vec::new());
    for path in &paths {
        let stem = path.file_stem().unwrap().to_string_lossy().to_uppercase();
        let eif = match Eif::parse(&fs::read(path)?) {
            Ok(e) => e,
            Err(e) => {
                failures.push(format!("{stem}: {e}"));
                continue;
            }
        };
        let dir = out_dir.join(&stem);
        fs::create_dir_all(&dir)?;
        let mut manifest = Manifest { title: eif.title.clone(), strips: vec![] };
        for (si, strip) in eif.strips.iter().enumerate() {
            let mut info = StripInfo { name: strip.name.clone(), cels: vec![] };
            for (ci, cel) in strip.cels.iter().enumerate() {
                let img = match cel.decode() {
                    Ok(p) => p,
                    Err(e) => {
                        failures.push(format!("{stem} strip {si} cel {ci}: {e}"));
                        continue;
                    }
                };
                let file = format!("{si:02}_{}_{ci:02}.png", sanitize(&strip.name));
                // Effect spans, and index 255 in raw unit sprites, are shadows the
                // game draws through a darkening remap table; export them as
                // translucent black.
                let shadow_idx = (cel.kind == 3).then_some(255u8);
                let rgba: Vec<u8> = img
                    .pixels
                    .iter()
                    .zip(&img.effect)
                    .flat_map(|(&i, &fx)| {
                        if fx != 0 || Some(i) == shadow_idx { [0, 0, 0, 128] } else { rgba_pal[i as usize] }
                    })
                    .collect();
                write_png(&dir.join(&file), cel.width as u32, cel.height as u32, &rgba)
                    .with_context(|| format!("{stem}/{file}"))?;
                info.cels.push(CelInfo {
                    file,
                    name: cel.name.clone(),
                    width: cel.width,
                    height: cel.height,
                    hot_x: cel.hot_x,
                    hot_y: cel.hot_y,
                    kind: cel.kind,
                });
                cels_ok += 1;
            }
            manifest.strips.push(info);
        }
        fs::write(dir.join("manifest.json"), serde_json::to_string_pretty(&manifest)?)?;
        files_ok += 1;
    }

    println!("{files_ok}/{} EIF files, {cels_ok} cels extracted", paths.len());
    for f in &failures {
        eprintln!("  failed: {f}");
    }
    Ok(())
}

/// Battle order in the original's scenario table (string ids 93..102, 80).
fn render(data_dir: &Path, file: &str, out: &Path, zoom: &str) -> Result<()> {
    let zoom = match zoom {
        "in" => Zoom::In,
        "normal" => Zoom::Normal,
        "out" => Zoom::Out,
        z => bail!("unknown zoom {z}"),
    };
    let mut load = |name: &str| find_ci(data_dir, name).ok().and_then(|p| fs::read(p).ok());
    // a scenario brings its map and units
    let (map_file, units) = if file.to_ascii_lowercase().ends_with(".scn") {
        let scn = Scenario::parse(&load(file).with_context(|| format!("{file} not found"))?)?;
        (scn.map_file, scn.units)
    } else {
        (file.to_string(), Vec::new())
    };
    let map = Map::parse(&load(&map_file).with_context(|| format!("{map_file} not found"))?)?;
    let pal = battlefield::palette(&mut load)?;
    let titles = eif_title_index(data_dir)?;
    let mut load_title =
        |title: &str| titles.get(title).and_then(|n| fs::read(data_dir.join(n)).ok());
    let canvas = battlefield::render(&map_file, &map, zoom, &pal, 1, &units, &mut load, &mut load_title)?;
    write_png(out, canvas.width as u32, canvas.height as u32, &canvas.to_rgba(&pal))
}

/// EIF file names in `dir` by title.
fn eif_title_index(dir: &Path) -> Result<std::collections::HashMap<String, String>> {
    let mut heads = Vec::new();
    for path in files_with_ext(dir, "eif")? {
        let mut head = vec![0u8; 128];
        use std::io::Read;
        let n = fs::File::open(&path)?.read(&mut head)?;
        head.truncate(n);
        heads.push((path.file_name().unwrap().to_string_lossy().into_owned(), head));
    }
    Ok(battlefield::eif_titles(heads.iter().map(|(n, h)| (n.as_str(), h.as_slice()))))
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let mut enc = png::Encoder::new(fs::File::create(path)?, w.max(1), h.max(1));
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header()?;
    if w == 0 || h == 0 {
        wr.write_image_data(&[0; 4])?;
    } else {
        wr.write_image_data(rgba)?;
    }
    Ok(())
}
