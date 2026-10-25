//! Which leader is active: elite initiative, the initiative-weighted draw
//! and momentum (see `docs/formats.md`, "Leaders and command").

use crate::battle::Battle;
use gboh_formats::crt::BorlandRand;

/// Per battle (index into the original's battle table): the leader who
/// takes the elite initiative on the first turn (matched by name), or
/// `None` where the rule is off.
const ELITE_FIRST: [Option<&str>; 12] = [
    None, // Chaeronea
    Some("Alexander"),
    Some("Alexander"),
    Some("Alexander"),
    Some("Parmenion"), // Issus
    Some("Alexander"),
    None, // Samarkand
    Some("Alexander"),
    Some("Alexander"),
    Some("Alexander"),
    Some("Alexander"),
    None,
];

/// Per battle: percent chance on later turns that Alexander takes the elite
/// initiative.
const ELITE_CHANCE: [u32; 12] = [40, 40, 100, 40, 40, 40, 40, 40, 40, 40, 40, 0];

/// Elite initiative is used at most this many times per battle.
const ELITE_USES: u16 = 3;

/// The activation state of one turn.
#[derive(Debug, Clone)]
pub struct Activation {
    /// Battle index into the original's table, for the elite initiative
    /// rule; `None` turns the rule off.
    battle: Option<usize>,
    /// Turn number, 0 for the first.
    pub turn: u16,
    /// Leaders (unit indices) in the order they registered.
    leaders: Vec<usize>,
    /// Which of `leaders` have been activated this turn.
    done: Vec<bool>,
    /// The active leader, as an index into `leaders`.
    current: Option<usize>,
    /// Successful momentum attempts by the current leader.
    momentum: u8,
    elite_uses: u16,
    /// Hannibal's rules (elite initiative for any leader so marked, the
    /// superior chain); and at Trasimene its Romans sit out the first turn.
    hannibal: bool,
    pub romans_idle_first_turn: bool,
    /// Caesar's Sabis: the Caesarians sit out the first turn (no elite
    /// initiative either).
    pub caesarians_idle_first_turn: bool,
    /// The sides (nation bytes) whose elite leaders are asked whether to
    /// take the initiative (the players'), and whether the active leader's
    /// is such an offer awaiting the answer.
    pub asks: Vec<u8>,
    pub offered: bool,
    /// Hannibal's "Free Units" entry in the draw (initiative 3), while the
    /// battle has leaderless units: whether it has gone this turn, and
    /// whether it was just drawn (the game then lets them act).
    free: bool,
    free_done: bool,
    pub free_drawn: bool,
}

/// Hannibal's: the computer's leaders take an elite initiative on later
/// turns with this percent chance (the original's table: 40 for every
/// battle).
const HANNIBAL_ELITE_CHANCE: u32 = 40;
const ROMAN: u8 = 6;

impl Activation {
    pub fn new(battle: &Battle, battle_index: Option<usize>) -> Self {
        let leaders: Vec<usize> = battle.leaders().collect();
        Self {
            battle: battle_index,
            turn: 0,
            done: vec![false; leaders.len()],
            leaders,
            current: None,
            momentum: 0,
            elite_uses: 0,
            hannibal: battle.hannibal(),
            romans_idle_first_turn: false,
            caesarians_idle_first_turn: false,
            asks: Vec::new(),
            offered: false,
            // (in every battle of Hannibal's engine, leaderless units or not)
            free: battle.hannibal(),
            free_done: false,
            free_drawn: false,
        }
    }

    /// Mark a leader as having gone this turn (a leader finished by a
    /// casualty roll).
    pub fn finish(&mut self, unit: usize) {
        if let Some(i) = self.leaders.iter().position(|&u| u == unit) {
            self.done[i] = true;
        }
    }

    /// Resume a turn in which `done` (unit indices) have already been
    /// active: draw the next of the others.
    pub fn resume(&mut self, battle: &Battle, done: &[usize], rng: &mut BorlandRand) -> Option<usize> {
        for (i, &l) in self.leaders.iter().enumerate() {
            self.done[i] = done.contains(&l);
        }
        self.current = None;
        self.momentum = 0;
        self.draw(battle, rng)
    }

    /// The active leader (a unit index).
    pub fn active(&self) -> Option<usize> {
        self.current.map(|i| self.leaders[i])
    }

