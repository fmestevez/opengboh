//! Command-line utilities for the original game data.
//!
//! * `extract <DATA dir> <out dir>`: convert every EIF sprite file into PNGs
//!   plus a `manifest.json` per file with names and hotspots.
//! * `maps <DATA dir>`: parse every battle map and print a summary.
//! * `scenarios <DATA dir>`: parse every scenario and check units fit the map.
//! * `units <DATA dir> <SCN file>`: list a scenario's units.
//! * `command <DATA dir> <SCN file>`: a sample turn: leaders in activation
//!   order and the units each commands.
//! * `moves <DATA dir> <SCN file> <unit name> [l|r...]`: hexes a unit can
//!   reach, optionally after turning it left or right.
//! * `turns <DATA dir> <SCN file> <n>`: play n turns with no orders (the
//!   turn loop: activation order, routs, victory check).
//! * `ai <DATA dir> <SCN file> <n> [v] [home|visitor] [plan=N]`: armies
//!   played by the original's AI scripts for n turns; `v` prints the
//!   scripts' debug output, `home`/`visitor` plays only that army (the
//!   other gives no orders), `plan=N` uses each side's Nth plan, `save=F`
//!   starts from the original's saved game F (at its turn), `seed=N`
//!   seeds the dice.
//! * `strips <EIF file>`: list an art file's strips and cel counts.
//! * `render … [save=F]`: draw a saved game of the original instead of the
//!   scenario's opening (routed units in their routed pose).
//! * `render <DATA dir> <MAP or SCN file> <out.png> [in|normal|out]`: draw a
//!   map (or a scenario's map with its units) with the faithful renderer.

use anyhow::{Context, Result, bail};
use gboh_formats::units::{title, unit_type};
use gboh_formats::{eif::Eif, map::Map, pal::Palette, scn::Scenario};
use gboh_render::battlefield;
use gboh_render::terrain::Zoom;
use serde::Serialize;
use std::collections::BTreeMap;
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
        Some("strips") if args.len() == 3 => strips(Path::new(&args[2])),
        Some("maps") if args.len() == 3 => maps(Path::new(&args[2])),
        Some("scenarios") if args.len() == 3 => scenarios(Path::new(&args[2])),
        Some("units") if args.len() == 4 => units(Path::new(&args[2]), &args[3]),
        Some("command") if args.len() == 4 => command(Path::new(&args[2]), &args[3]),
        Some("turns") if args.len() == 5 => turns(Path::new(&args[2]), &args[3], args[4].parse()?),
        Some("ai") if args.len() >= 5 => ai(Path::new(&args[2]), &args[3], args[4].parse()?, &args[5..]),
        Some("moves") if (5..=6).contains(&args.len()) => moves(Path::new(&args[2]), &args[3], &args[4], args.get(5).map_or("", String::as_str)),
        Some("render") if (5..=10).contains(&args.len()) => render(
            Path::new(&args[2]),
            &args[3],
            Path::new(&args[4]),
            args.get(5).map_or("normal", String::as_str),
            args[5..].iter().find_map(|a| a.strip_prefix("save=")),
            args[5..].iter().any(|a| a == "static-water"),
            args[5..].iter().any(|a| a == "grid"),
            args[5..].iter().any(|a| a == "reversed"),
        ),
        _ => bail!(
            "usage: gboh-tools extract <DATA dir> <out dir> | strips <EIF file> | maps <DATA dir> | scenarios <DATA dir> | units <DATA dir> <SCN file> \
             | command <DATA dir> <SCN file> | turns <DATA dir> <SCN file> <n> \
             | moves <DATA dir> <SCN file> <unit name> [l|r...] | render <DATA dir> <MAP|SCN file> <out.png> [in|normal|out] [save=F] [static-water] [grid] [reversed]"
        ),
    }
}

