//! Shock combat: the column, die roll and results the original uses (see
//! `docs/formats.md`, "Shock combat").
//!
//! The tables come from the game's data and agree with the manual's
//! "Combat Tables" (two results cells differ; the game's values are kept).
//! The game does not apply the manual's terrain shifts or skirmisher
//! halving (its terrain shift argument is always 0), so neither does this.
//! Who attacks whom, the pre-shock checks, leader casualties and routs are
//! not ported yet.

use crate::movement::MoveClass;

/// One shock: attackers against a defender.
#[derive(Debug, Clone, Copy)]
pub struct Shock {
    /// The attacking units' class (the leading attacker's).
    pub attacker: MoveClass,
    /// The defender's class.
    pub defender: MoveClass,
    /// 0 front, 1 flank, 2 rear.
    pub angle: u8,
    /// Total size of the attackers and size of the defender.
    pub attacker_size: i32,
    pub defender_size: i32,
    /// Cataphracts (the heavy cavalry flag): +2 columns when the leading
    /// attacker is one, −2 when the defender is.
    pub attacker_cataphract: bool,
    pub defender_cataphract: bool,
    /// Whether the defender can fire missiles.
    pub defender_missile: bool,
    /// Turns superiority off (set by the original when other units are
    /// involved in a particular way; not yet understood).
    pub no_superiority: bool,
    /// Further column shift.
    pub column_shift: i32,
    /// Hannibal's rules, and the sides' nations (scenario bytes: 6 Roman,
    /// 7 Carthaginian), which some of its elephant rules go by.
    pub hannibal: bool,
    pub attacker_nation: u8,
    pub defender_nation: u8,
}

/// Hannibal's Roman and Carthaginian nations.
const ROMAN: u8 = 6;
const CARTHAGINIAN: u8 = 7;

/// The result of a shock: cohesion hits for the attackers (shared among
/// them) and for the defender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShockResult {
    pub column: usize,
    pub attacker_hits: i32,
    pub defender_hits: i32,
}

impl Shock {
    /// Which side is superior: `(attacker, defender)`. A superior attacker
    /// doubles the defender's hits; a superior defender triples the
    /// attackers'.
    pub fn superiority(&self) -> (bool, bool) {
        use MoveClass::*;
        let (a, d) = (self.attacker as usize, self.defender as usize);
        let horse_or_lancer = |c: MoveClass| matches!(c, HeavyCavalry | Lancer | LightCavalry) || [9, 10].contains(&(c as usize));
        let mut att = self.angle == 1 || self.angle == 2;
        if self.defender == Skirmisher && self.angle == 1 {
            att = false;
        }
        if self.defender == Elephant && horse_or_lancer(self.attacker) {
            att = false;
        }
        if self.no_superiority {
            att = false;
        }
        // Hannibal's: Roman elephants attacked by others' elephants
        if self.hannibal && self.attacker == Elephant && self.defender == Elephant && self.attacker_nation != ROMAN && self.defender_nation == ROMAN {
            att = true;
        }
        let mut def = false;
        // Alexander consults the table only for an attacker not already
        // superior; Hannibal always, last, so that both sides can be
        if !att || self.hannibal {
            match SUPERIORITY[d][a] {
                1 => att = true,
                2 => {
                    def = true;
                    // exceptions where the defender's superiority does not
                    // apply (classes 1, 4, 9 and 10 have table entries but no
                    // unit type uses them)
                    let d_inf = [1, 2].contains(&d);
                    if d_inf && self.attacker_cataphract && self.attacker == HeavyCavalry {
                        def = false;
                    }
                    if self.defender_missile && d_inf && (self.attacker == HeavyCavalry || a == 10) {
                        def = false;
                    }
                    if self.defender_missile && [1, 2, 3].contains(&d) && a == 9 {
                        def = false;
                    }
                    if self.defender_missile && [1, 2, 3, 5, 6].contains(&d) && self.attacker == LightCavalry {
                        def = false;
                    }
                }
                _ => {}
            }
        }
        (att, def)
    }

