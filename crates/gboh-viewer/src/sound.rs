//! Battle sounds from the original's WAV files in `DATA`.
//!
//! The file names and their groups follow the original's tables (see
//! `docs/formats.md`, "Sounds"); which group a unit type uses is chosen by
//! the sounds' names (🟡 the original picks the movement group in its
//! presentation code, not traced).

use gboh_game::missile::Weapon;
use gboh_game::movement::MoveClass;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Something worth a sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Select,
    Move(MoveClass),
    Volley(Weapon, bool),
    Shock { elephants: bool },
    Start,
    End { won: bool },
}

/// Movement sounds by unit class (groups of the original's table).
fn movement(class: MoveClass) -> &'static [&'static str] {
    use MoveClass::*;
    match class {
        Phalanx | HeavyInfantry => &["hilup1", "hilup2", "hilup3", "hilup4"],
        MediumInfantry => &["milup1", "milup2"],
        HeavyCavalry | Lancer => &["hcav1", "hcav2", "hcav3"],
        LightCavalry => &["cavlup1", "cavlup2", "cavlup3", "cavlup4"],
        Elephant => &["eleflup1", "eleflup2", "eleflup3"],
        Chariot => &["charlup1", "charlup2", "charlup3"],
        Oxybeles => &["oxymove"],
        _ => &["lilup2", "lilup3", "lilup4", "lilup5"],
    }
}

/// A missile weapon's shot and hit sounds.
fn missile(weapon: Weapon) -> (&'static [&'static str], &'static [&'static str]) {
    match weapon {
        Weapon::Bow | Weapon::MountedBow => (&["bowshot", "bowshot1"], &["arohitsa", "arohitsb", "arohitsc"]),
        Weapon::Sling => (&["sling"], &["rokhit1", "rokhit2", "rokhit3"]),
        Weapon::Javelin | Weapon::MountedJavelin => (&["bowshot"], &["javhits1", "javhits2", "javhits3"]),
        Weapon::Bolt => (&["oxybeles"], &["oxyhit", "oxyhit1", "oxyhit2"]),
    }
}

/// Plays sounds; silent if there is no audio output.
pub struct Sound {
    output: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
    dir: PathBuf,
    files: HashMap<String, PathBuf>,
    cache: HashMap<String, Vec<u8>>,
    seed: u32,
}

impl Sound {
    pub fn new(data_dir: &Path) -> Self {
        let files = std::fs::read_dir(data_dir)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter_map(|p| Some((p.file_name()?.to_str()?.to_ascii_lowercase(), p)))
                    .collect()
            })
            .unwrap_or_default();
        // GBOH_MUTE silences everything
        let output = if std::env::var_os("GBOH_MUTE").is_some() { None } else { rodio::OutputStream::try_default().ok() };
        Self { output, dir: data_dir.into(), files, cache: HashMap::new(), seed: 1 }
    }

    fn pick<'a>(&mut self, names: &[&'a str]) -> &'a str {
        self.seed = self.seed.wrapping_mul(22695477).wrapping_add(1);
        names[((self.seed >> 16) as usize) % names.len()]
    }

    /// Play `name` (without `.wav`) once.
    pub fn play_file(&mut self, name: &str) {
        let Some((_, handle)) = &self.output else { return };
        let key = format!("{name}.wav");
        if !self.cache.contains_key(&key) {
            let Some(path) = self.files.get(&key).cloned().or_else(|| Some(self.dir.join(&key))) else { return };
            let Ok(data) = std::fs::read(path) else { return };
            self.cache.insert(key.clone(), data);
        }
        let data = self.cache[&key].clone();
        if let Ok(source) = rodio::Decoder::new(Cursor::new(data)) {
            let _ = handle.play_raw(rodio::Source::convert_samples(source));
        }
    }

    pub fn play(&mut self, event: Event) {
        let name = match event {
            Event::Select => "select",
            Event::Move(class) => self.pick(movement(class)),
            Event::Volley(weapon, hit) => {
                let (shot, hits) = missile(weapon);
                let s = self.pick(shot);
                self.play_file(s);
                if !hit {
                    return;
                }
                self.pick(hits)
            }
            Event::Shock { elephants } => {
                if elephants {
                    let e = self.pick(&["elef1", "elef2", "elef3", "elef4", "elef5", "elefblow"]);
                    self.play_file(e);
                }
                self.pick(&["shoklup1", "shoklup2", "shoklup3", "shoklup4", "shoklup5"])
            }
            Event::Start => "start",
            Event::End { won } => {
                if won {
                    "victory"
                } else {
                    "defeat"
                }
            }
        };
        self.play_file(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sound named here exists in the game's DATA directory; runs
    /// only with `GBOH_DATA` set.
    #[test]
    fn sounds_exist() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let have: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .map(|n| n.to_ascii_lowercase())
            .collect();
        use MoveClass::*;
        let mut names: Vec<&str> = vec!["select", "start", "victory", "defeat", "shoklup1", "shoklup5", "elef1", "elefblow"];
        for c in [Phalanx, HeavyInfantry, MediumInfantry, LightInfantry, Skirmisher, HeavyCavalry, Lancer, LightCavalry, Elephant, Chariot, Oxybeles] {
            names.extend(movement(c));
        }
        for w in [Weapon::Bow, Weapon::Sling, Weapon::Javelin, Weapon::MountedBow, Weapon::MountedJavelin, Weapon::Bolt] {
            let (a, b) = missile(w);
            names.extend(a);
            names.extend(b);
        }
        for n in names {
            assert!(have.contains(&format!("{n}.wav")), "{n}.wav missing");
        }
    }
}
