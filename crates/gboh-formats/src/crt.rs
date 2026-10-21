//! The original's C runtime, where the game's behaviour depends on it.

/// Borland C++ `rand()`, which the original uses for every random choice
/// (art variants, activation order, combat).
#[derive(Debug, Clone)]
pub struct BorlandRand(u32);

impl BorlandRand {
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(22695477).wrapping_add(1);
        (self.0 >> 16) & 0x7fff
    }
}