    /// The column (0..=12): the base for the classes and angle, −1, the size
    /// ratio, and the shifts.
    pub fn column(&self) -> usize {
        let (a, d) = (self.attacker as usize, self.defender as usize);
        let (sa, sd) = (self.attacker_size.max(1), self.defender_size.max(1));
        let size = if sa < sd {
            1 - (sd as f32 / sa as f32).ceil() as i32
        } else if sd < sa {
            sa / sd - 1
        } else {
            0
        };
        let mut col = COLUMN[d][self.angle as usize][a] as i32 - 1 + size;
        if d == 0 && self.angle == 0 {
            // a phalanx attacked from the front
            col -= 3;
        }
        if self.attacker_cataphract {
            col += 2;
        }
        if self.defender_cataphract {
            col -= 2;
        }
        // Hannibal's: elephants other than the Carthaginians' against other
        // troops
        if self.hannibal && self.attacker == MoveClass::Elephant && self.attacker_nation != CARTHAGINIAN && self.defender != MoveClass::Elephant {
            col += 1;
        }
        (col + self.column_shift).clamp(0, 12) as usize
    }

    /// Resolve the shock with die roll `roll` (0..=9, `rand() % 10` in the
    /// original).
    pub fn resolve(&self, roll: u32) -> ShockResult {
        let column = self.column();
        let row = roll.min(9) as usize;
        let (att_sup, def_sup) = self.superiority();
        let mut defender_hits = DEFENDER_HITS[row][column] as i32;
        let mut attacker_hits = ATTACKER_HITS[row][column] as i32;
        if att_sup {
            defender_hits *= 2;
        }
        if def_sup {
            attacker_hits *= 3;
        }
        ShockResult { column, attacker_hits, defender_hits }
    }
}