/// Case-insensitive lookup, since the shipped files mix `*.EIF` and `*.eif`.
/// The game's palette file (`GAME.PAL`, or the game's own name for it).
fn palette_path(dir: &Path) -> Result<PathBuf> {
    gboh_formats::pal::FILES.iter().find_map(|n| find_ci(dir, n).ok()).context("GAME.PAL not found")
}

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
    let mut paths: Vec<PathBuf> =
        fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext))).collect();
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
        let scn = match Scenario::parse(&fs::read(&path)?) {
            Ok(s) => s,
            Err(e) => {
                println!("{file:13} not read: {e}");
                continue;
            }
        };
        let map = match find_ci(data_dir, &scn.map_file).and_then(|p| Ok(Map::parse(&fs::read(p)?)?)) {
            Ok(m) => m,
            Err(e) => {
                println!("{file:13} {:28} map={}: {e}", scn.title, scn.map_file);
                continue;
            }
        };
        let off_map = scn.units.iter().filter(|u| map.hex_from_id(u.hex).is_none()).count();
        let mut kinds: BTreeMap<u8, usize> = BTreeMap::new();
        for u in &scn.units {
            *kinds.entry(u.kind).or_default() += 1;
        }
        println!(
            "{file:13} {:28} map={:13} {}x{} leaders={:2} units={:3} off_map={off_map} late={} ai={}+{} kinds={kinds:?}",
            scn.title,
            scn.map_file,
            map.width,
            map.height,
            scn.leaders.len(),
            scn.units.len(),
            scn.units.iter().filter(|u| u.arrival > 0).count(),
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
        let art = unit_type(u).and_then(title).unwrap_or("?");
        println!(
            "kind={:2} nation={} a={} b={} hex={} {:?} facing={} {:24} {art:36} {:?}",
            u.kind, u.nation, u.unk_a, u.unk_b, u.hex, hex, u.facing, u.name, u.extra
        );
    }
    Ok(())
}

fn command(data_dir: &Path, scn_file: &str) -> Result<()> {
    use gboh_game::{activation::Activation, battle::Battle, command::commanded};
    let scn = Scenario::parse(&fs::read(find_ci(data_dir, scn_file)?)?)?;
    let map = Map::parse(&fs::read(find_ci(data_dir, &scn.map_file)?)?)?;
    let battle = Battle::from_scenario(&scn, &map);
    let mut activation = Activation::new(&battle, gboh_formats::scn::battle_index(&scn.map_file));
    let mut rng = gboh_render::BorlandRand::new(1);
    // one sample turn: leaders in activation order, no momentum
    let mut next = activation.begin_turn(&battle, &mut rng);
    while let Some(l) = next {
        let u = &battle.units[l];
        let r = u.leader.unwrap();
        println!("{} (initiative {}, range {}, line {}) at ({}, {})", u.name, r.initiative, r.command_range, r.personal_combat, u.hex.x, u.hex.y);
        for i in commanded(&battle, &map, l) {
            let c = &battle.units[i];
            println!("    {:24} group {:2} at ({}, {})", c.name, c.group, c.hex.x, c.hex.y);
        }
        next = activation.next(&battle, false, &mut rng);
    }
    Ok(())
}

fn moves(data_dir: &Path, scn_file: &str, name: &str, turns: &str) -> Result<()> {
    use gboh_game::{
        battle::Battle,
        movement::{highlighted, reachable},
        orders::turn,
    };
    let scn = Scenario::parse(&fs::read(find_ci(data_dir, scn_file)?)?)?;
    let map = Map::parse(&fs::read(find_ci(data_dir, &scn.map_file)?)?)?;
    let mut battle = Battle::from_scenario(&scn, &map);
    let Some(i) = battle.units.iter().position(|u| u.name == name) else { bail!("no unit named {name}") };
    // optional turns first: l = left, r = right
    for c in turns.chars() {
        turn(&mut battle, &map, i, c == 'l').map_err(|e| anyhow::anyhow!("turn refused: {e:?}"))?;
    }
    let u = &battle.units[i];
    let points = u.mp;
    println!("{} at ({}, {}) facing {}, {points} movement points", u.name, u.hex.x, u.hex.y, u.facing);
    let costs = reachable(&battle, &map, i, points);
    for h in highlighted(&battle, &map, i, points) {
        let cost = costs.get(&h).map_or_else(|| "(second hex)".to_string(), |c| format!("cost {c}"));
        println!("  ({:2}, {:2}) hex {} {cost}", h.x, h.y, map.hex_id_origin as i32 + h.x * 100 + h.y);
    }
    Ok(())
}

