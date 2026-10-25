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

/// The unit type id the game assigns to a scenario unit (Alexander's
/// rules, or Hannibal's for a unit of one of its scenarios), or `None` for
/// combinations it rejects.
pub fn unit_type(u: &Unit) -> Option<usize> {
    match u.layout {
        crate::scn::Layout::Hannibal => return hannibal_unit_type(u),
        crate::scn::Layout::Caesar => return caesar_unit_type(u),
        crate::scn::Layout::Alexander => {}
    }
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
        (2, Macedonian) => {
            if more.unwrap_or(0) != 0 {
                44
            } else {
                8
            }
        }
        (2, Persian) => {
            if more.unwrap_or(0) != 0 {
                41
            } else {
                12
            }
        }
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
        (6 | 14, Macedonian | Persian | Greek) => {
            if flag == 0 {
                31
            } else {
                18
            }
        }
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

/// Where Hannibal's unit types start among the type ids: Alexander's take
/// `0..HANNIBAL`, Hannibal's follow, in the order its executable registers
/// them (the order its `GetIndexOfUnitType` answers in).
pub const HANNIBAL: usize = ART_TITLES.len();

/// What a unit type is, for the rules and the drawing (the original's
/// presentation and game classes, grouped as the rewrite uses them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Leader,
    HeavyInfantry,
    Phalanx,
    MediumInfantry,
    /// Light infantry that can shock (the "shocking javelinists").
    LightInfantryShock,
    LightInfantry,
    Skirmisher,
    /// Roman legionaries: hastati, principes, cohorts.
    Legion,
    LightCavalry,
    Lancer,
    HeavyCavalry,
    Elephant,
    Chariot,
    /// Caesar's barbarians: foot and horse that fight only hand to hand.
    BarbarianInfantry,
    BarbarianCavalry,
    /// Caesar's scorpions and ballistas.
    Artillery,
}

/// Hannibal's unit types: the EIF title of each one's sprite set and its
/// class.
pub const HANNIBAL_TYPES: [(&str, Class); 54] = {
    use Class::*;
    [
        ("Hannibal", Leader),
        ("Scipio", Leader),
        ("Antiochus", Leader),
        ("Carthaginian Leader", Leader),
        ("Greek Leader", Leader),
        ("Roman Leader", Leader),
        ("Persian Leader", Leader),
        ("Triarii", HeavyInfantry),
        ("AS Triarii", HeavyInfantry),
        ("Hoplite", HeavyInfantry),
        ("Numidian Heavy Infantry", HeavyInfantry),
        ("Hypaspist", HeavyInfantry),
        ("Phalanx", Phalanx),
        ("Celtic Medium Infantry (1)", MediumInfantry),
        ("Celtic Medium Infantry (2)", MediumInfantry),
        ("Celtic Medium Infantry (3)", MediumInfantry),
        ("Numidian Medium Infantry", MediumInfantry),
        ("Iberian Medium Infantry", MediumInfantry),
        ("Gallic Medium Infantry", MediumInfantry),
        ("Persian Medium Infantry", MediumInfantry),
        ("Athamanian Medium Infantry", MediumInfantry),
        ("Numidian Light Infantry", LightInfantry),
        ("Velites", LightInfantry),
        ("AS Velites", LightInfantry),
        ("Iberian Light Infantry", LightInfantryShock),
        ("Greek Light Infantry", LightInfantryShock),
        ("Macedonian Light Infantry", LightInfantryShock),
        ("Persian Light Infantry", LightInfantryShock),
        ("Balaeric Slinger", Skirmisher),
        ("Iberian Archers", Skirmisher),
        ("Persian Javelin", Skirmisher),
        ("Persian Archers", Skirmisher),
        ("Persian Slingers", Skirmisher),
        ("Macedonian Archers", Skirmisher),
        ("Macedonian Slingers", Skirmisher),
        ("Hastati", Legion),
        ("Principe", Legion),
        ("Cohort", Legion),
        ("Numidian Light Cavalry", LightCavalry),
        ("Persian Light Cavalry (Archers)", LightCavalry),
        ("Greek Cavalry", LightCavalry),
        ("Celtic Lancer", Lancer),
        ("Gallic Lancer", Lancer),
        ("Roman Cavalry", HeavyCavalry),
        ("AS Roman Cavalry", HeavyCavalry),
        ("Iberian Lancer", Lancer),
        ("Carthaginian Heavy Cavalry", HeavyCavalry),
        ("Persian Heavy Cavalry", HeavyCavalry),
        ("Persian Cataphracted Cavalry", HeavyCavalry),
        ("Persian Lancers", Lancer),
        ("Macedonian Lancer", Lancer),
        ("African Elephant", Elephant),
        ("Indian Elephant (Javelin)", Elephant),
        ("Persian Chariots", Chariot),
    ]
};