/// Base shock column by defender class, attack angle (front, flank,
/// rear) and attacker class.
const COLUMN: [[[i8; 16]; 3]; 16] = [
    [
        [7, 5, 6, 5, 5, 4, 4, 1, 5, 5, 5, 4, 3, 6, 8, 0],
        [12, 12, 11, 10, 10, 9, 9, 6, 10, 10, 10, 9, 8, 13, 13, 0],
        [13, 13, 12, 12, 12, 10, 10, 7, 11, 11, 11, 11, 9, 13, 13, 0],
    ],
    [
        [7, 7, 6, 7, 6, 5, 5, 2, 7, 7, 7, 4, 4, 7, 7, 0],
        [12, 9, 11, 9, 8, 9, 9, 5, 9, 9, 9, 8, 7, 13, 11, 0],
        [13, 11, 12, 11, 10, 11, 11, 7, 10, 10, 10, 10, 9, 13, 13, 0],
    ],
    [
        [8, 6, 7, 6, 7, 5, 5, 2, 6, 6, 6, 5, 4, 7, 9, 0],
        [11, 11, 10, 10, 9, 8, 8, 5, 9, 9, 9, 8, 7, 13, 13, 0],
        [13, 12, 12, 12, 11, 10, 10, 7, 11, 11, 11, 11, 9, 13, 13, 0],
    ],
    [
        [9, 8, 8, 7, 8, 6, 6, 3, 7, 7, 7, 6, 5, 9, 10, 0],
        [11, 11, 10, 10, 10, 9, 9, 5, 8, 8, 8, 7, 6, 13, 13, 0],
        [12, 12, 12, 12, 13, 11, 11, 7, 10, 10, 10, 9, 7, 13, 13, 0],
    ],
    [
        [9, 6, 7, 6, 7, 7, 7, 1, 5, 5, 5, 4, 3, 6, 10, 0],
        [11, 12, 11, 10, 10, 9, 9, 6, 10, 10, 10, 9, 8, 13, 13, 0],
        [12, 13, 12, 12, 13, 10, 10, 7, 11, 11, 11, 11, 9, 13, 13, 0],
    ],
    [
        [10, 8, 9, 8, 10, 7, 7, 5, 8, 8, 8, 7, 6, 11, 11, 0],
        [10, 9, 9, 8, 10, 7, 7, 5, 8, 8, 8, 7, 6, 13, 12, 0],
        [12, 12, 11, 10, 12, 9, 9, 6, 10, 8, 8, 8, 6, 13, 13, 0],
    ],
    [
        [10, 8, 9, 8, 10, 7, 7, 5, 8, 8, 8, 7, 6, 11, 11, 0],
        [10, 9, 9, 8, 10, 7, 7, 5, 8, 8, 8, 7, 6, 13, 12, 0],
        [12, 12, 11, 10, 12, 9, 9, 6, 10, 8, 8, 8, 6, 13, 13, 0],
    ],
    [
        [13, 11, 12, 11, 12, 10, 10, 7, 11, 11, 11, 10, 9, 13, 13, 0],
        [13, 12, 12, 11, 13, 10, 10, 7, 11, 11, 11, 10, 9, 13, 13, 0],
        [13, 12, 12, 11, 13, 10, 10, 7, 11, 11, 11, 11, 9, 13, 13, 0],
    ],
    [
        [9, 7, 8, 7, 7, 6, 6, 3, 7, 7, 7, 6, 5, 11, 11, 0],
        [10, 9, 9, 8, 9, 7, 7, 4, 8, 8, 8, 7, 6, 13, 12, 0],
        [11, 11, 10, 9, 11, 8, 8, 5, 9, 9, 9, 8, 7, 13, 13, 0],
    ],
    [
        [9, 7, 8, 7, 7, 6, 6, 3, 7, 7, 7, 6, 5, 11, 11, 0],
        [10, 9, 9, 8, 9, 7, 7, 4, 8, 8, 8, 7, 6, 13, 12, 0],
        [11, 11, 10, 9, 11, 8, 8, 5, 9, 9, 9, 8, 7, 13, 13, 0],
    ],
    [
        [9, 7, 8, 7, 7, 6, 6, 3, 7, 7, 7, 6, 5, 11, 11, 0],
        [10, 9, 9, 8, 9, 7, 7, 4, 8, 8, 8, 7, 6, 13, 12, 0],
        [11, 11, 10, 9, 11, 8, 8, 5, 9, 9, 9, 8, 7, 13, 13, 0],
    ],
    [
        [11, 9, 10, 9, 9, 8, 8, 5, 9, 9, 9, 8, 7, 11, 13, 0],
        [11, 10, 10, 9, 10, 8, 8, 5, 9, 9, 9, 8, 7, 13, 13, 0],
        [12, 11, 11, 10, 11, 9, 9, 6, 10, 10, 10, 9, 8, 13, 13, 0],
    ],
    [
        [11, 10, 10, 9, 10, 8, 8, 5, 9, 9, 9, 8, 7, 13, 13, 0],
        [11, 11, 10, 10, 11, 8, 8, 5, 9, 9, 9, 8, 7, 13, 13, 0],
        [12, 12, 11, 11, 12, 9, 9, 6, 10, 10, 10, 9, 8, 13, 13, 0],
    ],
    [
        [4, 3, 3, 3, 1, 1, 1, 1, 0, 0, 0, 0, 0, 7, 8, 0],
        [9, 8, 8, 7, 5, 6, 6, 3, 7, 7, 7, 6, 5, 7, 11, 0],
        [12, 11, 11, 11, 7, 9, 9, 6, 10, 10, 10, 10, 8, 7, 13, 0],
    ],
    [
        [5, 6, 4, 3, 3, 2, 2, 1, 3, 3, 3, 2, 1, 8, 7, 0],
        [13, 13, 12, 11, 11, 10, 10, 7, 3, 3, 3, 9, 9, 13, 13, 0],
        [13, 13, 12, 13, 13, 10, 10, 7, 3, 3, 3, 9, 9, 13, 13, 0],
    ],
    [
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ],
];
/// Superiority by defender class and attacker class: 1 the attacker is
/// superior, 2 the defender.
const SUPERIORITY: [[u8; 16]; 16] = [
    [0, 0, 0, 0, 2, 0, 2, 2, 2, 2, 2, 2, 2, 2, 0, 0],
    [0, 0, 0, 0, 0, 2, 2, 2, 2, 2, 2, 0, 2, 0, 0, 0],
    [0, 1, 0, 0, 0, 0, 0, 2, 2, 2, 2, 2, 2, 0, 0, 0],
    [1, 1, 0, 0, 0, 0, 0, 2, 0, 2, 0, 0, 2, 0, 0, 0],
    [1, 1, 0, 0, 0, 2, 2, 2, 0, 0, 0, 0, 2, 0, 0, 0],
    [1, 1, 1, 0, 1, 0, 0, 0, 2, 2, 2, 2, 2, 0, 0, 0],
    [1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0],
    [1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0],
    [0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 0, 0, 1, 1, 0],
    [0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 1, 0, 0, 1, 1, 0],
    [0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 0, 0, 1, 1, 0],
    [0, 0, 0, 0, 0, 0, 0, 2, 1, 0, 0, 0, 0, 1, 1, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 1, 1, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];
/// Cohesion hits the attackers take, by die roll and column.
const ATTACKER_HITS: [[u8; 13]; 10] = [
    [6, 5, 4, 4, 3, 3, 3, 3, 3, 3, 3, 2, 2],
    [6, 4, 4, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2],
    [5, 4, 4, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2],
    [5, 4, 4, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2],
    [5, 4, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 1],
    [4, 4, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [4, 4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1],
    [4, 4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1],
    [4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1],
    [3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1],
];
/// Cohesion hits the defender takes, by die roll and column.
const DEFENDER_HITS: [[u8; 13]; 10] = [
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3],
    [2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 4, 3],
    [2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4],
    [2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 4, 4, 4],
    [2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4],
    [2, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4],
    [2, 2, 2, 2, 3, 4, 4, 4, 4, 4, 4, 4, 6],
];

/// Whether `attacker` class is attacker superior to `defender` by weapon
/// system alone (the superiority table's AS).
pub fn attacker_superior(attacker: MoveClass, defender: MoveClass) -> bool {
    SUPERIORITY[defender as usize][attacker as usize] == 1
}

/// The base column (before the −1 and adjustments) for attacker class
/// against defender class at `angle`.
pub fn base_column(attacker: MoveClass, defender: MoveClass, angle: u8) -> i32 {
    COLUMN[defender as usize][angle as usize][attacker as usize] as i32
}

/// What happens to the leader who loses the casualty roll after a shock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderCasualty {
    /// Finished for this turn.
    Finished,
    Wounded,
    Killed,
}

