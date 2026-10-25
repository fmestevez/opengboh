//! The player's preferences: what the original keeps in the registry
//! (`HKCU\Software\I-Magic\Alexander\Preferences`, read in the
//! application's set-up, `FUN_004816e2`), here in a text file beside the
//! saves; and the Preferences dialog (`PREFERENCES` of `alex.exe`) that
//! edits them.

use crate::hud::{Plain, SLIDER_MAX};
use std::path::{Path, PathBuf};

/// The preferences. A missing file gives the original's defaults: all on,
/// both volumes 5000 of 10000.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    pub movement_animation: bool,
    pub combat_animation: bool,
    pub sounds: bool,
    pub ambient_sounds: bool,
    pub music: bool,
    /// The finish leader reminder (`LEADERALMOSTFINISHED`).
    pub finish_leader_dialog: bool,
    pub overall_combat_results: bool,
    pub intro_video: bool,
    pub auto_save: bool,
    pub volume: u16,
    pub music_volume: u16,
    /// "Never show this again" ticked on the missile results.
    pub quiet_missiles: bool,
    /// Hannibal's: the Triarii tactical doctrine, the group command
    /// confirmation, and sparse trees.
    pub triarii: bool,
    pub confirm_group: bool,
    pub sparse_trees: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            movement_animation: true,
            combat_animation: true,
            sounds: true,
            ambient_sounds: true,
            music: true,
            finish_leader_dialog: true,
            overall_combat_results: true,
            intro_video: true,
            auto_save: true,
            volume: 5000,
            music_volume: 5000,
            quiet_missiles: false,
            triarii: true,
            confirm_group: true,
            sparse_trees: false,
        }
    }
}

/// A preference a check box sets.
type Flag = fn(&mut Prefs) -> &mut bool;

/// The dialog's check boxes (control ids) and the preference each sets.
const BOXES: [(u16, Flag); 11] = [
    (101, |p| &mut p.movement_animation),
    (102, |p| &mut p.combat_animation),
    (104, |p| &mut p.sounds),
    (15, |p| &mut p.ambient_sounds),
    (103, |p| &mut p.music),
    (17, |p| &mut p.finish_leader_dialog),
    (20, |p| &mut p.overall_combat_results),
    (18, |p| &mut p.intro_video),
    (19, |p| &mut p.auto_save),
    // Hannibal's
    (21, |p| &mut p.triarii),
    (22, |p| &mut p.confirm_group),
];

/// Hannibal's trees radio buttons: sparse, dense.
pub const SPARSE: u16 = 255;
pub const DENSE: u16 = 254;

/// Where the dialog's two volume sliders lie, in dialog units: the
/// original makes them in code (`FUN_00599944`, `Slider` controls of
/// range 10000) in the gaps above "Sound Volume" and "Music Volume" (🟡
/// their exact places are not traced).
const SLIDERS: [(i16, i16); 2] = [(13, 72), (13, 115)];

/// Hannibal's dialog has them two units lower.
const HANNIBAL_SLIDERS: [(i16, i16); 2] = [(13, 74), (13, 117)];

impl Prefs {
    /// The preferences file: beside the saves directory.
    pub fn path() -> PathBuf {
        let saves = crate::saves::dir();
        saves.parent().map_or_else(|| PathBuf::from("preferences.txt"), |p| p.join("preferences.txt"))
    }

