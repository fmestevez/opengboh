//! Missile fire: volleys and reaction fire (see `docs/formats.md`,
//! "Missile fire").

use crate::movement::MoveClass;

/// Missile weapons as the game numbers them internally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weapon {
    Bow = 1,
    Sling = 3,
    Javelin = 4,
    /// Bows on light cavalry.
    MountedBow = 6,
    /// Javelins on light cavalry.
    MountedJavelin = 7,
    Bolt = 8,
    /// Caesar's barbarians' thrown axes.
    Axe = 9,
}

impl Weapon {
    /// The weapon of a unit with scenario missile byte `missile` (1 bow,
    /// 2 sling, 3 javelin, 4 bolt) and movement class `class`.
    pub fn of(missile: u8, class: MoveClass) -> Option<Self> {
        let mounted = class == MoveClass::LightCavalry;
        Some(match missile {
            1 if mounted => Weapon::MountedBow,
            1 => Weapon::Bow,
            2 => Weapon::Sling,
            3 if mounted => Weapon::MountedJavelin,
            3 => Weapon::Javelin,
            5 => Weapon::Axe,
            4 => Weapon::Bolt,
            _ => return None,
        })
    }

    /// Missiles carried at the start of a battle (from the manual).
    pub fn supply(self) -> u16 {
        match self {
            Weapon::Javelin | Weapon::MountedJavelin => 3,
            Weapon::Axe => 2,
            Weapon::Bolt => 99,
            _ => 10,
        }
    }

    /// Longest range with any chance of a hit.
    pub fn range(self) -> i32 {
        match self {
            Weapon::Bolt => 7,
            Weapon::Axe => 1,
            w => (1..7).rev().find(|&r| HIT_ON[w as usize][r] > 0).unwrap_or(1) as i32,
        }
    }
}

/// Highest modified roll that hits, by weapon and range (0..=6). Rows 0, 2
/// and 5 belong to no weapon.
const HIT_ON: [[u8; 7]; 9] = [
    [4, 0, 0, 0, 6, 0, 0],
    [0, 5, 3, 3, 1, 0, 0],
    [0, 9, 6, 5, 2, 0, 0],
    [0, 3, 2, 0, 0, 0, 0],
    [0, 5, 0, 0, 0, 0, 0],
    [0, 3, 0, 0, 0, 0, 0],
    [0, 5, 3, 1, 0, 0, 0],
    [0, 3, 0, 0, 0, 0, 0],
    [0, 7, 5, 5, 3, 2, 1],
];

/// The table entry. (A bolt at range 7 reads past the original's table,
/// the byte after it, 85, so it always hit there; fixed: it hits on 0, the
/// row's 3, 2, 1 going on down.)
fn hit_on(weapon: Weapon, range: i32) -> i32 {
    match (weapon, range) {
        (Weapon::Bolt, 7) => 0,
        // (Caesar's axes: its table's new row)
        (Weapon::Axe, 1) => 7,
        (Weapon::Axe, _) => 0,
        (w, r) if (0..7).contains(&r) => HIT_ON[w as usize][r as usize] as i32,
        _ => 0,
    }
}

/// One volley.
#[derive(Debug, Clone, Copy)]
pub struct Volley {
    pub weapon: Weapon,
    /// Distance to the target in hexes.
    pub range: i32,
    pub target: MoveClass,
    /// The shooter stands in one of the target's front hexes.
    pub from_front: bool,
    /// The shooter has moved this turn (does not apply to javelins).
    pub moved: bool,
    /// The target is cataphracted heavy cavalry.
    pub target_cataphract: bool,
    /// Caesar's: a ballista's bolts, by its own range table; and a further
    /// modifier (a shooter that has rallied +1, barbarian bows −1).
    pub ballista: bool,
    pub bonus: i32,
}

/// Caesar's ballistas' table by range (`0x63cf91`).
const BALLISTA: [i32; 8] = [0, 9, 7, 6, 4, 3, 2, 1];

/// The result of a volley.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolleyResult {
    pub hits: i32,
    /// The die came up 0 (the original records this; its use is not
    /// traced yet).
    pub zero: bool,
}

impl Volley {
    /// Reaction fire at a unit entering the shooter's zone of control.
    pub fn reaction(weapon: Weapon, target: MoveClass, target_cataphract: bool) -> Self {
        Volley { weapon, range: 1, target, from_front: false, moved: false, target_cataphract, ballista: false, bonus: 0 }
    }

    /// The modifier added to the die.
    pub fn modifier(&self) -> i32 {
        use MoveClass::*;
        let w = self.weapon;
        let armoured = matches!(self.target, Phalanx | HeavyInfantry);
        let mut m = 0;
        if armoured {
            m += 1;
            if self.range == 1 && self.from_front && w != Weapon::Javelin && w != Weapon::MountedJavelin {
                m += 3;
            }
        }
        if w != Weapon::Javelin && self.moved {
            m += 1;
        }
        if self.target_cataphract {
            m += match w {
                Weapon::Bow | Weapon::MountedBow => 2,
                Weapon::Sling | Weapon::Javelin | Weapon::MountedJavelin | Weapon::Axe => 1,
                Weapon::Bolt => 0,
            };
        }
        m + self.bonus
    }

    /// Resolve with die `roll` (0..=9, `rand() % 10` in the original): one
    /// hit if `roll + modifier` is at most the table entry, two against
    /// elephants and chariots.
    pub fn resolve(&self, roll: u32) -> VolleyResult {
        let table = if self.ballista { BALLISTA.get(self.range as usize).copied().unwrap_or(0) } else { hit_on(self.weapon, self.range) };
        let hit = roll as i32 + self.modifier() <= table;
        let hits = match (hit, self.target) {
            (false, _) => 0,
            (true, MoveClass::Elephant | MoveClass::Chariot) => 2,
            (true, _) => 1,
        };
        VolleyResult { hits, zero: roll == 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MoveClass::*;

    #[test]
    fn ranges_match_the_manual() {
        assert_eq!(Weapon::Bow.range(), 4);
        assert_eq!(Weapon::Sling.range(), 2);
        assert_eq!(Weapon::Javelin.range(), 1);
        assert_eq!(Weapon::MountedBow.range(), 3);
        assert_eq!(Weapon::MountedJavelin.range(), 1);
        assert_eq!(Weapon::Bolt.range(), 7);
    }

    #[test]
    fn reaction_javelins_hit_on_five_or_less() {
        // India 3's javelins at the Agema Companions (Hydaspes)
        let v = Volley::reaction(Weapon::Javelin, HeavyCavalry, false);
        assert_eq!(v.resolve(5).hits, 1);
        assert_eq!(v.resolve(6).hits, 0);
    }

    #[test]
    fn shields_and_armour() {
        let v = Volley { weapon: Weapon::Bow, range: 1, target: Phalanx, from_front: true, moved: false, target_cataphract: false, ballista: false, bonus: 0 };
        assert_eq!(v.modifier(), 4);
        assert_eq!(v.resolve(1).hits, 1);
        assert_eq!(v.resolve(2).hits, 0);
        let e = Volley { target: Elephant, from_front: false, ..v };
        assert_eq!(e.resolve(5).hits, 2);
    }

    #[test]
    fn bolts_at_range_seven_hit_only_on_zero() {
        let v = Volley { weapon: Weapon::Bolt, range: 7, target: Skirmisher, from_front: false, moved: false, target_cataphract: false, ballista: false, bonus: 0 };
        assert_eq!(v.resolve(9).hits, 0);
        assert_eq!(v.resolve(0).hits, 1);
    }
}