    /// Start a turn and return its first leader: the elite initiative if it
    /// applies, else the initiative-weighted draw.
    pub fn begin_turn(&mut self, battle: &Battle, rng: &mut BorlandRand) -> Option<usize> {
        self.done.iter_mut().for_each(|d| *d = false);
        self.current = None;
        self.momentum = 0;
        self.offered = false;
        self.free_done = false;
        if self.hannibal {
            self.idle(battle);
        }
        let elite = if self.hannibal {
            self.hannibal_elite(battle, rng).map(|(i, ask)| {
                self.offered = ask;
                i
            })
        } else {
            self.elite(battle, rng)
        };
        if let Some(i) = elite {
            self.elite_uses = if self.turn == 0 && !self.hannibal { 1 } else { self.elite_uses + 1 };
            self.done[i] = true;
            self.current = Some(i);
            return self.active();
        }
        self.draw(battle, rng)
    }

    fn elite(&self, battle: &Battle, rng: &mut BorlandRand) -> Option<usize> {
        let b = self.battle?;
        let first = (*ELITE_FIRST.get(b)?)?;
        if self.elite_uses >= ELITE_USES {
            return None;
        }
        let name = if self.turn == 0 {
            first
        } else if rng.next() % 100 < ELITE_CHANCE[b] {
            "Alexander"
        } else {
            return None;
        };
        self.leaders.iter().position(|&u| battle.units[u].name.contains(name))
    }

    /// Hannibal's leaders who can't go this turn are counted as gone: those
    /// not on the field (not arrived, or out), and at Trasimene the Romans
    /// on the first turn.
    fn idle(&mut self, battle: &Battle) {
        for (i, &l) in self.leaders.iter().enumerate() {
            let u = &battle.units[l];
            let first = self.turn == 0;
            if !u.on_field() || (self.romans_idle_first_turn && first && u.nation == ROMAN) || (self.caesarians_idle_first_turn && first && u.nation == 9) {
                self.done[i] = true;
            }
        }
    }

    /// Hannibal's elite initiative (`FUN_00554496`): one of the leaders so
    /// marked, at random, three times a battle at most; on the first turn
    /// always, his chain of superiors followed to the top; later with
    /// [`HANNIBAL_ELITE_CHANCE`] for the computer's leaders, while a
    /// player's is offered it (`HanEliteInitDlg`; the flag says so).
    fn hannibal_elite(&self, battle: &Battle, rng: &mut BorlandRand) -> Option<(usize, bool)> {
        if self.elite_uses >= ELITE_USES {
            return None;
        }
        let candidates: Vec<usize> = (0..self.leaders.len()).filter(|&i| !self.done[i] && battle.units[self.leaders[i]].elite).collect();
        if candidates.is_empty() {
            return None;
        }
        let pick = candidates[rng.random() as usize % candidates.len()];
        if self.turn == 0 {
            return Some((self.top_of_chain(battle, pick), false));
        }
        if self.asks.contains(&battle.units[self.leaders[pick]].side) {
            return Some((pick, true));
        }
        (rng.random() % 100 < HANNIBAL_ELITE_CHANCE).then_some((pick, false))
    }

    /// The player declines the elite initiative offered: it isn't used, and
    /// the turn's first leader is drawn as usual.
    pub fn decline(&mut self, battle: &Battle, rng: &mut BorlandRand) -> Option<usize> {
        let i = self.current.filter(|_| self.offered)?;
        self.offered = false;
        self.done[i] = false;
        self.current = None;
        self.elite_uses = self.elite_uses.saturating_sub(1);
        self.draw(battle, rng)
    }

    /// Up leader `i`'s chain of superiors to the last who hasn't gone this
    /// turn.
    fn top_of_chain(&self, battle: &Battle, mut i: usize) -> usize {
        for _ in 0..self.leaders.len() {
            let Some(s) = battle.units[self.leaders[i]].superior.and_then(|u| self.leaders.iter().position(|&l| l == u)) else { break };
            if self.done[s] {
                break;
            }
            i = s;
        }
        i
    }

    /// Whether the active leader is going again on momentum.
    pub fn in_momentum(&self) -> bool {
        self.momentum > 0
    }

    /// Whether the active leader may still try for momentum (twice at most).
    pub fn momentum_left(&self) -> bool {
        self.current.is_some() && self.momentum < 2
    }

    /// End the active leader's activation and pick the next. If
    /// `wants_momentum`, the leader first tries to go again: he succeeds
    /// with `(initiative + 1) × 8` percent on the first try and `× 5` on the
    /// second, at most twice. Returns `None` when every leader has gone.
    pub fn next(&mut self, battle: &Battle, wants_momentum: bool, rng: &mut BorlandRand) -> Option<usize> {
        if let (Some(i), true) = (self.current, wants_momentum)
            && self.momentum < 2
        {
            let per = if self.momentum == 0 { 8 } else { 5 };
            let chance = (initiative(battle, self.leaders[i]) + 1) * per;
            // (Hannibal's engine rolls on `random()`)
            let roll = if self.hannibal { rng.random() } else { rng.next() };
            if roll % 100 < chance {
                self.momentum += 1;
                return self.active();
            }
        }
        self.momentum = 0;
        if self.hannibal {
            self.idle(battle);
        }
        self.draw(battle, rng)
    }

