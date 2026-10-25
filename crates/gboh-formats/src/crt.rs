//! The original's C runtime, where the game's behaviour depends on it.

/// Borland C++ `rand()`, which the original uses for most random choices
/// (art variants, activation order, combat), and `random()` (`FUN_005b1170`)
/// for a few (missile replenishment, leaders withdrawing). They share a
/// state: `rand()` steps its low 32 bits (`× 0x015A4E35 + 1`, returning
/// bits 16–30), `random()` the whole 64 (`× 0x000015A4_00004E35 + 1`,
/// returning the high word's low 31 bits); `srand` sets the low word and
/// clears the high.
#[derive(Debug, Clone)]
pub struct BorlandRand(u32, u32);

impl BorlandRand {
    pub fn new(seed: u32) -> Self {
        Self(seed, 0)
    }

    /// The low word of the state, to carry it on later with
    /// [`BorlandRand::new`] (as long as `random()` was not used).
    pub fn state(&self) -> u32 {
        self.0
    }

    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(22695477).wrapping_add(1);
        (self.0 >> 16) & 0x7fff
    }

    /// `random()`: 31 random bits from the 64-bit state.
    pub fn random(&mut self) -> u32 {
        let s = ((self.1 as u64) << 32 | self.0 as u64).wrapping_mul(0x0000_15A4_0000_4E35).wrapping_add(1);
        (self.0, self.1) = (s as u32, (s >> 32) as u32);
        self.1 & 0x7fff_ffff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_shares_rand_state() {
        // with the high word clear, random() steps the low word as rand() does
        let (mut a, mut b) = (BorlandRand::new(7), BorlandRand::new(7));
        a.random();
        b.next();
        assert_eq!(a.state(), 7u32.wrapping_mul(0x4E35).wrapping_add(1));
        assert_eq!(b.state(), 7u32.wrapping_mul(22695477).wrapping_add(1));
        // the high word: 7 × 0x15A4 (no carry from the low product)
        assert_eq!(a.1, 7 * 0x15A4);
    }
}
