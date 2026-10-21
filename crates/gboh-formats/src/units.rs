//! Unit types of *The Great Battles of Alexander*: which sprite set a
//! scenario unit is drawn with.
//!
//! A scenario unit only stores its class (`kind`), nation, missile type and a
//! few flags; the game derives a unit *type* from those when it loads the
//! scenario, and every type names an EIF sprite set by title.

use crate::scn::{Unit, UnitExtra};

/// Nations as the game numbers them internally (the scenario byte is
/// remapped on load, see [`Nation::from_scenario`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nation {
    Macedonian,
    Persian,
    Indian,
    Greek,
    /// Danubian / tribal troops (Pelium, Lyginus).
    Tribal,
    /// A second Persian nation, drawn like [`Nation::Persian`].
    PersianAllied,
}

impl Nation {
    /// The scenario's nation byte: 0 Greek, 1 Persian, 2 Macedonian,
    /// 3 Indian, 4 tribal, 5 Persian allied.
    pub fn from_scenario(b: u8) -> Option<Self> {
        Some(match b {
            0 => Nation::Greek,
            1 => Nation::Persian,
            2 => Nation::Macedonian,
            3 => Nation::Indian,
            4 => Nation::Tribal,
            5 => Nation::PersianAllied,
            _ => return None,
        })
    }
}

/// Missile weapon (the scenario byte: 1 bow, 2 sling, 3 javelin, 4 bolt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missile {
    Bow,
    Sling,
    Javelin,
    Bolt,
}

impl Missile {
    pub fn from_scenario(b: u8) -> Option<Self> {
        Some(match b {
            1 => Missile::Bow,
            2 => Missile::Sling,
            3 => Missile::Javelin,
            4 => Missile::Bolt,
            _ => return None,
        })
    }
}

/// Title of the EIF sprite set for each unit type id (the half and quarter
/// size sets append `" (Half size)"` / `" (Quarter size)"`). Type 1 is a
/// placeholder the game never places.
pub const ART_TITLES: [&str; 45] = [
    "Alexander",
    "Greek Heavy Infantry",
    "Macedonian Javelin",
    "Macedonian Heavy Infantry",
    "Macedonian Leader",
    "Persian Leader",
    "Greek Leader",
    "Darius",
    "Macedonian Heavy Cavalry",
    "Macedonian Lancer",
    "Macedonian Archers",
    "Persian Medium Infantry",
    "Persian Heavy Cavalry",
    "Persian Javelin",
    "Persian Light Infantry",
    "Persian Archers",
    "Macedonian Light Cavalry (Archers)",
    "Phalanx",
    "Greek Hoplites",
    "Macedonian Light Cavalry (Javelin)",
    "Macedonian Light Infantry",
    "Greek Light Infantry",
    "Persian Chariots",
    "Indian Chariots",
    "Persian Elephants (javelin)",
    "Persian Light Cavalry (Javelin)",
    "Persian Light Cavalry (Archers)",
    "Persian Lancers",
    "Indian Lancer",
    "Macedonian Slingers",
    "Persian Slingers",
    "Greek Hoplites",
    "Tribal Light Infantry",
    "Tribal Medium Infantry",
    "Greek Cavalry",
    "Indian Leader",
    "Indian Light Infantry (Archers)",
    "Indian Light Infantry (Javelin)",
    "Oxybeles",
    "Indian Light Cavalry (Javelin)",
    "Indian Elephant (Javelin)",
    "Persian Cataphracted Cavalry",
    "Agrianian Javelinists",
    "Persian Light Infantry",
    "Macedonian Cataphracted Cavalry",
];