/// The leader casualty roll after a shock. Each side's leader involved (the
/// one with the highest first rating) rolls `rand() % 10` plus his fourth
/// rating; the higher total is the one hurt (the original, at `0x4a9140`),
/// by the difference: 1–2 finished, 3–4 wounded (killed if already
/// wounded), 5–6 killed. A tie, or a difference of 7 or more, has no
/// effect (the original only tests 1–6). Returns whether the attacking
/// side's leader is hurt, and the effect.
pub fn leader_casualty(
    attacker: (u16, u32),
    defender: (u16, u32),
    loser_wounded: impl Fn(bool) -> bool,
) -> Option<(bool, LeaderCasualty)> {
    let a = attacker.1 as i32 + attacker.0 as i32;
    let d = defender.1 as i32 + defender.0 as i32;
    let attacker_lost = a > d;
    let effect = match (a - d).abs() {
        1 | 2 => LeaderCasualty::Finished,
        3 | 4 if !loser_wounded(attacker_lost) => LeaderCasualty::Wounded,
        3..=6 => LeaderCasualty::Killed,
        _ => return None,
    };
    Some((attacker_lost, effect))
}

/// A side's combat size: the sum of its units' sizes, leaving skirmishers
/// out when any other kind of unit fights with them.
pub fn side_size(units: &[(MoveClass, i32)]) -> i32 {
    let mixed = units.iter().any(|&(c, _)| c != MoveClass::Skirmisher);
    units.iter().filter(|&&(c, _)| !mixed || c != MoveClass::Skirmisher).map(|&(_, s)| s).sum()
}

/// Which defending unit's class the shock uses: the one giving the
/// attacker the lowest frontal column (the first on a tie).
pub fn defending_class(attacker: MoveClass, defenders: &[MoveClass]) -> Option<MoveClass> {
    let mut best: Option<(i8, MoveClass)> = None;
    for &d in defenders {
        let col = COLUMN[d as usize][0][attacker as usize];
        if best.is_none_or(|(b, _)| col < b) {
            best = Some((col, d));
        }
    }
    best.map(|(_, d)| d)
}

/// One unit's part in sharing out a side's hits.
#[derive(Debug, Clone, Copy)]
pub struct Share {
    /// Hits already taken and troop quality.
    pub hits: i32,
    pub tq: i32,
    /// Whether an enemy claims the unit's hex (the original asks each enemy
    /// unit, as for zones of control).
    pub enemy_contact: bool,
}

