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
}

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
        if let Some(i) = self.elite(battle, rng) {
            self.elite_uses = if self.turn == 0 { 1 } else { self.elite_uses + 1 };
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
            if rng.next() % 100 < chance {
                self.momentum += 1;
                return self.active();
            }
        }
        self.momentum = 0;
        self.draw(battle, rng)
    }

    /// Draw among the leaders not yet activated this turn, weighted by
    /// initiative.
    fn draw(&mut self, battle: &Battle, rng: &mut BorlandRand) -> Option<usize> {
        let waiting = |s: &Self, i: usize| !s.done[i];
        let total: u32 = (0..self.leaders.len()).filter(|&i| waiting(self, i)).map(|i| initiative(battle, self.leaders[i])).sum();
        let roll = if total == 0 { 0 } else { rng.next() % total };
        let mut below = 0;
        for i in 0..self.leaders.len() {
            if !waiting(self, i) {
                continue;
            }
            let w = initiative(battle, self.leaders[i]);
            if below <= roll && roll < below + w {
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
            missile: None,
            fired: false,
            wounded: false,
            reordered: false,
            reorder_hit: false,
            done: false,
        }
    }

    #[test]
    fn every_leader_goes_once_per_turn() {
        let battle = Battle { units: vec![leader("A", 2), leader("B", 5), leader("C", 0), leader("D", 3)] };
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

    #[test]
    fn elite_initiative_on_first_turn() {
        let battle = Battle { units: vec![leader("Coenus", 5), leader("Alexander III", 7)] };
        let mut act = Activation::new(&battle, Some(9));
        assert_eq!(act.begin_turn(&battle, &mut BorlandRand::new(1)), Some(1));
    }
}
