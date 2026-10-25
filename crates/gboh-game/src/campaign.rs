//! The campaign: Alexander's conquest province by province over ten
//! rounds, from the original's campaign object (`AlexCampaignGame`,
//! `CAMPAIGN.CPP`).
//!
//! Eight provinces: 0 Macedon, 1 Anatolia, 2 Levant, 3 Egypt,
//! 4 Mesopotamia, 5 Persia, 6 Bactria, 7 Indus. Each round the army (its
//! flag) moves to a neighbouring province; an unconquered one is taken by
//! fighting its battles (Egypt and Persia have none: marching in takes
//! them). Garrisons (infantry and cavalry counted in units of 1, 5, 10 and
//! 50) keep conquered provinces subjugated; ungarrisoned ones slip free. A
//! battle lost ends the campaign, except at Samarkand (Bactria's first).
//! Nothing of a battle (casualties, leaders) carries over.
//!
//! Hannibal's campaign (`HannibalCampaignGame`) is the same game on its own
//! tables: ten provinces round the western Mediterranean (0 Carthage,
//! 1 Numidia, 2 Lusitania, 3 Hispania, 4 Gaul, 5 Cisalpine Gaul,
//! 6 Samnium, 7 Roma, 8 Magna Graecia, 9 Sicily), the army starting in
//! Gaul; battles queued when the map closes, chains of them scheduled
//! rounds ahead after Cannae and Metaurus; won with Zama or by holding
//! Italy (see [`Kind::Hannibal`]).

use gboh_formats::crt::BorlandRand;
use std::collections::VecDeque;

/// No province (before the first round).
pub const NONE: usize = 8;
/// Alexander's provinces (Hannibal's: [`HANNIBAL_PROVINCES`]).
pub const PROVINCES: usize = 8;
pub const HANNIBAL_PROVINCES: usize = 10;

/// Whose campaign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Alexander,
    Hannibal,
}

/// Hannibal's battle table (its scenario slots: the campaign fights 0
/// Metaurus, 1 Bagradas, 2 Cannae, 4 Zama, 5 Trebbia, 6 Baecula, 7 Ilipa,
/// 9 Great Plains and 11 Trasimene).
pub const HANNIBAL_BATTLES: [&str; 13] = [
    "METAURUS.SCN",
    "BAGRADAS.SCN",
    "CANNAE.SCN",
    "CYNO.SCN",
    "ZAMA.SCN",
    "TREBBIA.SCN",
    "BAECULA.SCN",
    "ILIPA.SCN",
    "BENEVENT.SCN",
    "GPLAINS.SCN",
    "MAGNESIA.SCN",
    "TRASIMEN.SCN",
    "DERTOSA.SCN",
];

/// Hannibal's provinces' neighbours, the routes and what a conquest
/// reveals alike (`FUN_005323c0`, `FUN_00532ef8`).
fn hannibal_neighbours(p: usize) -> &'static [usize] {
    match p {
        0 => &[9, 1],
        1 => &[0, 2],
        2 => &[1, 3],
        3 => &[2, 4],
        4 => &[3, 5],
        5 => &[4, 6, 7],
        6 => &[5, 7],
        7 => &[5, 6, 8],
        8 => &[7, 9],
        9 => &[8, 0],
        _ => &[],
    }
}

/// Hannibal's provinces' battles (Numidia, Cisalpine Gaul, Magna Graecia).
fn hannibal_battles(p: usize) -> &'static [usize] {
    match p {
        1 => &[1],
        5 => &[5, 11],
        8 => &[2],
        _ => &[],
    }
}

/// The province of one of Hannibal's battles (for the slip of a lost
/// battle's province, `FUN_00532e9b`).
fn hannibal_battle_province(b: usize) -> Option<usize> {
    match b {
        0 => Some(6),
        1 => Some(1),
        2 => Some(8),
        5 | 11 => Some(5),
        _ => None,
    }
}

/// Hannibal's Roma, never conquered nor subjugated.
const ROMA: usize = 7;
/// Bactria and Indus.
const BACTRIA: usize = 6;
const INDUS: usize = 7;
/// Rounds to conquer the world in.
pub const ROUNDS: i16 = 10;