/// Share out `total` hits among a side's `units`: each takes `total ÷ n`,
/// the leading unit (index `lead`) also the remainder. A unit whose share is
/// at least three times the other side's share (`other_total ÷ other_n`, at
/// least 1) takes 2 more. A unit one hit from routing that is in enemy
/// contact makes a TQ check with `roll` (0..=9): it takes 1 fewer if the
/// check costs nothing, 1 more otherwise (`rolls` supplies one roll per such
/// unit, in order).
pub fn share_hits(
    total: i32,
    units: &[Share],
    lead: usize,
    other_total: i32,
    other_n: usize,
    rolls: &mut dyn FnMut() -> u32,
) -> Vec<i32> {
    let n = units.len().max(1) as i32;
    let (each, extra) = (total / n, total % n);
    let other = (other_total / other_n.max(1) as i32).max(1);
    units
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let mut h = each + if i == lead { extra } else { 0 };
            if other * 3 <= h {
                h += 2;
            }
            if u.tq - u.hits == 1 && u.enemy_contact {
                h += if tq_check(u.tq, rolls()) == 0 { -1 } else { 1 };
            }
            h
        })
        .collect()
}

/// Hits from a TQ check with die `roll` (0..=9): how far the roll exceeds
/// the unit's troop quality (in the original a flag lowers the threshold to
/// 1; not yet identified).
pub fn tq_check(tq: i32, roll: u32) -> i32 {
    (roll as i32 - tq).max(0)
}

/// Rout points a unit is worth when it is eliminated or routs terminally
/// (the original's per-class method): Alexander his initiative × 10, other
/// leaders × 5, phalanxes twice their TQ, skirmishers 1 (Agrianians
/// included, though the manual says 2), elephants and chariots 2, oxybeles
/// 0, everything else its TQ.
pub fn rout_points(type_id: usize, tq: u16, initiative: u16) -> u16 {
    match type_id {
        0 => initiative * 10,
        4..=7 | 35 => initiative * 5,
        // 🟡 Hannibal's leaders as Alexander's other leaders
        t if gboh_formats::units::hannibal_class(t) == Some(gboh_formats::units::Class::Leader) => initiative * 5,
        _ => match MoveClass::of_type(type_id) {
            Some(MoveClass::Phalanx) => tq * 2,
            Some(MoveClass::Skirmisher) => 1,
            Some(MoveClass::Elephant | MoveClass::Chariot) => 2,
            Some(MoveClass::Oxybeles) => 0,
            _ => tq,
        },
    }
}

/// Whether an attacker takes the TQ check before contact: in the original
/// every attacker that moved this turn is marked when its attack is
/// registered (`FUN_00570883`), whatever its class (the manual's list of
/// classes and exceptions is not applied; the original's commentary shows
/// skirmishers checking).
pub fn checks_before_shock(_class: MoveClass, moved: bool, _defender: MoveClass, _frontal: bool) -> bool {
    moved
}