/// Where Caesar's unit types start among the type ids (after Hannibal's).
pub const CAESAR: usize = HANNIBAL + HANNIBAL_TYPES.len();

/// Caesar's unit types (its registry, `FUN_0047ef74`): title, class and the
/// missile art its men throw (its table: 0 arrows, 1 bolt, 2 javelins,
/// 3 axes, 4 mounted javelins, 5 light javelins, 6 pila, 7 stones).
pub const CAESAR_TYPES: [(&str, Class, Option<u8>); 59] = {
    use Class::*;
    [
        ("J. Caesar", Leader, None),
        ("Pompey", Leader, None),
        ("Caesarian Leader", Leader, None),
        ("Pompeian Leader", Leader, None),
        ("Beorix", Leader, None),
        ("Pontine Leader", Leader, None),
        ("Tribal Leader", Leader, None),
        ("Pontine Heavy Infantry", HeavyInfantry, None),
        ("Phalanx", Phalanx, None),
        ("Marines", LightInfantry, Some(5)),
        ("Numidian Light Infantry", LightInfantryShock, Some(5)),
        ("Pontine Light Infantry", LightInfantryShock, Some(2)),
        ("Tribal Light Infantry", LightInfantryShock, Some(2)),
        ("Tribal Light Infantry B", LightInfantryShock, Some(2)),
        ("Tribal Light Infantry C", LightInfantryShock, Some(2)),
        ("Tribal Archers", LightInfantry, Some(0)),
        ("Tribal Axemen", LightInfantry, Some(3)),
        ("Tribal Axemen B", LightInfantry, Some(3)),
        ("Tribal Axemen C", LightInfantry, Some(3)),
        ("Pontine Medium Infantry", MediumInfantry, None),
        ("Tribal Barbarian Infantry", BarbarianInfantry, None),
        ("Tribal Barbarian Infantry B", BarbarianInfantry, None),
        ("Tribal Barbarian Infantry C", BarbarianInfantry, None),
        ("Gallic Medium Infantry", BarbarianInfantry, None),
        ("Celtic Medium Infantry", BarbarianInfantry, None),
        ("Celtic Medium Infantry B", BarbarianInfantry, None),
        ("Celtic Medium Infantry C", BarbarianInfantry, None),
        ("Caesarian Archers", Skirmisher, Some(0)),
        ("Pompeian Archers", Skirmisher, Some(0)),
        ("Caesarian Slingers", Skirmisher, Some(7)),
        ("Pompeian Slingers", Skirmisher, Some(7)),
        ("Numidian Archers", Skirmisher, Some(0)),
        ("Numidian Slingers", Skirmisher, Some(7)),
        ("Pontine Archers", Skirmisher, Some(0)),
        ("Pontine Slingers", Skirmisher, Some(7)),
        ("Caesarian Cohort Veteran", Legion, Some(6)),
        ("Caesarian Cohort Recruit", Legion, Some(6)),
        ("Caesarian Cohort Conscript", Legion, Some(6)),
        ("Pompeian Cohort Veteran", Legion, Some(6)),
        ("Pompeian Cohort Recruit", Legion, Some(6)),
        ("Pompeian Cohort Conscript", Legion, Some(6)),
        ("Numidian Light Cavalry", LightCavalry, Some(4)),
        ("Asian Light Cavalry (Archers)", LightCavalry, Some(0)),
        ("Greek Light Cavalry", LightCavalry, Some(4)),
        ("Caesarian Roman Cavalry", HeavyCavalry, None),
        ("Pompeian Roman Cavalry", HeavyCavalry, None),
        ("Pontine Heavy Cavalry", HeavyCavalry, None),
        ("Pontine Cataphracted Cavalry", HeavyCavalry, None),
        ("German Barbarian Cavalry", BarbarianCavalry, None),
        ("Tribal Barbarian Cavalry", BarbarianCavalry, None),
        ("Gallic Lancer", Lancer, None),
        ("African Elephant", Elephant, Some(4)),
        ("Pontine Chariots", Chariot, Some(5)),
        ("Pompeian Scorpion", Artillery, Some(1)),
        ("Caesarian Ballista", Artillery, Some(1)),
        ("Caesarian Scorpion", Artillery, Some(1)),
        ("Scorpion", Artillery, Some(1)),
        ("Ballista", Artillery, Some(1)),
        ("Pompeian Ballista", Artillery, Some(1)),
    ]
};