    /// Draw among the leaders not yet activated this turn, weighted by
    /// initiative.
    fn draw(&mut self, battle: &Battle, rng: &mut BorlandRand) -> Option<usize> {
        let waiting = |s: &Self, i: usize| !s.done[i];
        let free = if self.free && !self.free_done { FREE_UNITS_INITIATIVE } else { 0 };
        let total: u32 = (0..self.leaders.len()).filter(|&i| waiting(self, i)).map(|i| initiative(battle, self.leaders[i])).sum::<u32>() + free;
        let roll = if total == 0 {
            0
        } else if self.hannibal {
            rng.random() % total
        } else {
            rng.next() % total
        };
        // the free units' entry is the list's first; drawn, they act and
        // the draw goes on
        if roll < free {
            self.free_done = true;
            self.free_drawn = true;
            return self.draw(battle, rng);
        }
        let mut below = free;
        for i in 0..self.leaders.len() {
            if !waiting(self, i) {
                continue;
            }
            let w = initiative(battle, self.leaders[i]);
            if below <= roll && roll < below + w {
                // (Hannibal's: a leader's superior goes first if he hasn't)
                let i = if self.hannibal { self.top_of_chain(battle, i) } else { i };
                self.done[i] = true;
                self.current = Some(i);
                return self.active();
            }
            below += w;
        }
        self.current = None;
        None
    }
}

/// The "Free Units" entry's initiative (Hannibal's `GameControl`).
const FREE_UNITS_INITIATIVE: u32 = 3;

fn initiative(battle: &Battle, unit: usize) -> u32 {
    battle.units[unit].leader.map_or(0, |r| r.initiative as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::{LeaderRatings, Unit};
    use crate::hex::Hex;

    fn leader(name: &str, initiative: u16) -> Unit {
        Unit {
            source: 0,
            name: name.into(),
            type_id: 4,
            nation: 2,
            side: 2,
            group: 0,
            hex: Hex::new(0, 0),
            facing: 3,
            leader: Some(LeaderRatings { charisma: 0, command_range: 4, initiative, personal_combat: 0 }),
            retreat: 7,
            max_mp: 9,
            mp: 9,
            turned: false,
            size: 6,
            tq: 6,
            hits: 0,
            state: crate::battle::UnitState::Fighting,
            arrival: 0,
            elite: false,
            superior: None,
            link: -1,
            legion: None,
            missile: None,
            fired: false,
            wounded: false,
            reordered: false,
            reorder_hit: false,
            rallied: false,
            done: false,
        }
    }

    #[test]
    fn every_leader_goes_once_per_turn() {
        let battle = Battle { units: vec![leader("A", 2), leader("B", 5), leader("C", 0), leader("D", 3)], ..Default::default() };
        let mut act = Activation::new(&battle, None);
        let mut rng = BorlandRand::new(7);
        let mut seen = vec![act.begin_turn(&battle, &mut rng).unwrap()];
        while let Some(l) = act.next(&battle, false, &mut rng) {
            seen.push(l);
        }
        seen.sort();
        // a leader with initiative 0 is never drawn
        assert_eq!(seen, vec![0, 1, 3]);
    }

    /// Hannibal's leaderless units have their own entry in the draw, once a
    /// turn.
    #[test]
    fn free_units_are_drawn_once_a_turn() {
        let hannibal = |mut u: Unit| {
            u.type_id = gboh_formats::units::HANNIBAL;
            u
        };
        let mut free = hannibal(leader("Celts", 0));
        (free.leader, free.group) = (None, 0xffff);
        let battle = Battle { units: vec![hannibal(leader("A", 2)), hannibal(leader("B", 5)), free], ..Default::default() };
        let mut act = Activation::new(&battle, None);
        let mut rng = BorlandRand::new(3);
        let mut drawn = 0;
        for _ in 0..20 {
            let mut next = act.begin_turn(&battle, &mut rng);
            let mut free = std::mem::take(&mut act.free_drawn) as u32;
            while next.is_some() {
                next = act.next(&battle, false, &mut rng);
                free += std::mem::take(&mut act.free_drawn) as u32;
            }
            assert_eq!(free, 1);
            drawn += free;
            act.turn += 1;
        }
        assert_eq!(drawn, 20);
    }

    #[test]
    fn elite_initiative_on_first_turn() {
        let battle = Battle { units: vec![leader("Coenus", 5), leader("Alexander III", 7)], ..Default::default() };
        let mut act = Activation::new(&battle, Some(9));
        assert_eq!(act.begin_turn(&battle, &mut BorlandRand::new(1)), Some(1));
    }
}