fn turns(data_dir: &Path, scn_file: &str, n: u32) -> Result<()> {
    use gboh_game::game::{Game, Verdict};
    let scn = Scenario::parse(&fs::read(find_ci(data_dir, scn_file)?)?)?;
    let map = Map::parse(&fs::read(find_ci(data_dir, &scn.map_file)?)?)?;
    let mut game = Game::new(&scn, map, 1);
    for turn in 1..=n {
        let mut order = Vec::new();
        let mut next = game.begin_turn();
        while let Some(l) = next {
            order.push(game.battle.units[l].name.clone());
            next = game.finish_leader(false).1;
        }
        let verdict = game.end_turn();
        println!(
            "turn {turn}: {}  | rout points {}/{} vs {}/{}",
            order.join(", "),
            game.rout_points(0),
            game.armies[0].1,
            game.rout_points(1),
            game.armies[1].1
        );
        if let Verdict::Won(a) = verdict {
            println!("army {a} wins");
            break;
        }
    }
    Ok(())
}

fn ai(data_dir: &Path, scn_file: &str, n: u32, opts: &[String]) -> Result<()> {
    let verbose = opts.iter().any(|o| o == "v");
    // `home` / `visitor`: only that army is played; the other gives no orders
    let only = opts.iter().find_map(|o| match o.as_str() {
        "home" => Some(0),
        "visitor" => Some(1),
        _ => None,
    });
    let plan_no: usize = opts.iter().find_map(|o| o.strip_prefix("plan=")?.parse().ok()).unwrap_or(1);
    use gboh_game::ai::{Machine, Script};
    use gboh_game::battle::UnitState;
    use gboh_game::game::{Game, Verdict};
    let scn = Scenario::parse(&fs::read(find_ci(data_dir, scn_file)?)?)?;
    let map = Map::parse(&fs::read(find_ci(data_dir, &scn.map_file)?)?)?;
    let mut load = |name: &str| find_ci(data_dir, name).ok().and_then(|p| fs::read(p).ok()).map(|d| String::from_utf8_lossy(&d).into_owned());
    let plan = |files: &[String], load: &mut dyn FnMut(&str) -> Option<String>| -> Result<Machine> {
        let f = files.get(plan_no - 1).or(files.first()).context("no AI plan")?;
        println!("plan {f}");
        Ok(Machine::new(Script::load(f, load).map_err(|e| anyhow::anyhow!("{e}"))?))
    };
    let mut machines = [plan(&scn.home_ai, &mut load)?, plan(&scn.visitor_ai, &mut load)?];
    let seed: u32 = opts.iter().find_map(|o| o.strip_prefix("seed=")?.parse().ok()).unwrap_or(1);
    let mut game = Game::new(&scn, map, seed);
    // `options=a,b`: rule options (see `gboh_game::options`)
    if let Some(o) = opts.iter().find_map(|o| o.strip_prefix("options=")) {
        game.set_options(gboh_game::options::Options::parse(&o.replace(',', " ")));
    }
    let mut first = 1;
    let mut resume: Option<Vec<usize>> = None;
    if let Some(path) = opts.iter().find_map(|o| o.strip_prefix("save=")) {
        let data = fs::read(path)?;
        let saved = gboh_game::save::read(&data, &game.battle, scn.home.nation).map_err(|e| anyhow::anyhow!("{}", e.0))?;
        gboh_game::save::apply(&mut game.battle, &saved);
        let t = gboh_game::save::turn(&data).unwrap_or(1);
        game.activation.turn = t - 1;
        first = t as u32;
        println!("loaded {path}: turn {t}, rout points {} vs {}", game.rout_points(0), game.rout_points(1));
        // carry on with the leaders who have not yet been active this turn
        let names = gboh_game::save::active_this_turn(&data);
        resume = Some(game.battle.leaders().filter(|&l| names.iter().any(|n| *n == game.battle.units[l].name)).collect());
    }
    for turn in first..first + n {
        let mut next = match resume.take() {
            Some(done) => game.resume_turn(&done),
            None => game.begin_turn(),
        };
        while let Some(l) = next {
            let side = game.battle.units[l].side;
            let army = if side == game.armies[0].0 { 0 } else { 1 };
            let m = &mut machines[army];
            let before: Vec<_> = game.battle.units.iter().map(|u| (u.hex, u.facing)).collect();
            let hits_before: Vec<i32> = game.battle.units.iter().map(|u| u.hits).collect();
            let log = if only.is_none_or(|o| o == army) { game.ai_orders(m).map_err(|e| anyhow::anyhow!("{e}"))? } else { Vec::new() };
            let moved = game.battle.units.iter().zip(&before).filter(|(u, b)| (u.hex, u.facing) != **b).count();
            let used = game.points.as_ref().map_or(0, |p| p.used);
            let moved_names: Vec<String> = game
                .battle
                .units
                .iter()
                .zip(&before)
                .zip(&hits_before)
                .filter(|((u, b), _)| (u.hex, u.facing) != **b)
                .map(|((u, _), h)| if u.hits > *h { format!("{} (+{} hits)", u.name, u.hits - h) } else { u.name.clone() })
                .collect();
            let shocks = game.shocks.len();
            let name = game.battle.units[l].name.clone();
            // the AI always tries for momentum (the original's commentary)
            let (out, n2) = game.finish_leader(only.is_none_or(|o| o == army));
            let routed: usize = out.iter().map(|o| o.routed.len() + o.eliminated.len()).sum();
            println!("turn {turn} {name:24} orders {used:2}  units moved/turned {moved:2}  shocks {shocks}  routed {routed}");
            // `events`: what happened, in the original's commentary terms
            if opts.iter().any(|o| o == "events") {
                use gboh_game::game::Happened;
                let n = |u: usize| game.battle.units[u].name.clone();
                for e in std::mem::take(&mut game.log) {
                    match e {
                        Happened::Volley { shooter, target, .. } => println!("    {} volleys at {}", n(shooter), n(target)),
                        Happened::Casualties { unit, count, hits, kind, .. } => println!("    {} takes {count} ({kind:?}), {hits} in all", n(unit)),
                        Happened::Routed { unit } => println!("    {} is routing", n(unit)),
                        Happened::Eliminated { unit, .. } => println!("    {} is eliminated", n(unit)),
                        Happened::Shock { attackers, defenders } => println!(
                            "    shock: {} -> {}",
                            attackers.iter().map(|&u| n(u)).collect::<Vec<_>>().join(", "),
                            defenders.iter().map(|&u| n(u)).collect::<Vec<_>>().join(", ")
                        ),
                        Happened::Moved { .. } => {}
                    }
                }
            } else {
                game.log.clear();
            }
            if opts.iter().any(|o| o == "names") && !moved_names.is_empty() {
                println!("    moved: {}", moved_names.join(", "));
            }
            if verbose {
                for line in log {
                    println!("    {line}");
                }
            }
            next = n2;
        }
        let verdict = game.end_turn();
        let gone = |side: u8| game.battle.units.iter().filter(|u| u.side == side && u.state == UnitState::Gone).count();
        let routed = |side: u8| game.battle.units.iter().filter(|u| u.side == side && u.state == UnitState::Routed).count();
        println!(
            "end of turn {turn}: rout points {}/{} vs {}/{} (units out {} vs {}, routed {} vs {})",
            game.rout_points(0),
            game.armies[0].1,
            game.rout_points(1),
            game.armies[1].1,
            gone(game.armies[0].0),
            gone(game.armies[1].0),
            routed(game.armies[0].0),
            routed(game.armies[1].0)
        );
        if let Verdict::Won(a) = verdict {
            let names = [
                "Greeks",
                "Persians",
                "Macedonians",
                "Indians",
                "Danubians",
                "Scythians",
                "Romans",
                "Carthaginians",
                "Seleucids",
                "Caesarians",
                "Pompeians",
                "Barbarians",
            ];
            println!("{} win", names.get(game.armies[a].0 as usize).unwrap_or(&"?"));
            break;
        }
    }
    Ok(())
}