/// The unit type id the game assigns to a scenario unit, or `None` for
/// combinations it rejects.
pub fn unit_type(u: &Unit) -> Option<usize> {
    use Missile::*;
    use Nation::*;
    let nation = Nation::from_scenario(u.nation)?;
    let (flag, missile, more) = match u.extra {
        UnitExtra::Leader(_) => (0, None, None),
        UnitExtra::Combat { c, more, .. } => (c, None, more),
        UnitExtra::Missile { c, missile, more, .. } => (c, Missile::from_scenario(missile), more),
    };
    Some(match (u.kind, nation) {
        (0, Macedonian) => {
            if u.name.contains("Alexander") {
                0
            } else {
                4
            }
        }
        (0, Persian | PersianAllied) => {
            if u.name == "Darius III" {
                7
            } else {
                5
            }
        }
        (0, Indian) => 35,
        (0, Greek | Tribal) => 6,
        (1 | 17, Macedonian) => 38,
        // heavy cavalry; the trailing byte marks cataphracts
        (2, Macedonian) => if more.unwrap_or(0) != 0 { 44 } else { 8 },
        (2, Persian) => if more.unwrap_or(0) != 0 { 41 } else { 12 },
        (2, PersianAllied) => 12,
        // light cavalry
        (3, Macedonian) => match missile? {
            Bow => 16,
            Javelin => 19,
            _ => return None,
        },
        (3, Persian | PersianAllied) => match missile? {
            Bow => 26,
            Javelin => 25,
            _ => return None,
        },
        (3, Indian) if missile == Some(Javelin) => 39,
        (3, Greek) if missile == Some(Javelin) => 34,
        (4, Persian) => 22,
        (4, Indian) => 23,
        (5, Macedonian | Indian) => 40,
        (5, Persian) => 24,
        // hoplites: the combat flag selects the double-sized formation
        (6 | 14, Macedonian | Persian | Greek) => if flag == 0 { 31 } else { 18 },
        (7, Macedonian) => 20,
        (7, Persian) if flag != 0 => 43,
        (7, Persian) => match missile? {
            Bow => 15,
            Javelin => 14,
            _ => return None,
        },
        (7, Indian) => match missile? {
            Bow => 36,
            Javelin => 37,
            _ => return None,
        },
        (7, Greek) if missile == Some(Javelin) => 21,
        (7, Tribal) => 32,
        (8, Persian) => 11,
        (8, Tribal) => 33,
        (9, Macedonian) => 9,
        (9, Persian | PersianAllied) => 27,
        (9, Indian) => 28,
        (10, Macedonian) => match missile? {
            Bow => 10,
            Sling => 29,
            Javelin if u.name.contains("Ag") => 42,
            Javelin => 2,
            Bolt => return None,
        },
        (10, Persian) => match missile? {
            Bow => 15,
            Sling => 30,
            Javelin => 13,
            Bolt => return None,
        },
        (15, Macedonian) => 3,
        (18, Macedonian) if missile == Some(Javelin) => 20,
        (18, Greek) if missile == Some(Javelin) => 21,
        (19, _) => 17,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(kind: u8, nation: u8, name: &str, extra: UnitExtra) -> Unit {
        Unit { kind, name: name.into(), nation, unk_a: 0, unk_b: 0, hex: 0, facing: 1, extra }
    }

    fn combat(c: u8) -> UnitExtra {
        UnitExtra::Combat { a: 0, b: 0, c, more: None }
    }

    fn missile(m: u8) -> UnitExtra {
        UnitExtra::Missile { a: 0, b: 0, c: 0, missile: m, m1: 0, m2: 0, more: None }
    }

    // units from CHAERONE.SCN
    #[test]
    fn chaeronea() {
        let art = |u: &Unit| unit_type(u).map(|t| ART_TITLES[t]);
        assert_eq!(art(&unit(19, 2, "Coenus (5)", combat(1))), Some("Phalanx"));
        assert_eq!(art(&unit(6, 0, "Thebes 1", combat(1))), Some("Greek Hoplites"));
        assert_eq!(unit_type(&unit(6, 0, "Thebes 1", combat(1))), Some(18));
        assert_eq!(unit_type(&unit(6, 0, "Theban Sacred Band", combat(0))), Some(31));
        assert_eq!(art(&unit(15, 2, "Hypaspist 1", combat(0))), Some("Macedonian Heavy Infantry"));
        assert_eq!(art(&unit(7, 0, "Greek Mercenary 1", missile(3))), Some("Greek Light Infantry"));
        assert_eq!(art(&unit(7, 2, "Thrace 1", missile(3))), Some("Macedonian Light Infantry"));
        assert_eq!(art(&unit(3, 0, "Athens", missile(3))), Some("Greek Cavalry"));
        assert_eq!(art(&unit(0, 2, "Alexander (CH)", UnitExtra::Leader([0; 4]))), Some("Alexander"));
        assert_eq!(art(&unit(0, 2, "Phillip II", UnitExtra::Leader([0; 4]))), Some("Macedonian Leader"));
    }
}