/// How subjugated a conquered province is.
pub const FREE: u8 = 0;
pub const PARTIAL: u8 = 1;
pub const TOTAL: u8 = 2;

/// Whether a province is known, and taken.
pub const HIDDEN: u8 = 0;
pub const OPEN: u8 = 1;
pub const CONQUERED: u8 = 2;

/// Unit kinds of the garrisons.
pub const INFANTRY: usize = 0;
pub const CAVALRY: usize = 1;

/// The battles (scenario table indices, 0 Chaeronea … 9 Hydaspes) of a
/// province, fought in order when the army enters it unconquered
/// (`FUN_0052da77`).
pub fn battles(province: usize) -> &'static [usize] {
    match province {
        0 => &[1, 2],
        1 => &[3],
        2 => &[4],
        4 => &[5],
        6 => &[6, 7],
        7 => &[8, 9],
        _ => &[],
    }
}

/// The provinces a conquest reveals (`FUN_0052d559`).
fn reveals(province: usize) -> &'static [usize] {
    match province {
        0 => &[1],
        1 => &[4, 2],
        2 => &[4, 3],
        4 => &[5, 2],
        5 => &[6],
        6 => &[7],
        _ => &[],
    }
}

/// The provinces the army may march to `target` from (`FUN_0052dd41`).
fn routes_to(target: usize) -> &'static [usize] {
    match target {
        0 => &[1],
        1 => &[0, 2, 4],
        2 => &[3, 1, 4],
        3 => &[2],
        4 => &[1, 2, 5],
        5 => &[4, 6],
        6 => &[5, 7],
        7 => &[6],
        _ => &[],
    }
}

/// Something that happened on the map, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Round(i16),
    /// The last round's results.
    Final,
    /// A province's subjugation rose (true) or fell to this level.
    Subjugation { province: usize, level: u8, rose: bool },
    /// Reinforcements of a kind assembled in Macedon (0: none available).
    Reinforcements { kind: usize, count: u16 },
}

/// What comes after a battle or a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Fight this battle (scenario table index).
    Battle(usize),
    /// Show the campaign map for a new round.
    Map,
    /// The campaign is over: show its results.
    Results,
}

/// The campaign's results: rounds played, provinces conquered, the rating
/// (0 the worst to 5 the best) and whether Bactria fell (victory). Hannibal's
/// add the provinces wholly subjugated and name the rating by a string
/// (by how the campaign ended).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Results {
    pub rounds: i16,
    pub conquered: usize,
    pub rating: usize,
    pub victory: bool,
    pub subjugated: usize,
    pub rating_string: Option<u16>,
}

/// A campaign in progress (the original's fields in its order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Campaign {
    pub kind: Kind,
    pub round: i16,
    /// Where the army was at the start of the round, and is now.
    pub last: usize,
    pub current: usize,
    pub indus_won: bool,
    pub bactria_won: bool,
    /// The round Indus fell (11 while it stands).
    pub indus_round: i16,
    /// Garrison units per province and kind.
    pub units: Vec<[u16; 2]>,
    pub subjugation: Vec<u8>,
    pub status: Vec<u8>,
    /// Provinces whose coasts the fleet holds (Anatolia, Levant, Egypt
    /// until taken; Hannibal's Gaul, Magna Graecia, Sicily).
    pub navy: Vec<bool>,
    pub garrison: [u16; 2],
    pub last_won: bool,
    /// Battles to fight, in order (this round's).
    pub pending: VecDeque<usize>,
    /// Hannibal's battles scheduled for the next round and the one after.
    pub later: [VecDeque<usize>; 2],
    /// Hannibal's: the map closed with Subjugate.
    pub subjugate: bool,
    /// The last battle fought.
    pub last_battle: Option<usize>,
    pub log: Vec<Event>,
    pub rng: u32,
}