fn strips(path: &Path) -> Result<()> {
    let eif = gboh_formats::eif::Eif::parse(&fs::read(path)?)?;
    println!("{}", eif.title);
    for (i, s) in eif.strips.iter().enumerate() {
        let sizes: Vec<String> = s.cels.iter().take(3).map(|c| format!("{}x{}", c.width, c.height)).collect();
        println!("  {i:3} {:32} {:3} cels  {}", s.name, s.cels.len(), sizes.join(" "));
    }
    Ok(())
}

fn sanitize(s: &str) -> String {
    let s: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if s.is_empty() { "_".into() } else { s }
}

fn extract(data_dir: &Path, out_dir: &Path) -> Result<()> {
    let pal = Palette::parse(&fs::read(palette_path(data_dir)?)?)?;
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
                    .flat_map(|(&i, &fx)| if fx != 0 || Some(i) == shadow_idx { [0, 0, 0, 128] } else { rgba_pal[i as usize] })
                    .collect();
                write_png(&dir.join(&file), cel.width as u32, cel.height as u32, &rgba).with_context(|| format!("{stem}/{file}"))?;
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
/// `static_water` keeps the water at its `GAME.PAL` colours, as the original
/// shows it on a true-colour display (its palette animation then has no
/// effect), for comparing with captures.
#[allow(clippy::too_many_arguments)]
fn render(data_dir: &Path, file: &str, out: &Path, zoom: &str, save: Option<&str>, static_water: bool, grid: bool, reversed: bool) -> Result<()> {
    let zoom = match zoom {
        "in" => Zoom::In,
        "normal" => Zoom::Normal,
        "out" => Zoom::Out,
        z => bail!("unknown zoom {z}"),
    };
    let mut load = |name: &str| find_ci(data_dir, name).ok().and_then(|p| fs::read(p).ok());
    // a scenario brings its map and units
    let scenario =
        if file.to_ascii_lowercase().ends_with(".scn") { Some(Scenario::parse(&load(file).with_context(|| format!("{file} not found"))?)?) } else { None };
    let map_file = scenario.as_ref().map_or_else(|| file.to_string(), |s| s.map_file.clone());
    let units = scenario.as_ref().map_or_else(Vec::new, |s| s.units.clone());
    let map = Map::parse(&load(&map_file).with_context(|| format!("{map_file} not found"))?)?;
    let flagged = scenario.as_ref().map_or_else(Vec::new, |s| gboh_game::command::opening_flags(s, &map, 1));
    let pal = if static_water {
        Palette::parse(&gboh_formats::pal::FILES.iter().find_map(|n| load(n)).context("GAME.PAL not found")?)?
    } else {
        battlefield::palette(&mut load)?
    };
    let titles = eif_title_index(data_dir)?;
    let mut load_title = |title: &str| titles.get(title).and_then(|n| fs::read(data_dir.join(n)).ok());
    let canvas = match (save, &scenario) {
        // a saved game's positions, routed units in their routed pose
        (Some(path), Some(scn)) => {
            use gboh_render::units::UnitView;
            let mut battle = gboh_game::battle::Battle::from_scenario(scn, &map);
            let data = fs::read(path)?;
            let saved = gboh_game::save::read(&data, &battle, scn.home.nation).map_err(|e| anyhow::anyhow!("{}", e.0))?;
            gboh_game::save::apply(&mut battle, &saved);
            let views: Vec<UnitView> = battle
                .units
                .iter()
                .filter(|u| u.on_field())
                .map(|u| UnitView {
                    hex: (u.hex.x as usize, u.hex.y as usize),
                    facing: u.facing,
                    type_id: u.type_id,
                    nation: u.nation,
                    id: u.source,
                    routed: u.state == gboh_game::battle::UnitState::Routed,
                    walk: None,
                    hits: u.hits.max(0) as u16,
                    tq: u.tq,
                    act: None,
                    fallen: None,
                })
                .collect();
            let types: Vec<usize> = views.iter().map(|v| v.type_id).collect();
            let scene = battlefield::Scene::build(&map_file, &map, zoom, &pal, 1, &types, &mut load, &mut load_title)?;
            scene.compose(&views, &[])
        }
        _ if grid || reversed => {
            let views: Vec<gboh_render::units::UnitView> =
                units.iter().enumerate().filter_map(|(i, u)| gboh_render::units::UnitView::of_scenario(&map, i, u)).collect();
            let types: Vec<usize> = views.iter().map(|v| v.type_id).collect();
            let mut scene = battlefield::Scene::build_view(&map_file, &map, zoom, reversed, &pal, 1, &types, &mut load, &mut load_title)?;
            scene.grid_on = grid;
            scene.compose(&views, &flagged)
        }
        _ => battlefield::render(&map_file, &map, zoom, &pal, 1, &units, &flagged, &mut load, &mut load_title)?,
    };
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