/// Apply a TQ check to a unit with `hits` and troop quality `tq`: returns
/// its new hits and whether it routs (hits reaching TQ). A routing
/// defender is eliminated if attacked.
pub fn tq_check_unit(hits: i32, tq: i32, roll: u32) -> (i32, bool) {
    let h = hits + tq_check(tq, roll);
    (h, h >= tq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use MoveClass::*;

    fn shock(attacker: MoveClass, defender: MoveClass, angle: u8, sa: i32, sd: i32) -> Shock {
        Shock {
            attacker,
            defender,
            angle,
            attacker_size: sa,
            defender_size: sd,
            attacker_cataphract: false,
            defender_cataphract: false,
            defender_missile: false,
            no_superiority: false,
            column_shift: 0,
            hannibal: false,
            attacker_nation: 0,
            defender_nation: 0,
        }
    }

    /// In Hannibal's rules the table applies after the angle, so on a flank
    /// a defender the table favours is superior too.
    #[test]
    fn hannibal_superiority_stacks() {
        let pair = (0..16)
            .flat_map(|d| (0..16).map(move |a| (a, d)))
            .find(|&(a, d)| SUPERIORITY[d][a] == 2 && a == HeavyInfantry as usize && d == Phalanx as usize)
            .or_else(|| {
                (0..16).flat_map(|d| (0..16).map(move |a| (a, d))).find(|&(a, d)| SUPERIORITY[d][a] == 2 && [0, 2, 3, 6].contains(&a) && [0, 2, 3].contains(&d))
            });
        let (a, d) = pair.expect("a pair the table favours the defender in");
        let class = |n: usize| [Phalanx, Legion, HeavyInfantry, MediumInfantry, LightInfantryMG, LightInfantryMG, LightInfantry].into_iter().find(|c| *c as usize == n).unwrap();
        let mut s = shock(class(a), class(d), 1, 6, 6);
        assert_eq!(s.superiority(), (true, false));
        s.hannibal = true;
        assert_eq!(s.superiority(), (true, true));
    }

    #[test]
    fn skirmishers_do_not_add_size_to_others() {
        assert_eq!(side_size(&[(Skirmisher, 1), (HeavyInfantry, 6)]), 6);
        assert_eq!(side_size(&[(Skirmisher, 1), (Skirmisher, 1)]), 2);
    }

    #[test]
    fn rout_points_by_class() {
        assert_eq!(rout_points(0, 0, 7), 70); // Alexander
        assert_eq!(rout_points(4, 0, 3), 15);
        assert_eq!(rout_points(17, 7, 0), 14); // phalanx
        assert_eq!(rout_points(42, 6, 0), 1); // Agrianians
        assert_eq!(rout_points(24, 6, 0), 2);
        assert_eq!(rout_points(38, 5, 0), 0);
        assert_eq!(rout_points(31, 6, 0), 6);
    }

    #[test]
    fn leader_casualties() {
        let none = |_| false;
        // (fourth rating, roll): totals 5 vs 7, the defender's leader (the
        // higher total) is finished
        assert_eq!(leader_casualty((2, 3), (4, 3), none), Some((false, LeaderCasualty::Finished)));
        assert_eq!(leader_casualty((7, 2), (0, 5), none), Some((true, LeaderCasualty::Wounded)));
        assert_eq!(leader_casualty((7, 2), (0, 5), |_| true), Some((true, LeaderCasualty::Killed)));
        assert_eq!(leader_casualty((7, 9), (0, 0), none), None);
        assert_eq!(leader_casualty((3, 3), (3, 3), none), None);
    }

    #[test]
    fn tq_checks() {
        // India 8 (chariots, TQ 4, 2 hits) rolled at least 7 at Hydaspes
        assert_eq!(tq_check_unit(2, 4, 7), (5, true));
        assert_eq!(tq_check_unit(0, 9, 9), (0, false));
    }

    #[test]
    fn hits_are_shared() {
        let u = Share { hits: 0, tq: 6, enemy_contact: false };
        let mut roll = || 0;
        assert_eq!(share_hits(5, &[u, u], 0, 2, 2, &mut roll), vec![3 + 2, 2]);
        assert_eq!(share_hits(4, &[u, u], 1, 4, 1, &mut roll), vec![2, 2]);
    }

    #[test]
    fn size_ratio_shifts_the_column() {
        let even = shock(HeavyInfantry, HeavyInfantry, 0, 6, 6).column();
        assert_eq!(shock(HeavyInfantry, HeavyInfantry, 0, 12, 6).column(), even + 1);
        assert_eq!(shock(HeavyInfantry, HeavyInfantry, 0, 6, 12).column(), even - 1);
        assert_eq!(shock(HeavyInfantry, HeavyInfantry, 0, 4, 9).column(), even - 2);
    }

    #[test]
    fn flank_attacks_are_superior() {
        assert_eq!(shock(HeavyInfantry, HeavyInfantry, 1, 6, 6).superiority(), (true, false));
        // except against elephants by horse
        assert!(!shock(HeavyCavalry, Elephant, 1, 6, 6).superiority().0);
    }

    #[test]
    fn phalanx_front_is_hard_to_shock() {
        let front = shock(HeavyInfantry, Phalanx, 0, 6, 6);
        assert_eq!(front.column(), COLUMN[0][0][2] as usize - 1 - 3);
        let r = front.resolve(0);
        let c = front.column();
        assert_eq!((r.attacker_hits, r.defender_hits), (ATTACKER_HITS[0][c] as i32, DEFENDER_HITS[0][c] as i32));
    }
}