impl Campaign {
    /// A new campaign (`FUN_0052cc64`): Macedon known and subjugated, the
    /// fleet off Anatolia, the Levant and Egypt.
    pub fn new(seed: u32) -> Self {
        let mut subjugation = vec![FREE; PROVINCES];
        subjugation[0] = TOTAL;
        let mut status = vec![HIDDEN; PROVINCES];
        status[0] = OPEN;
        let mut navy = vec![false; PROVINCES];
        navy[1..=3].fill(true);
        Self {
            kind: Kind::Alexander,
            round: 0,
            last: NONE,
            current: NONE,
            indus_won: false,
            bactria_won: false,
            indus_round: ROUNDS + 1,
            units: vec![[0; 2]; PROVINCES],
            subjugation,
            status,
            navy,
            garrison: [0; 2],
            last_won: true,
            pending: VecDeque::new(),
            later: Default::default(),
            subjugate: false,
            last_battle: None,
            log: Vec::new(),
            rng: seed,
        }
    }

    /// A new campaign of Hannibal's (`FUN_00531acc`): the army in Gaul;
    /// Carthage, Numidia, Lusitania and Hispania held, wholly; the fleet off
    /// Gaul, Magna Graecia and Sicily.
    pub fn new_hannibal(seed: u32) -> Self {
        let n = HANNIBAL_PROVINCES;
        let mut c = Self::new(seed);
        c.kind = Kind::Hannibal;
        c.current = 4;
        c.units = vec![[0; 2]; n];
        c.subjugation = vec![FREE; n];
        c.status = vec![HIDDEN; n];
        c.navy = vec![false; n];
        for p in 0..4 {
            c.subjugation[p] = TOTAL;
            c.status[p] = CONQUERED;
        }
        c.status[4] = OPEN;
        for p in [4, 8, 9] {
            c.navy[p] = true;
        }
        c
    }

    /// How many provinces.
    pub fn provinces(&self) -> usize {
        self.status.len()
    }

    /// The nation (scenario byte) the player leads.
    pub fn player_nation(&self) -> u8 {
        match self.kind {
            Kind::Alexander => 2,
            Kind::Hannibal => 7,
        }
    }

    /// Whether the map should offer Battle now (the army stands where
    /// battles are to be fought); else Subjugate.
    pub fn battle_offered(&self) -> bool {
        match self.kind {
            Kind::Alexander => !self.pending.is_empty(),
            // the province the flag was moved to, unconquered, with battles
            Kind::Hannibal => self.current != self.last && self.status[self.current] != CONQUERED && !hannibal_battles(self.current).is_empty(),
        }
    }

    /// Hannibal's victory (`FUN_0053328f`): Zama won, or Cisalpine Gaul,
    /// Samnium, Magna Graecia and Sicily wholly subjugated.
    fn hannibal_victory(&self) -> bool {
        (self.last_won && self.last_battle == Some(4)) || [5, 6, 8, 9].iter().all(|&p| self.subjugation[p] == TOTAL)
    }

    fn rand(&mut self) -> u32 {
        let mut r = BorlandRand::new(self.rng);
        let v = r.next();
        self.rng = r.state();
        v
    }

    /// Enter `province` with the army: it becomes the current one and, if
    /// not yet conquered, its battles are queued.
    fn enter(&mut self, province: usize) {
        self.current = province;
        self.pending.clear();
        if self.status[province] == OPEN {
            self.pending.extend(battles(province));
        }
    }

    /// Whether the army may march to `target` this round.
    pub fn can_march(&self, target: usize) -> bool {
        if self.kind == Kind::Hannibal {
            // (staying put isn't a march)
            return target < self.provinces() && self.status[target] != HIDDEN && target != self.last && hannibal_neighbours(self.last).contains(&target);
        }
        target < PROVINCES && self.status[target] != HIDDEN && (target == self.last || routes_to(target).contains(&self.last))
    }

    /// March the army to `target` (dropping its flag there); returns whether
    /// it may.
    pub fn march(&mut self, target: usize) -> bool {
        if !self.can_march(target) {
            return false;
        }
        // (Hannibal's battles are queued only when the map closes)
        if self.kind == Kind::Hannibal {
            self.current = target;
        } else {
            self.enter(target);
        }
        true
    }