/// How many unit type ids there are (all games').
pub const TYPE_COUNT: usize = CAESAR + CAESAR_TYPES.len();

/// The unit type ids of `game`'s units (Caesar's: none yet).
pub fn types_of(game: crate::game::Game) -> std::ops::Range<usize> {
    use crate::game::Game;
    match game {
        Game::Alexander => 0..HANNIBAL,
        Game::Hannibal => HANNIBAL..CAESAR,
        Game::Caesar => CAESAR..TYPE_COUNT,
    }
}

/// The EIF title of unit type `ty`'s sprite set.
pub fn title(ty: usize) -> Option<&'static str> {
    ART_TITLES
        .get(ty)
        .copied()
        .or_else(|| HANNIBAL_TYPES.get(ty.checked_sub(HANNIBAL)?).map(|t| t.0))
        .or_else(|| CAESAR_TYPES.get(ty.checked_sub(CAESAR)?).map(|t| t.0))
}

/// The class of one of Hannibal's or Caesar's unit types (`None` for
/// Alexander's, whose classes the rules and the renderer list by id).
pub fn hannibal_class(ty: usize) -> Option<Class> {
    if ty >= CAESAR {
        return CAESAR_TYPES.get(ty - CAESAR).map(|t| t.1);
    }
    HANNIBAL_TYPES.get(ty.checked_sub(HANNIBAL)?).map(|t| t.1)
}

/// The missile art a Caesar unit type's men throw (its table's index).
pub fn caesar_missile_art(ty: usize) -> Option<u8> {
    CAESAR_TYPES.get(ty.checked_sub(CAESAR)?)?.2
}

/// Whether unit type `ty` is cataphracted cavalry.
pub fn cataphract(ty: usize) -> bool {
    matches!(ty, 41 | 44) || hannibal_title_is(ty, "Persian Cataphracted Cavalry") || hannibal_title_is(ty, "Pontine Cataphracted Cavalry")
}

/// Whether unit type `ty` is a chariot.
pub fn chariot(ty: usize) -> bool {
    matches!(ty, 22 | 23) || hannibal_class(ty) == Some(Class::Chariot)
}

/// Whether unit type `ty` takes two hexes.
pub fn two_hex(ty: usize) -> bool {
    matches!(ty, 17 | 18) || hannibal_class(ty) == Some(Class::Phalanx)
}

fn hannibal_title_is(ty: usize, t: &str) -> bool {
    ty >= HANNIBAL && title(ty) == Some(t)
}

/// The type id of one of Hannibal's titles.
fn hannibal(t: &str) -> Option<usize> {
    HANNIBAL_TYPES.iter().position(|e| e.0 == t).map(|k| HANNIBAL + k)
}

/// The type id of one of Caesar's titles.
fn caesar(t: &str) -> Option<usize> {
    CAESAR_TYPES.iter().position(|e| e.0 == t).map(|k| CAESAR + k)
}