    /// Read `path` (`name value` lines); what is missing keeps its default.
    pub fn load(path: &Path) -> Self {
        let mut p = Self::default();
        let Ok(text) = std::fs::read_to_string(path) else { return p };
        for line in text.lines() {
            let Some((key, value)) = line.split_once(' ') else { continue };
            let flag = value.trim() == "1";
            let number = value.trim().parse::<u16>().ok().map(|v| v.min(SLIDER_MAX));
            match key {
                "movement_animation" => p.movement_animation = flag,
                "combat_animation" => p.combat_animation = flag,
                "sounds" => p.sounds = flag,
                "ambient_sounds" => p.ambient_sounds = flag,
                "music" => p.music = flag,
                "finish_leader_dialog" => p.finish_leader_dialog = flag,
                "overall_combat_results" => p.overall_combat_results = flag,
                "intro_video" => p.intro_video = flag,
                "auto_save" => p.auto_save = flag,
                "quiet_missiles" => p.quiet_missiles = flag,
                "triarii" => p.triarii = flag,
                "confirm_group" => p.confirm_group = flag,
                "sparse_trees" => p.sparse_trees = flag,
                "volume" => p.volume = number.unwrap_or(p.volume),
                "music_volume" => p.music_volume = number.unwrap_or(p.music_volume),
                _ => {}
            }
        }
        p
    }

    pub fn to_text(&self) -> String {
        let f = |b: bool| if b { 1 } else { 0 };
        format!(
            "movement_animation {}\ncombat_animation {}\nsounds {}\nambient_sounds {}\nmusic {}\nfinish_leader_dialog {}\noverall_combat_results {}\nintro_video {}\nauto_save {}\nquiet_missiles {}\nvolume {}\nmusic_volume {}\ntriarii {}\nconfirm_group {}\nsparse_trees {}\n",
            f(self.movement_animation),
            f(self.combat_animation),
            f(self.sounds),
            f(self.ambient_sounds),
            f(self.music),
            f(self.finish_leader_dialog),
            f(self.overall_combat_results),
            f(self.intro_video),
            f(self.auto_save),
            f(self.quiet_missiles),
            self.volume,
            self.music_volume,
            f(self.triarii),
            f(self.confirm_group),
            f(self.sparse_trees)
        )
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_text())
    }

    /// The Preferences dialog showing these preferences (`game`'s layout).
    pub fn dialog(&self, game: gboh_formats::game::Game) -> Plain {
        let mut me = self.clone();
        let mut checked: Vec<u16> = BOXES.iter().filter(|(_, get)| *get(&mut me)).map(|(id, _)| *id).collect();
        checked.push(if self.sparse_trees { SPARSE } else { DENSE });
        let at = if game != gboh_formats::game::Game::Alexander { HANNIBAL_SLIDERS } else { SLIDERS };
        let sliders = vec![(at[0].0, at[0].1, self.volume), (at[1].0, at[1].1, self.music_volume)];
        Plain { name: "PREFERENCES", checked, sliders, ..Default::default() }
    }

    /// The preferences the dialog `d` was left showing (on OK).
    pub fn with_dialog(&self, d: &Plain) -> Self {
        let mut p = self.clone();
        for (id, get) in BOXES {
            *get(&mut p) = d.checked.contains(&id);
        }
        p.sparse_trees = d.checked.contains(&SPARSE);
        if let [(_, _, v), (_, _, m)] = d.sliders.as_slice() {
            p.volume = *v;
            p.music_volume = *m;
        }
        p
    }

    /// Whether `id` is one of the dialog's check boxes.
    pub fn is_box(id: u16) -> bool {
        BOXES.iter().any(|(b, _)| *b == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefs_round_trip() {
        let p = Prefs { combat_animation: false, music: false, volume: 1234, ..Default::default() };
        let dir = std::env::temp_dir().join(format!("gboh-prefs-{}", std::process::id()));
        let path = dir.join("preferences.txt");
        p.save(&path).unwrap();
        assert_eq!(Prefs::load(&path), p);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(Prefs::load(&dir.join("none")), Prefs::default());
        // through the dialog and back
        let mut d = p.dialog(gboh_formats::game::Game::Alexander);
        assert!(!d.checked.contains(&102) && d.checked.contains(&101));
        d.checked.retain(|&c| c != 101);
        d.sliders[1].2 = 42;
        let q = p.with_dialog(&d);
        assert_eq!((q.movement_animation, q.music_volume, q.volume), (false, 42, 1234));
    }
}