    /// Move `count` garrison units of `kind` from one conquered province to
    /// another.
    pub fn move_units(&mut self, from: usize, to: usize, kind: usize, count: u16) -> bool {
        if from == to || to >= self.provinces() || self.status[to] != CONQUERED || self.units[from][kind] < count {
            return false;
        }
        self.units[from][kind] -= count;
        self.units[to][kind] += count;
        true
    }

    /// What follows a battle (or the start, `won` true): the next battle of
    /// the province while it is being won (Samarkand is followed by Jaxartes
    /// whatever its result), else a new round (`FUN_00487782`).
    pub fn after_battle(&mut self, won: bool) -> Next {
        // (Hannibal's: a loss never goes on to the next battle)
        let go_on = won || (self.kind == Kind::Alexander && self.last_battle == Some(6));
        if go_on && let Some(b) = self.pending.pop_front() {
            self.last_battle = Some(b);
            return Next::Battle(b);
        }
        self.begin_round(won)
    }

    /// A new round (`FUN_0052d7cd`, up to the map): a lost battle ends the
    /// campaign unless it was fought in Bactria; the province the army
    /// stands in falls and reveals its neighbours. Then the map shows
    /// (`Next::Map`), and [`Campaign::opening`] runs.
    pub fn begin_round(&mut self, won: bool) -> Next {
        if self.kind == Kind::Hannibal {
            return self.hannibal_turn(won);
        }
        self.last_won = won;
        self.pending.clear();
        if !won && self.current != BACTRIA {
            return Next::Results;
        }
        if won && self.current == BACTRIA {
            self.bactria_won = true;
        }
        if won && self.current == INDUS {
            self.indus_won = true;
        }
        if self.current == NONE {
            self.enter(0);
        } else {
            let p = self.current;
            self.status[p] = CONQUERED;
            for &n in reveals(p) {
                if self.status[n] == HIDDEN {
                    self.status[n] = OPEN;
                }
            }
            self.navy[p] = false;
            if p == INDUS {
                self.indus_round = self.round;
            }
        }
        self.round += 1;
        Next::Map
    }

    /// Hannibal's turn (`FUN_005328b8`): the scheduled battles move up a
    /// round; Hannibal's victory ends it; a lost battle too, but Metaurus
    /// (Great Plains and Zama are then fought the round after next); Cannae
    /// won schedules Baecula and Metaurus for the round after next and Ilipa
    /// after that; the army's province reveals its neighbours and, from the
    /// second round, falls (Roma never).
    fn hannibal_turn(&mut self, won: bool) -> Next {
        self.last_won = won;
        self.pending = std::mem::take(&mut self.later[0]);
        self.later[0] = std::mem::take(&mut self.later[1]);
        if self.hannibal_victory() {
            return Next::Results;
        }
        if !won {
            if self.last_battle != Some(0) {
                return Next::Results;
            }
            self.later[0].extend([9, 4]);
        } else if self.last_battle == Some(2) {
            self.later[0].extend([6, 0]);
            self.later[1].push_back(7);
        }
        let p = self.current;
        for &n in hannibal_neighbours(p) {
            if self.status[n] == HIDDEN {
                self.status[n] = OPEN;
            }
        }
        if self.round != 0 {
            if p != ROMA {
                self.status[p] = CONQUERED;
            }
            self.navy[p] = false;
        }
        self.round += 1;
        self.last = p;
        Next::Map
    }