/// The unit type id Caesar's game assigns to a scenario unit (its
/// `JCLL::readUnits`): by kind, the nation its art goes by (the header's
/// nation pairs applied), names (leaders' exactly), missile byte, cohort
/// type; its random picks among a set's A, B and C art follow here from
/// the name (🟡 not its random sequence).
pub fn caesar_unit_type(u: &Unit) -> Option<usize> {
    // the nation byte remapped as by the game: 3 Pontine, 6 Roman (drawn
    // as Pompeian), 9 Caesarian, 10 Pompeian, 11 barbarians
    let n = match u.art_nation {
        0 => 3,
        1 => 1,
        2 => 0,
        3 => 2,
        b @ 4..=11 => b,
        _ => return None,
    };
    let has = |s: &str| u.name.contains(s);
    let pick = |set: [&'static str; 3]| set[u.name.bytes().map(|b| b as usize).sum::<usize>() % 3];
    let (missile, more, legion) = match u.extra {
        UnitExtra::Leader(_) => (0, None, None),
        UnitExtra::Combat { more, .. } => (0, more, None),
        UnitExtra::Missile { missile, more, legion, .. } => (missile, more, legion),
    };
    // the scenario's missile byte: 1 bow, 2 sling, 3 javelin, 4 bolt, 5 axe
    let title: String = match (u.kind, n) {
        (0, 3) => "Pontine Leader".into(),
        (0, 6) => "Pompeian Leader".into(),
        (0, 9) => if u.name == "J. Caesar" { "J. Caesar" } else { "Caesarian Leader" }.into(),
        (0, 10) => if u.name == "Pompey" { "Pompey" } else { "Pompeian Leader" }.into(),
        (0, 11) => if u.name == "Beorix" { "Beorix" } else { "Tribal Leader" }.into(),
        (1 | 17, 6) => "Scorpion".into(),
        (1 | 17, 9) => if has("Sco") { "Caesarian Scorpion" } else { "Caesarian Ballista" }.into(),
        (1 | 17, 10) => if has("Sco") { "Pompeian Scorpion" } else { "Pompeian Ballista" }.into(),
        (2, 3) => if more.unwrap_or(0) != 0 { "Pontine Cataphracted Cavalry" } else { "Pontine Heavy Cavalry" }.into(),
        (3, 3) => if missile == 1 { "Asian Light Cavalry (Archers)" } else { "Greek Light Cavalry" }.into(),
        (3, 9) => "Numidian Light Cavalry".into(),
        (3, 10) => if missile == 1 {
            "Asian Light Cavalry (Archers)"
        } else if has("Numid") {
            "Numidian Light Cavalry"
        } else {
            "Greek Light Cavalry"
        }
        .into(),
        (4, 3) => "Pontine Chariots".into(),
        (5, 10) => "African Elephant".into(),
        (6 | 14, 3) => "Pontine Heavy Infantry".into(),
        (7, 3) => "Pontine Light Infantry".into(),
        (7, 6 | 10) => if has("Mar") { "Marines" } else { "Numidian Light Infantry" }.into(),
        (7, 9) => "Numidian Light Infantry".into(),
        (7, 11) => match missile {
            1 => "Tribal Archers",
            3 => pick(["Tribal Light Infantry", "Tribal Light Infantry B", "Tribal Light Infantry C"]),
            5 => pick(["Tribal Axemen", "Tribal Axemen B", "Tribal Axemen C"]),
            _ => "Tribal Light Infantry",
        }
        .into(),
        (8, 3) => "Pontine Medium Infantry".into(),
        (9, 6 | 9 | 10 | 11) => "Gallic Lancer".into(),
        (10, 3 | 6 | 9 | 10) => {
            let base = match missile {
                1 => "Archers",
                2 => "Slingers",
                _ => return None,
            };
            let prefix = match n {
                3 => "Pontine",
                9 => "Caesarian",
                _ if has("Numid") => "Numidian",
                _ => "Pompeian",
            };
            format!("{prefix} {base}")
        }
        (19, 3) => "Phalanx".into(),
        (22, 3 | 9) => "Caesarian Roman Cavalry".into(),
        (22, 6 | 10) => "Pompeian Roman Cavalry".into(),
        (23, 11) => match missile {
            5 => pick(["Tribal Axemen", "Tribal Axemen B", "Tribal Axemen C"]),
            0 if has("Hel") => "Gallic Medium Infantry",
            0 if has("Tul") || has("Ver") => pick(["Celtic Medium Infantry", "Celtic Medium Infantry B", "Celtic Medium Infantry C"]),
            0 => pick(["Tribal Barbarian Infantry", "Tribal Barbarian Infantry B", "Tribal Barbarian Infantry C"]),
            _ => return None,
        }
        .into(),
        (24, 6 | 9 | 10) => "German Barbarian Cavalry".into(),
        (24, 11) => "Tribal Barbarian Cavalry".into(),
        (25, 6 | 9 | 10) => {
            let kind = match legion? {
                1 => "Veteran",
                2 => "Recruit",
                3 => "Conscript",
                _ => return None,
            };
            format!("{} Cohort {kind}", if n == 9 { "Caesarian" } else { "Pompeian" })
        }
        _ => return None,
    };
    caesar(&title)
}

/// The unit type id Hannibal's game assigns to a scenario unit (its
/// `ALL::readUnits`: by kind, nation, name and fields), or `None` for
/// units it rejects or leaves without a type of their own.
pub fn hannibal_unit_type(u: &Unit) -> Option<usize> {
    // the scenario's nation byte, remapped as by the game: 0 Macedonian,
    // 3 Greek, 6 Roman, 7 Carthaginian, 8 Seleucid
    let n = match u.nation {
        0 => 3,
        1 => 1,
        2 => 0,
        3 => 2,
        b @ 4..=8 => b,
        _ => return None,
    };
    let has = |s: &str| u.name.contains(s);
    let (missile, more, legion) = match u.extra {
        UnitExtra::Leader(_) => (0, None, None),
        UnitExtra::Combat { more, .. } => (0, more, None),
        UnitExtra::Missile { missile, more, legion, .. } => (missile, more, legion),
    };
    let title = match (u.kind, n) {
        (0, 0 | 3) => "Greek Leader",
        (0, 6) => {
            if has("P. Scipio") {
                "Scipio"
            } else {
                "Roman Leader"
            }
        }
        (0, 7) => {
            if has("Hannibal") {
                "Hannibal"
            } else if has("Xan") {
                "Greek Leader"
            } else {
                "Carthaginian Leader"
            }
        }
        (0, 8) => {
            if has("Antioch") {
                "Antiochus"
            } else {
                "Persian Leader"
            }
        }
        (2, 0 | 6 | 7) => "Carthaginian Heavy Cavalry",
        (2, 8) => {
            if more.unwrap_or(0) != 0 {
                "Persian Cataphracted Cavalry"
            } else {
                "Persian Heavy Cavalry"
            }
        }
        (3, 6 | 7) => {
            if has("Aetolia") {
                "Greek Cavalry"
            } else if has("Taren") {
                "Macedonian Lancer"
            } else {
                "Numidian Light Cavalry"
            }
        }
        // the scenario's missile byte: 0 none, 1 bow
        (3, 8) => match missile {
            0 => "Persian Lancers",
            1 => "Persian Light Cavalry (Archers)",
            _ => return None,
        },
        (4, 8) => "Persian Chariots",
        (5, 6 | 7) => "African Elephant",
        (5, 8) => "Indian Elephant (Javelin)",
        (6 | 14, 0 | 3 | 7 | 8) => {
            if ["Iber", "Liby", "Africa", "Macedon", "Mazedon"].iter().any(|s| has(s)) {
                "Hypaspist"
            } else if has("Numid") {
                "Numidian Heavy Infantry"
            } else {
                "Hoplite"
            }
        }
        (6 | 14, 6) => {
            if has("Triarii") {
                if has("AS") { "AS Triarii" } else { "Triarii" }
            } else if has("Macedon") {
                "Hypaspist"
            } else {
                "Hoplite"
            }
        }
        (7, 0 | 7 | 8) => {
            if has("Numid") || has("Africa") {
                "Numidian Light Infantry"
            } else if has("Thra") || has("Illyr") {
                "Greek Light Infantry"
            } else if has("Macedon") {
                "Macedonian Light Infantry"
            } else if has("Capp") {
                "Persian Light Infantry"
            } else {
                "Iberian Light Infantry"
            }
        }
        (7, 6) => {
            if has("Iber") {
                "Iberian Light Infantry"
            } else if has("Numid") {
                "Numidian Light Infantry"
            } else if has("Ach") {
                "Greek Light Infantry"
            } else if has("Pergamum") {
                "Macedonian Light Infantry"
            } else if has("Velite") {
                if has("AS") { "AS Velites" } else { "Velites" }
            } else {
                return None;
            }
        }
        (8, 0) => "Hypaspist",
        (8, 6) => {
            if has("Numid") {
                "Numidian Medium Infantry"
            } else if has("Athaman") {
                "Athamanian Medium Infantry"
            } else {
                "Hypaspist"
            }
        }
        (8, 7) => {
            if has("Celt") {
                // the game draws one of three at random (`rand() % 3`) as
                // it loads; here it follows from the name (🟡 not its
                // random sequence)
                ["Celtic Medium Infantry (1)", "Celtic Medium Infantry (2)", "Celtic Medium Infantry (3)"]
                    [u.name.bytes().map(|b| b as usize).sum::<usize>() % 3]
            } else if has("Gaul") || has("Liguria") {
                "Gallic Medium Infantry"
            } else if has("Afri") {
                "Numidian Medium Infantry"
            } else {
                "Iberian Medium Infantry"
            }
        }
        (8, 8) => "Persian Medium Infantry",
        (9, 6) => "Macedonian Lancer",
        (9, 7) => {
            if has("Celt") {
                "Celtic Lancer"
            } else if has("Gaul") {
                "Gallic Lancer"
            } else {
                "Iberian Lancer"
            }
        }
        (9, 8) => {
            if has("Taren") {
                "Macedonian Lancer"
            } else {
                "Persian Lancers"
            }
        }
        // skirmishers; the scenario's missile byte: 1 bow, 2 sling, 3 javelin
        (10, 6) => match missile {
            1 => {
                if has("Crete") {
                    "Macedonian Archers"
                } else {
                    "Iberian Archers"
                }
            }
            2 => {
                if has("Trall") {
                    "Macedonian Slingers"
                } else {
                    "Balaeric Slinger"
                }
            }
            _ => return None,
        },
        (10, 7) => {
            if has("Iber") {
                "Iberian Archers"
            } else {
                "Balaeric Slinger"
            }
        }
        (10, 8) => match missile {
            1 => {
                if has("Crete") {
                    "Macedonian Archers"
                } else {
                    "Persian Archers"
                }
            }
            2 => {
                if has("Trall") {
                    "Macedonian Slingers"
                } else {
                    "Persian Slingers"
                }
            }
            3 => "Persian Javelin",
            _ => return None,
        },
        (19, 0 | 7 | 8) => "Phalanx",
        (21, _) => match legion? {
            1 => "Hastati",
            2 => "Principe",
            3 => "Cohort",
            _ => return None,
        },
        (22, 6) => {
            if has("AS") {
                "AS Roman Cavalry"
            } else {
                "Roman Cavalry"
            }
        }
        _ => return None,
    };
    hannibal(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every unit of Caesar's scenarios (but its scenario builder's file)
    /// gets a type whose sprite set is there; runs only with
    /// `GBOH_CAESAR` (its `data` directory) set.
    #[test]
    fn caesar_units_have_art() {
        let Ok(dir) = std::env::var("GBOH_CAESAR") else { return };
        let titles: std::collections::HashSet<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().to_ascii_uppercase().ends_with(".EIF"))
            .filter_map(|e| crate::eif::Eif::parse(&std::fs::read(e.path()).ok()?).ok().map(|e| e.title))
            .collect();
        let mut units = 0;
        for e in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_ascii_uppercase();
            if !name.ends_with(".SCN") || name == "LAST.SCN" {
                continue;
            }
            let scn = crate::scn::Scenario::parse(&std::fs::read(e.path()).unwrap()).unwrap();
            assert_eq!(scn.layout, crate::scn::Layout::Caesar, "{name}");
            for u in &scn.units {
                let ty = unit_type(u).unwrap_or_else(|| panic!("{name}: {} (kind {}, nation {})", u.name, u.kind, u.art_nation));
                assert!(titles.contains(title(ty).unwrap()), "{name}: {}", title(ty).unwrap());
                units += 1;
            }
        }
        assert!(units > 3000, "{units}");
    }

    /// Every unit of Hannibal's scenarios gets a type whose sprite set is
    /// there; runs only with `GBOH_HANNIBAL` (its `Data` directory) set.
    #[test]
    fn hannibal_units_have_art() {
        let Ok(dir) = std::env::var("GBOH_HANNIBAL") else { return };
        let titles: std::collections::HashSet<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().to_ascii_uppercase().ends_with(".EIF"))
            .filter_map(|e| crate::eif::Eif::parse(&std::fs::read(e.path()).ok()?).ok().map(|e| e.title))
            .collect();
        let mut units = 0;
        for e in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_ascii_uppercase();
            if !name.ends_with(".SCN") || name == "CANNAE2.SCN" {
                continue;
            }
            let scn = crate::scn::Scenario::parse(&std::fs::read(e.path()).unwrap()).unwrap();
            assert_eq!(scn.layout, crate::scn::Layout::Hannibal, "{name}");
            for u in &scn.units {
                let ty = hannibal_unit_type(u).unwrap_or_else(|| panic!("{name}: {} (kind {})", u.name, u.kind));
                assert!(titles.contains(title(ty).unwrap()), "{name}: {}", title(ty).unwrap());
                units += 1;
            }
        }
        assert!(units > 3000, "{units}");
    }

    fn unit(kind: u8, nation: u8, name: &str, extra: UnitExtra) -> Unit {
        Unit {
            kind,
            name: name.into(),
            nation,
            unk_a: 0,
            unk_b: 0,
            hex: 0,
            facing: 1,
            extra,
            arrival: 0,
            elite: 0,
            link: -1,
            layout: crate::scn::Layout::Alexander,
            art_nation: nation,
        }
    }

    fn combat(c: u8) -> UnitExtra {
        UnitExtra::Combat { a: 0, b: 0, c, more: None }
    }

    fn missile(m: u8) -> UnitExtra {
        UnitExtra::Missile { a: 0, b: 0, c: 0, missile: m, m1: 0, m2: 0, more: None, legion: None }
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
