//! Which of the Erudite *Great Battles* games a data directory belongs to.
//! The three share the engine; their files differ in places (the palette's
//! name, the scenario records, the art), and each has its own rules.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Game {
    #[default]
    Alexander,
    Hannibal,
    Caesar,
}

impl Game {
    pub const ALL: [Game; 3] = [Game::Alexander, Game::Hannibal, Game::Caesar];

    /// The game's palette file in its `DATA` directory.
    pub fn palette_file(self) -> &'static str {
        crate::pal::FILES[self as usize]
    }

    /// The game's executable, beside its `DATA` directory (the rewrite reads
    /// its menus, dialogs and strings).
    pub fn exe_file(self) -> &'static str {
        ["ALEX.EXE", "HANNIBAL.EXE", "CAESAR.EXE"][self as usize]
    }

    /// The game's help file, in its `DATA` directory.
    pub fn help_file(self) -> &'static str {
        ["ALEX.HLP", "HANNIBAL.HLP", "CAESAR.HLP"][self as usize]
    }

    /// The game whose files are in `data_dir`, told by its palette's name.
    pub fn detect(data_dir: &Path) -> Option<Game> {
        let names: Vec<String> = std::fs::read_dir(data_dir)
            .ok()?
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .map(|n| n.to_ascii_uppercase())
            .collect();
        Game::ALL.into_iter().find(|g| names.iter().any(|n| n == g.palette_file()))
    }
}
