//! The games' data the player has imported: a copy of each title's
//! executable and `DATA` directory, kept with the rewrite's other files
//! (beside the saves), and the title whose look the menus take (the
//! theme). The rewrite carries no game data; it works from these copies.
//!
//! ```text
//! <application data>/opengboh/
//!     games/alexander/ALEX.EXE, games/alexander/DATA/...
//!     games/hannibal/..., games/caesar/...
//!     settings.txt          "theme alexander"
//!     saves/, preferences.txt
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The Erudite titles the engine is meant for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Title {
    Alexander,
    Hannibal,
    Caesar,
}

impl Title {
    pub const ALL: [Title; 3] = [Title::Alexander, Title::Hannibal, Title::Caesar];

    pub fn name(self) -> &'static str {
        match self {
            Title::Alexander => "The Great Battles of Alexander",
            Title::Hannibal => "The Great Battles of Hannibal",
            Title::Caesar => "The Great Battles of Caesar",
        }
    }

    /// The word the title goes by in its executable's strings and in the
    /// library's directory names.
    pub fn key(self) -> &'static str {
        match self {
            Title::Alexander => "alexander",
            Title::Hannibal => "hannibal",
            Title::Caesar => "caesar",
        }
    }

    /// The engine's name for the game.
    pub fn game(self) -> gboh_formats::game::Game {
        use gboh_formats::game::Game;
        match self {
            Title::Alexander => Game::Alexander,
            Title::Hannibal => Game::Hannibal,
            Title::Caesar => Game::Caesar,
        }
    }

    pub fn from_key(key: &str) -> Option<Title> {
        Title::ALL.into_iter().find(|t| t.key() == key.trim().to_ascii_lowercase())
    }

    /// Whether the rewrite plays this title's battles yet (all three's).
    pub fn playable(self) -> bool {
        true
    }
}

/// The rewrite's own directory (where the saves directory lives).
pub fn root() -> PathBuf {
    let saves = crate::saves::dir();
    saves.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("opengboh"))
}

fn game_dir(t: Title) -> PathBuf {
    root().join("games").join(t.key())
}

/// The imported `DATA` directory of `t`, if it has been imported.
pub fn data_dir(t: Title) -> Option<PathBuf> {
    let d = crate::find_ci(&game_dir(t), "DATA")?;
    crate::palette_path(&d).map(|_| d)
}

/// The titles imported, in their order.
pub fn imported() -> Vec<Title> {
    Title::ALL.into_iter().filter(|&t| data_dir(t).is_some()).collect()
}

/// A game's installation found in a folder the player picked: which title,
/// its executable and its `DATA` directory.
#[derive(Debug, Clone)]
pub struct Found {
    pub title: Title,
    pub exe: PathBuf,
    pub data: PathBuf,
}

/// Look for an installation of `want` in `folder` (the game's folder, or
/// its `DATA` directory): a `DATA` directory with the game's palette, and beside it
/// an executable whose title string names the game.
pub fn detect(folder: &Path, want: Title) -> Result<Found, String> {
    let (top, data) = match crate::find_ci(folder, "DATA") {
        Some(d) => (folder.to_path_buf(), d),
        None if crate::palette_path(folder).is_some() => (folder.parent().map(Path::to_path_buf).unwrap_or_default(), folder.to_path_buf()),
        None => return Err(format!("No DATA folder in {}.", folder.display())),
    };
    if crate::palette_path(&data).is_none() {
        return Err(format!("{} has no palette file: not a Great Battles game.", data.display()));
    }
    let mut exes: Vec<PathBuf> = std::fs::read_dir(&top)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
        .collect();
    // the game's own executable first (Caesar's folder also holds its
    // scenario editor)
    let own = want.game().exe_file();
    exes.sort_by_key(|p| !p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(own)));
    // the executable whose first string (the game's title) names it
    let named =
        |p: &PathBuf| std::fs::read(p).ok().and_then(|d| gboh_formats::pe::Resources::parse(&d).ok()).and_then(|r| r.string(1)).map(|s| s.to_ascii_lowercase());
    for exe in &exes {
        if let Some(title) = named(exe) {
            if title.contains(want.key()) {
                return Ok(Found { title: want, exe: exe.clone(), data });
            }
            if let Some(other) = Title::ALL.into_iter().find(|t| title.contains(t.key())) {
                return Err(format!("That folder holds {}, not {}.", other.name(), want.name()));
            }
        }
    }
    Err(format!("No executable of {} in {}.", want.name(), top.display()))
}

/// An import under way: bytes copied of all, and how it ended.
#[derive(Debug, Default)]
pub struct Progress {
    pub copied: AtomicU64,
    pub total: AtomicU64,
    pub done: Mutex<Option<Result<(), String>>>,
}