    /// Hannibal's opening (`FUN_00595ab3`): none of the changes in the
    /// first round; the army's province held tighter (not Carthage nor
    /// Roma); every other province held rolls as Alexander's do (coasts
    /// Gaul, Magna Graecia, Sicily), the province of a battle just lost
    /// slipping for sure; reinforcements in Carthage, fewer the more of
    /// Numidia, Lusitania and Hispania need watching, twice as many with
    /// Magna Graecia wholly held.
    fn hannibal_opening(&mut self) -> Vec<Event> {
        let mut events = vec![if self.round > ROUNDS { Event::Final } else { Event::Round(self.round) }];
        let p = self.current;
        let mut change = |c: &mut Self, province: usize, level: u8| {
            let old = c.subjugation[province];
            if old != level {
                c.subjugation[province] = level;
                events.push(Event::Subjugation { province, level, rose: level > old });
            }
        };
        if self.round != 1 {
            if p != 0 && p != ROMA {
                let level = self.subjugation[p];
                if level == FREE {
                    if self.last_won {
                        change(self, p, PARTIAL);
                    }
                } else {
                    self.subjugate = false;
                    change(self, p, (level + 1).min(TOTAL));
                }
            }
            let lost = if self.last_won { None } else { self.last_battle.and_then(hannibal_battle_province) };
            for i in 1..self.provinces() {
                if i == p || self.status[i] != CONQUERED {
                    continue;
                }
                let r = (self.rand() % 100) as f64;
                let level = self.subjugation[i];
                let garrison = (self.units[i][INFANTRY] + self.units[i][CAVALRY]) as f64;
                let mut d = [0.6f32, 0.3, 0.05][level as usize];
                if [4, 8, 9].contains(&i) {
                    d += 0.3 * [4, 8, 9].iter().filter(|&&n| self.navy[n]).count() as f32;
                }
                let mut threshold = (95.0 - garrison / d as f64).trunc();
                if lost == Some(i) {
                    threshold = 101.0;
                }
                let threshold = threshold.max(0.0);
                let new = if r < threshold { level.saturating_sub(1) } else { (level + 1).min(TOTAL) };
                change(self, i, new);
            }
        }
        let n = (0..4).filter(|&i| self.subjugation[i] != TOTAL && i != p).count() as u16;
        let s: u16 = if self.subjugation[8] == TOTAL { 2 } else { 1 };
        self.garrison[INFANTRY] += 4 * n;
        self.garrison[CAVALRY] += n;
        let infantry = (12 * s).saturating_sub(4 * n);
        let cavalry = (3 * s).saturating_sub(n);
        self.units[0][INFANTRY] += infantry;
        self.units[0][CAVALRY] += cavalry;
        events.push(Event::Reinforcements { kind: INFANTRY, count: infantry });
        events.push(Event::Reinforcements { kind: CAVALRY, count: cavalry });
        self.log.extend(events.iter().cloned());
        events
    }

    /// The map's opening (`FUN_00588819`): the army's province is held
    /// tighter; every other conquered province rolls to slip or tighten
    /// (`rand() % 100` against `95 − garrison / d`, `d` 0.6, 0.3 or 0.05
    /// by level plus 0.3 for each coast the fleet holds, for coastal
    /// provinces); reinforcements gather in Macedon, fewer the more
    /// provinces need watching (the rest garrison them).
    pub fn opening(&mut self) -> Vec<Event> {
        if self.kind == Kind::Hannibal {
            return self.hannibal_opening();
        }
        let mut events = vec![if self.round > ROUNDS { Event::Final } else { Event::Round(self.round) }];
        let p = self.current;
        let mut change = |c: &mut Self, province: usize, level: u8| {
            let old = c.subjugation[province];
            if old != level {
                c.subjugation[province] = level;
                events.push(Event::Subjugation { province, level, rose: level > old });
            }
        };
        if p != 0 && p != NONE {
            let level = self.subjugation[p];
            let new = if level == FREE { if self.last_won { PARTIAL } else { FREE } } else { (level + 1).min(TOTAL) };
            change(self, p, new);
        }
        for i in 1..PROVINCES {
            if i == p || self.status[i] != CONQUERED {
                continue;
            }
            let r = (self.rand() % 100) as f64;
            let level = self.subjugation[i];
            let garrison = (self.units[i][INFANTRY] + self.units[i][CAVALRY]) as f64;
            let mut d = [0.6, 0.3, 0.05][level as usize];
            if (1..=3).contains(&i) {
                d += 0.3 * self.navy[1..=3].iter().filter(|&&n| n).count() as f64;
            }
            let threshold = (95.0 - garrison / d).trunc().max(0.0);
            let new = if r < threshold { level.saturating_sub(1) } else { (level + 1).min(TOTAL) };
            change(self, i, new);
        }
        if self.round <= ROUNDS {
            let n = (1..PROVINCES).filter(|&i| i != p && self.status[i] == CONQUERED && self.subjugation[i] < TOTAL).count() as u16;
            self.garrison[INFANTRY] += 4 * n;
            self.garrison[CAVALRY] += 2 * n;
            let infantry = 12u16.saturating_sub(4 * n);
            let cavalry = 3u16.saturating_sub(2 * n);
            self.units[0][INFANTRY] += infantry;
            self.units[0][CAVALRY] += cavalry;
            events.push(Event::Reinforcements { kind: INFANTRY, count: infantry });
            events.push(Event::Reinforcements { kind: CAVALRY, count: cavalry });
        }
        self.log.extend(events.iter().cloned());
        events
    }

