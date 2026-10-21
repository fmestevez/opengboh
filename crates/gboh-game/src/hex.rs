//! Hex coordinates on a battle map.
//!
//! Map hexes are addressed by column `x` and row `y`; odd columns sit half a
//! hex lower than even ones (see `gboh_formats::map`).

/// A map hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hex {
    pub x: i32,
    pub y: i32,
}

impl Hex {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Axial coordinates `(q, r)`.
    fn axial(self) -> (i32, i32) {
        (self.x, self.y - (self.x - (self.x & 1)) / 2)
    }

    /// Distance in hexes.
    pub fn distance(self, other: Hex) -> i32 {
        let (q1, r1) = self.axial();
        let (q2, r2) = other.axial();
        let (dq, dr) = (q2 - q1, r2 - r1);
        dq.abs().max(dr.abs()).max((dq + dr).abs())
    }

    /// The six neighbours, clockwise from N.
    pub fn neighbours(self) -> [Hex; 6] {
        let (x, y) = (self.x, self.y);
        // odd columns are shifted down, so their upper diagonals share the row
        let up = if x & 1 == 0 { y - 1 } else { y };
        [
            Hex::new(x, y - 1),
            Hex::new(x + 1, up),
            Hex::new(x + 1, up + 1),
            Hex::new(x, y + 1),
            Hex::new(x - 1, up + 1),
            Hex::new(x - 1, up),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbours_are_one_apart() {
        for h in [Hex::new(4, 4), Hex::new(5, 4)] {
            for n in h.neighbours() {
                assert_eq!(h.distance(n), 1, "{h:?} -> {n:?}");
            }
        }
    }

    #[test]
    fn distance() {
        // Hydaspes: Alexander III to Antigenes and to Craterus
        assert_eq!(Hex::new(11, 15).distance(Hex::new(12, 8)), 8);
        assert_eq!(Hex::new(11, 15).distance(Hex::new(21, 26)), 16);
        assert_eq!(Hex::new(0, 0).distance(Hex::new(0, 3)), 3);
        assert_eq!(Hex::new(0, 0).distance(Hex::new(4, 0)), 4);
    }
}