fn files_in(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files_in(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Copy `found` into the library on a thread of its own, replacing an
/// earlier import of the title once the copy is complete.
pub fn import(found: Found) -> Arc<Progress> {
    let progress = Arc::new(Progress::default());
    let p = progress.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let dest = game_dir(found.title);
            let staging = dest.with_extension("importing");
            let _ = std::fs::remove_dir_all(&staging);
            let mut files = Vec::new();
            files_in(&found.data, &mut files);
            let size = |f: &Path| std::fs::metadata(f).map_or(0, |m| m.len());
            p.total.store(files.iter().map(|f| size(f)).sum::<u64>() + size(&found.exe), Ordering::Relaxed);
            let data_dest = staging.join("DATA");
            for f in &files {
                let rel = f.strip_prefix(&found.data).map_err(|e| e.to_string())?;
                let to = data_dest.join(rel);
                if let Some(d) = to.parent() {
                    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                }
                std::fs::copy(f, &to).map_err(|e| format!("{}: {e}", f.display()))?;
                p.copied.fetch_add(size(f), Ordering::Relaxed);
            }
            let exe_name = found.exe.file_name().ok_or("no executable name")?;
            std::fs::copy(&found.exe, staging.join(exe_name)).map_err(|e| e.to_string())?;
            p.copied.fetch_add(size(&found.exe), Ordering::Relaxed);
            let _ = std::fs::remove_dir_all(&dest);
            std::fs::rename(&staging, &dest).map_err(|e| e.to_string())?;
            Ok(())
        })();
        *p.done.lock().unwrap() = Some(result);
    });
    progress
}

/// The rewrite's settings: the title whose look the menus take.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub theme: Option<Title>,
    /// The rule options new battles are fought by.
    pub options: gboh_game::options::Options,
}

impl Settings {
    pub fn load() -> Self {
        let text = std::fs::read_to_string(root().join("settings.txt")).unwrap_or_default();
        let theme = text.lines().find_map(|l| l.strip_prefix("theme ").and_then(Title::from_key));
        let options = text.lines().find_map(|l| l.strip_prefix("options ")).map(gboh_game::options::Options::parse).unwrap_or_default();
        Self { theme, options }
    }

    pub fn save(&self) {
        let dir = root();
        let _ = std::fs::create_dir_all(&dir);
        let text = self.theme.map_or(String::new(), |t| format!("theme {}\n", t.key())) + &format!("options {}\n", self.options.to_text());
        let _ = std::fs::write(dir.join("settings.txt"), text);
    }

    /// The theme in force: the one chosen if imported, else the first
    /// title imported.
    pub fn theme_in_force(&self, imported: &[Title]) -> Option<Title> {
        self.theme.filter(|t| imported.contains(t)).or_else(|| imported.first().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recognises the games' own folders (and their DATA directories); runs only
    /// with `GBOH_DATA` set.
    #[test]
    fn detects_alexander() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let data = Path::new(&dir);
        let top = data.parent().unwrap();
        let found = detect(top, Title::Alexander).unwrap();
        assert!(found.exe.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("alex.exe"));
        assert!(detect(data, Title::Alexander).is_ok());
        let wrong = detect(top, Title::Caesar).unwrap_err();
        assert!(wrong.contains("Alexander"), "{wrong}");
        assert_eq!(Settings { theme: Some(Title::Caesar), options: Default::default() }.theme_in_force(&[Title::Alexander]), Some(Title::Alexander));
    }

    /// Hannibal's and Caesar's folders (theirs beside Alexander's, as in the
    /// developer's set-up); Caesar's is found by `caesar.exe`, not its
    /// editor's.
    #[test]
    fn detects_hannibal_and_caesar() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let downloads = Path::new(&dir).ancestors().nth(3).unwrap().to_path_buf();
        for (t, folder) in [(Title::Hannibal, "hannibal/app"), (Title::Caesar, "caesar/app")] {
            let top = downloads.join(folder);
            if !top.exists() {
                continue;
            }
            let found = detect(&top, t).unwrap();
            assert!(found.exe.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case(t.game().exe_file()), "{found:?}");
        }
    }

    /// Imports a small installation (the executable and a DATA folder with
    /// the palette) into a library of its own; runs only with `GBOH_DATA`.
    #[test]
    fn imports_into_the_library() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let data = Path::new(&dir);
        let tmp = std::env::temp_dir().join(format!("gboh-lib-{}", std::process::id()));
        let src = tmp.join("game");
        std::fs::create_dir_all(src.join("DATA")).unwrap();
        std::fs::copy(data.join("GAME.PAL"), src.join("DATA/GAME.PAL")).unwrap();
        std::fs::copy(crate::find_ci(data.parent().unwrap(), "ALEX.EXE").unwrap(), src.join("ALEX.EXE")).unwrap();
        let found = detect(&src, Title::Alexander).unwrap();
        // the library lives where GBOH_SAVES points (its parent)
        unsafe { std::env::set_var("GBOH_SAVES", tmp.join("lib/saves")) };
        let p = import(found);
        let result = loop {
            if let Some(r) = p.done.lock().unwrap().take() {
                break r;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(result, Ok(()));
        assert!(data_dir(Title::Alexander).is_some());
        assert_eq!(imported(), vec![Title::Alexander]);
        unsafe { std::env::remove_var("GBOH_SAVES") };
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