    /// The map closed with OK (Battle or Subjugate): after the last round,
    /// the results; else the round's battles, or (none queued) another
    /// round at once, which takes the province the army stands in.
    pub fn end_round(&mut self) -> Next {
        if self.round > ROUNDS {
            return Next::Results;
        }
        self.last = self.current;
        self.garrison = [0; 2];
        // Hannibal's: the province's battles, unless Subjugate
        if self.kind == Kind::Hannibal && !std::mem::take(&mut self.subjugate) {
            self.pending.extend(hannibal_battles(self.current));
        }
        match self.pending.pop_front() {
            Some(b) => {
                self.last_battle = Some(b);
                Next::Battle(b)
            }
            None => {
                self.last_battle = None;
                self.begin_round(self.last_won)
            }
        }
    }

    /// The results (`FUN_0052d629`): the score is the round Indus fell
    /// (11 if not) plus the provinces not wholly subjugated, less one for
    /// Bactria won; 6–9 rates best (5), 10 to 13 down to 1, else 0.
    pub fn results(&self) -> Results {
        let subjugated = self.subjugation.iter().filter(|&&s| s == TOTAL).count();
        let conquered = self.status.iter().filter(|&&s| s == CONQUERED).count();
        if self.kind == Kind::Hannibal {
            // (`FUN_005325aa`: the rating named by how it ended)
            let victory = self.hannibal_victory();
            let id = match (victory, self.last_battle) {
                (false, Some(5 | 11)) => 138,
                (false, Some(2)) => 140,
                (false, Some(6 | 7)) => 142,
                (false, _) => 158,
                (true, Some(4)) => 162,
                (true, _) => 164,
            };
            return Results { rounds: self.round, conquered, rating: 0, victory, subjugated, rating_string: Some(id) };
        }
        let unsubjugated = self.subjugation.iter().filter(|&&s| s != TOTAL).count() as i16;
        let score = self.indus_round + unsubjugated - self.bactria_won as i16;
        let rating = match score {
            6..=9 => 5,
            10 => 4,
            11 => 3,
            12 => 2,
            13 => 1,
            _ => 0,
        };
        Results { rounds: self.round - 1, conquered, rating, victory: self.status[BACTRIA] == CONQUERED, subjugated, rating_string: None }
    }

    /// Garrison counts split into icons of 50, 10, 5 and 1 (the map's
    /// display, greedy).
    pub fn icons(count: u16) -> [u16; 4] {
        let mut left = count;
        let mut out = [0; 4];
        for (k, v) in [50, 10, 5, 1].into_iter().enumerate() {
            out[k] = left / v;
            left %= v;
        }
        out
    }

