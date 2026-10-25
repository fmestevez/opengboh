//! Saved games: the battle's scenario, who the computer plays, the rout
//! levels and the player's orders in turn. Loading starts the scenario
//! afresh and replays the orders; play is deterministic, so the battle comes
//! back exactly as it was. (The original's own `*.alx` files hold its whole
//! state, of which only the units are decoded; see `gboh_game::save`.)
//!
//! ```text
//! opengboh save 1
//! description <text>
//! scenario <file>
//! computer <0|1> <0|1>
//! levels <home> <visitor>
//! <order>*
//! ```
//!
//! A battle of a campaign has the campaign's state first, each line of it
//! after `campaign `. Orders, one a line: `c <x> <y> l|r` click, `a` arm a volley, `g` group
//! move, `y` rally, `v` recover cohesion, `t l|r` turn, `b` about face,
//! `u` undo, `f 0|1` finish (with momentum), `s` the computer's leader
//! played.

use crate::play::{GroupOrder, Order};
use gboh_game::hex::Hex;
use std::path::{Path, PathBuf};

/// The first line of a save, with the format's version.
const MAGIC: &str = "opengboh save 1";

/// A saved game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Save {
    pub description: String,
    pub scenario: String,
    pub computer: [bool; 2],
    pub levels: [u16; 2],
    /// Hannibal's: the battle began with a deployment phase.
    pub deploy: bool,
    /// The rule options the battle is fought by.
    pub options: gboh_game::options::Options,
    pub orders: Vec<Order>,
    /// The campaign the battle is part of, if one (its own text, see
    /// `gboh_game::campaign::Campaign::to_text`).
    pub campaign: Option<String>,
}

/// An order as a line of a save.
pub fn order_line(o: &Order) -> String {
    let flag = |b: bool| if b { "1" } else { "0" };
    let side = |left: bool| if left { "l" } else { "r" };
    match *o {
        Order::Click { hex, right } => format!("c {} {} {}", hex.x, hex.y, side(!right)),
        Order::ArmVolley => "a".into(),
        Order::ArmBackward => "k".into(),
        Order::EndDeployment => "d".into(),
        Order::GroupMove => "g".into(),
        Order::Rally => "y".into(),
        Order::Recover => "v".into(),
        Order::Turn { left } => format!("t {}", side(left)),
        Order::AboutFace => "b".into(),
        Order::Undo => "u".into(),
        Order::Finish { momentum } => format!("f {}", flag(momentum)),
        Order::StepComputer => "s".into(),
        Order::Elite { accept } => format!("e {}", flag(accept)),
        Order::Group(g) => format!("G {}", ["retreat", "attack", "restore", "extend"][g as usize]),
    }
}

/// An order read from a line of a save.
pub fn parse_order(line: &str) -> Result<Order, ()> {
    let w: Vec<&str> = line.split_whitespace().collect();
    let n = |s: &str| s.parse::<i32>().map_err(|_| ());
    Ok(match w.as_slice() {
        ["c", x, y, s] => Order::Click { hex: Hex::new(n(x)?, n(y)?), right: *s == "r" },
        ["a"] => Order::ArmVolley,
        ["k"] => Order::ArmBackward,
        ["d"] => Order::EndDeployment,
        ["g"] => Order::GroupMove,
        ["y"] => Order::Rally,
        ["v"] => Order::Recover,
        ["t", s] => Order::Turn { left: *s == "l" },
        ["b"] => Order::AboutFace,
        ["u"] => Order::Undo,
        ["f", m] => Order::Finish { momentum: *m == "1" },
        ["s"] => Order::StepComputer,
        ["e", a] => Order::Elite { accept: *a == "1" },
        ["G", g] => Order::Group(match *g {
            "retreat" => GroupOrder::Retreat,
            "attack" => GroupOrder::Attack,
            "restore" => GroupOrder::RestoreCohesion,
            "extend" => GroupOrder::LineExtension,
            _ => return Err(()),
        }),
        _ => return Err(()),
    })
}