    /// The campaign as text, for saving.
    pub fn to_text(&self) -> String {
        let list = |v: &[u16]| v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" ");
        let units: Vec<u16> = self.units.iter().flatten().copied().collect();
        let later = |q: &VecDeque<usize>| list(&q.iter().map(|&b| b as u16).collect::<Vec<_>>());
        let head = if self.kind == Kind::Hannibal {
            format!("game hannibal\nlater {}\nlater2 {}\nsubjugate {}\n", later(&self.later[0]), later(&self.later[1]), self.subjugate as u8)
        } else {
            String::new()
        };
        head + &format!(
            "round {}\nlast {}\ncurrent {}\nwon {} {}\nindus_round {}\nunits {}\nsubjugation {}\nstatus {}\nnavy {}\nlast_won {}\npending {}\nlast_battle {}\nrng {}\n",
            self.round,
            self.last,
            self.current,
            self.indus_won as u8,
            self.bactria_won as u8,
            self.indus_round,
            list(&units),
            list(&self.subjugation.iter().map(|&v| u16::from(v)).collect::<Vec<_>>()),
            list(&self.status.iter().map(|&v| u16::from(v)).collect::<Vec<_>>()),
            list(&self.navy.iter().map(|&v| u16::from(v)).collect::<Vec<_>>()),
            self.last_won as u8,
            list(&self.pending.iter().map(|&b| b as u16).collect::<Vec<_>>()),
            self.last_battle.map_or(-1, |b| b as i32),
            self.rng
        )
    }

    /// Read a campaign written by [`Campaign::to_text`].
    pub fn parse(text: &str) -> Option<Self> {
        let mut c = if text.lines().any(|l| l.trim() == "game hannibal") { Self::new_hannibal(0) } else { Self::new(0) };
        let n = c.provinces();
        for line in text.lines() {
            let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
            let nums: Vec<i64> = rest.split_whitespace().filter_map(|n| n.parse().ok()).collect();
            let one = || nums.first().copied();
            match key {
                "round" => c.round = one()? as i16,
                "last" => c.last = one()? as usize,
                "current" => c.current = one()? as usize,
                "won" => (c.indus_won, c.bactria_won) = (*nums.first()? != 0, *nums.get(1)? != 0),
                "indus_round" => c.indus_round = one()? as i16,
                "units" => {
                    for (k, v) in nums.iter().enumerate().take(2 * n) {
                        c.units[k / 2][k % 2] = *v as u16;
                    }
                }
                "subjugation" => nums.iter().take(n).enumerate().for_each(|(k, &v)| c.subjugation[k] = v as u8),
                "status" => nums.iter().take(n).enumerate().for_each(|(k, &v)| c.status[k] = v as u8),
                "navy" => nums.iter().take(n).enumerate().for_each(|(k, &v)| c.navy[k] = v != 0),
                "later" => c.later[0] = nums.iter().map(|&v| v as usize).collect(),
                "later2" => c.later[1] = nums.iter().map(|&v| v as usize).collect(),
                "subjugate" => c.subjugate = one()? != 0,
                "last_won" => c.last_won = one()? != 0,
                "pending" => c.pending = nums.iter().map(|&n| n as usize).collect(),
                "last_battle" => c.last_battle = one().filter(|&b| b >= 0).map(|b| b as usize),
                "rng" => c.rng = one()? as u32,
                _ => {}
            }
        }
        Some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hannibal's: the army in Gaul; into Cisalpine Gaul, Trebbia then
    /// Trasimene; Cannae won brings Baecula and Metaurus the round after
    /// next, then Ilipa; a loss ends it but at Metaurus.
    #[test]
    fn hannibal_campaign() {
        let mut c = Campaign::new_hannibal(1);
        assert_eq!(c.after_battle(true), Next::Map);
        assert_eq!((c.round, c.current, c.status[5], c.status[3]), (1, 4, OPEN, CONQUERED));
        c.opening();
        assert_eq!(c.units[0], [12, 3]);
        assert!(!c.battle_offered());
        assert!(!c.can_march(4) && c.can_march(5));
        assert!(c.march(5));
        assert!(c.battle_offered());
        assert_eq!(c.end_round(), Next::Battle(5));
        assert_eq!(c.after_battle(true), Next::Battle(11));
        assert_eq!(c.after_battle(true), Next::Map);
        assert_eq!((c.round, c.status[5], c.status[7]), (2, CONQUERED, OPEN));
        // on through Roma (never conquered) to Magna Graecia: Cannae
        c.opening();
        assert!(c.march(7));
        assert!(!c.battle_offered());
        c.subjugate = true;
        assert_eq!(c.end_round(), Next::Map);
        assert_eq!(c.status[7], OPEN, "Roma stands");
        c.opening();
        assert!(c.march(8));
        assert_eq!(c.end_round(), Next::Battle(2));
        // Cannae won (round 3): nothing next round, Baecula and Metaurus
        // the round after, Ilipa after that
        assert_eq!(c.after_battle(true), Next::Map);
        c.opening();
        c.subjugate = true;
        assert_eq!(c.end_round(), Next::Map);
        c.opening();
        c.subjugate = true;
        assert_eq!(c.end_round(), Next::Battle(6));
        assert_eq!(c.after_battle(true), Next::Battle(0));
        // Metaurus lost: on, Ilipa next round, then Great Plains and Zama
        assert_eq!(c.after_battle(false), Next::Map);
        c.opening();
        c.subjugate = true;
        assert_eq!(c.end_round(), Next::Battle(7));
        // Ilipa lost ends it
        assert_eq!(c.after_battle(false), Next::Results);
        assert_eq!(c.results().rating_string, Some(142));
        let back = Campaign::parse(&c.to_text()).unwrap();
        assert_eq!(Campaign { log: Vec::new(), ..c }, back);
    }

    #[test]
    fn opens_in_macedon() {
        let mut c = Campaign::new(1);
        assert_eq!(c.after_battle(true), Next::Map);
        assert_eq!((c.round, c.current), (1, 0));
        assert_eq!(c.pending, [1, 2]);
        c.opening();
        assert_eq!(c.units[0], [12, 3], "full reinforcements with nothing to watch");
        // Battle: Lyginus, then Pelium
        assert_eq!(c.end_round(), Next::Battle(1));
        assert_eq!(c.after_battle(true), Next::Battle(2));
        // both won: Macedon conquered, Anatolia revealed
        assert_eq!(c.after_battle(true), Next::Map);
        assert_eq!((c.status[0], c.status[1], c.round), (CONQUERED, OPEN, 2));
        assert!(!c.can_march(2) && c.can_march(1));
        assert!(c.march(1));
        assert_eq!(c.pending, [3]);
    }

    #[test]
    fn a_lost_battle_ends_it_but_samarkand() {
        let mut c = Campaign::new(1);
        c.after_battle(true);
        c.end_round();
        assert_eq!(c.after_battle(false), Next::Results);
        // Samarkand lost: Jaxartes is still fought
        let mut c = Campaign::new(1);
        c.status[BACTRIA] = OPEN;
        c.last = 5;
        c.round = 5;
        assert!(c.march(BACTRIA));
        assert_eq!(c.end_round(), Next::Battle(6));
        assert_eq!(c.after_battle(false), Next::Battle(7));
        // and losing Jaxartes still takes Bactria
        assert_eq!(c.after_battle(false), Next::Map);
        assert_eq!(c.status[BACTRIA], CONQUERED);
        assert!(c.results().victory);
    }

    #[test]
    fn egypt_falls_by_marching_in() {
        let mut c = Campaign::new(1);
        c.status[3] = OPEN;
        c.last = 2;
        c.round = 4;
        c.last_won = true;
        assert!(c.march(3));
        assert!(c.pending.is_empty());
        assert_eq!(c.end_round(), Next::Map);
        assert_eq!(c.status[3], CONQUERED);
    }

    #[test]
    fn ungarrisoned_provinces_slip() {
        // with no garrison a partly subjugated province almost surely
        // slips free (95 in 100)
        let mut slipped = 0;
        for seed in 0..100 {
            let mut c = Campaign::new(seed);
            c.status[1] = CONQUERED;
            c.subjugation[1] = PARTIAL;
            c.current = 0;
            c.round = 3;
            c.opening();
            slipped += (c.subjugation[1] == FREE) as i32;
        }
        assert!(slipped > 80, "{slipped}");
    }

    #[test]
    fn rates_and_saves() {
        let mut c = Campaign::new(7);
        c.indus_round = 8;
        c.subjugation = vec![TOTAL; PROVINCES];
        c.status = vec![CONQUERED; PROVINCES];
        c.round = 9;
        let r = c.results();
        assert_eq!((r.rating, r.conquered, r.rounds, r.victory), (5, 8, 8, true));
        c.units[3] = [7, 2];
        c.pending = [8, 9].into();
        c.last_battle = Some(8);
        let back = Campaign::parse(&c.to_text()).unwrap();
        assert_eq!(Campaign { log: Vec::new(), ..c }, back);
        assert_eq!(Campaign::icons(67), [1, 1, 1, 2]);
    }
}