impl Save {
    pub fn to_text(&self) -> String {
        let flag = |b: bool| if b { "1" } else { "0" };
        let mut s = format!(
            "{MAGIC}\ndescription {}\nscenario {}\ncomputer {} {}\nlevels {} {}\n",
            self.description.replace('\n', " "),
            self.scenario,
            flag(self.computer[0]),
            flag(self.computer[1]),
            self.levels[0],
            self.levels[1]
        );
        if self.deploy {
            s.push_str("deploy 1\n");
        }
        if self.options != Default::default() {
            s.push_str(&format!("options {}\n", self.options.to_text()));
        }
        for line in self.campaign.iter().flat_map(|c| c.lines()) {
            s.push_str(&format!("campaign {line}\n"));
        }
        for o in &self.orders {
            s.push_str(&order_line(o));
            s.push('\n');
        }
        s
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        if lines.next() != Some(MAGIC) {
            return Err("not an opengboh save (or another version)".into());
        }
        let mut field = |name: &str| -> Result<String, String> {
            let line = lines.next().ok_or(format!("no {name}"))?;
            line.strip_prefix(name)
                .map(|v| v.trim_start().to_string())
                .ok_or(format!("expected {name}, found {line:?}"))
        };
        let description = field("description")?;
        let scenario = field("scenario")?;
        let pair = |v: String| -> Result<[u16; 2], String> {
            let n: Vec<u16> = v.split_whitespace().map(|x| x.parse().map_err(|_| format!("bad number {x:?}"))).collect::<Result<_, _>>()?;
            n.try_into().map_err(|_| "expected two numbers".to_string())
        };
        let c = pair(field("computer")?)?;
        let levels = pair(field("levels")?)?;
        let mut orders = Vec::new();
        let mut campaign: Option<String> = None;
        let mut deploy = false;
        let mut options = gboh_game::options::Options::default();
        for (k, line) in lines.enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            if line.trim() == "deploy 1" {
                deploy = true;
                continue;
            }
            if let Some(o) = line.strip_prefix("options ") {
                options = gboh_game::options::Options::parse(o);
                continue;
            }
            if let Some(c) = line.strip_prefix("campaign ") {
                let text = campaign.get_or_insert_with(String::new);
                text.push_str(c);
                text.push('\n');
                continue;
            }
            orders.push(parse_order(line).map_err(|_| format!("bad order {line:?} (line {})", k + 6))?);
        }
        Ok(Self { description, scenario, computer: [c[0] != 0, c[1] != 0], levels, deploy, options, orders, campaign })
    }
}

/// Where saves go: `GBOH_SAVES` if set, else the platform's place for
/// application data (`~/Library/Application Support/opengboh/saves` on
/// macOS, `%APPDATA%\opengboh\saves` on Windows, `$XDG_DATA_HOME` or
/// `~/.local/share/opengboh/saves` elsewhere). Never the game's own
/// directory.
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("GBOH_SAVES") {
        return PathBuf::from(d);
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let base = if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home)
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share"))
    };
    base.join("opengboh").join("saves")
}

/// The saves in `dir`, newest first: file name and description.
pub fn list(dir: &Path) -> Vec<(String, String)> {
    let mut found: Vec<(std::time::SystemTime, String, String)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "sav"))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let save = Save::parse(&text).ok()?;
            let when = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((when, e.file_name().to_string_lossy().into_owned(), save.description))
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, f, d)| (f, d)).collect()
}

/// Write `save` to `dir/name` (`.sav` added if missing).
pub fn write(dir: &Path, name: &str, save: &Save) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut path = dir.join(name);
    if path.extension().is_none_or(|x| x != "sav") {
        path.set_extension("sav");
    }
    std::fs::write(&path, save.to_text())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_round_trip() {
        let save = Save {
            description: "Hydaspes; Turn #2; Coenus".into(),
            scenario: "HYDASPES.SCN".into(),
            computer: [false, true],
            levels: [80, 70],
            deploy: false,
            options: gboh_game::options::Options { roads: true, ..Default::default() },
            orders: vec![
                Order::Click { hex: Hex::new(13, 21), right: false },
                Order::Click { hex: Hex::new(4, 5), right: true },
                Order::Turn { left: true },
                Order::AboutFace,
                Order::Undo,
                Order::ArmVolley,
                Order::GroupMove,
                Order::Rally,
                Order::Recover,
                Order::Finish { momentum: true },
                Order::StepComputer,
                Order::Elite { accept: false },
            ],
            campaign: Some("round 3\ncurrent 1\n".into()),
        };
        assert_eq!(Save::parse(&save.to_text()), Ok(save.clone()));
        assert!(Save::parse("something else").is_err());
        let dir = std::env::temp_dir().join(format!("gboh-saves-{}", std::process::id()));
        let path = write(&dir, "test", &save).unwrap();
        assert_eq!(path.extension().unwrap(), "sav");
        assert_eq!(list(&dir), vec![("test.sav".to_string(), save.description.clone())]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
