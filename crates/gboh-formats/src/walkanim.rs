//! `WALKANIM.EIF` ("Walking Animation Data"): how each man of each kind of
//! unit walks from hex to hex (see `docs/formats.md`, "Walking").
//!
//! ```text
//! file    := title[128] i16 kind_count i16 record_count kind*
//! kind    := name[128] i16 count i32 unknown u32 next_kind_offset record{count}
//! record  := name[128] facing{8}
//! facing  := step{10} i16 x i16 y
//! step    := leg{3}
//! leg     := (i16 x, i16 y){3} i32 ms{3}
//! ```
//!
//! Kinds are `Foot`, `Mounted`, `Companion`, `Thessalian`, `Leader`,
//! `Elephant`, `Chariot`, `Phalanx` and `Oxybeles`, each with one record per
//! man of the formation plus one. A facing's point is the man's place in the
//! formation; its steps are by the screen direction of the step, each with
//! three variants (level, down, up) of up to three waypoints (zoom-in pixels
//! from the hex) and the milliseconds to reach each.

use crate::Result;
use crate::read::Reader;

/// A way of walking one step: waypoints and the milliseconds to each (unused
/// ones are zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Leg {
    pub points: [(i16, i16); 3],
    pub ms: [i32; 3],
}

impl Leg {
    /// Milliseconds the whole leg takes.
    pub fn total_ms(&self) -> i32 {
        self.ms.iter().sum()
    }
}

/// One man facing one way: his place, and his steps by direction (index 0
/// is direction 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facing {
    pub steps: [[Leg; 3]; 10],
    pub point: (i16, i16),
}

/// One man's walking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Man {
    pub name: String,
    /// By screen facing (index 0 is N).
    pub facings: Vec<Facing>,
}

/// A kind of unit's walking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kind {
    pub name: String,
    pub men: Vec<Man>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkAnim {
    pub kinds: Vec<Kind>,
}

impl WalkAnim {
    pub fn parse(d: &[u8]) -> Result<Self> {
        let mut r = Reader::new(d);
        r.pos = 128;
        let i16 = |r: &mut Reader| r.u16().map(|v| v as i16);
        let i32 = |r: &mut Reader| r.u32().map(|v| v as i32);
        let kind_count = i16(&mut r)?;
        let _records = i16(&mut r)?;
        let mut kinds = Vec::new();
        for _ in 0..kind_count.max(0) {
            let name = r.name(128)?;
            let count = i16(&mut r)?;
            let _unknown = i32(&mut r)?;
            let next = r.u32()? as usize;
            let mut men = Vec::new();
            for _ in 0..count.max(0) {
                let name = r.name(128)?;
                let mut facings = Vec::new();
                for _ in 0..8 {
                    let mut steps = [[Leg::default(); 3]; 10];
                    for step in steps.iter_mut() {
                        for leg in step.iter_mut() {
                            for p in leg.points.iter_mut() {
                                *p = (i16(&mut r)?, i16(&mut r)?);
                            }
                            for t in leg.ms.iter_mut() {
                                *t = i32(&mut r)?;
                            }
                        }
                    }
                    let point = (i16(&mut r)?, i16(&mut r)?);
                    facings.push(Facing { steps, point });
                }
                men.push(Man { name, facings });
            }
            if next != 0 {
                r.pos = next;
            }
            kinds.push(Kind { name, men });
        }
        Ok(Self { kinds })
    }

    pub fn kind(&self, name: &str) -> Option<&Kind> {
        self.kinds.iter().find(|k| k.name == name)
    }
}

impl Kind {
    /// Milliseconds a step in screen direction `dir` takes a unit facing
    /// screen direction `facing` (both 1 N … 8 NW): the slowest man's plain
    /// way (the first variant).
    pub fn step_ms(&self, facing: usize, dir: usize) -> Option<i32> {
        self.step_ms_variant(facing, dir, 0)
    }

    /// As [`Kind::step_ms`], for a variant: 0 on the level, 1 going down, 2
    /// going up (the original's `FUN_004ee88a` compares the two hexes'
    /// levels).
    pub fn step_ms_variant(&self, facing: usize, dir: usize, variant: usize) -> Option<i32> {
        self.men
            .iter()
            .filter_map(|m| Some(m.facings.get(facing.checked_sub(1)?)?.steps.get(dir.checked_sub(1)?)?.get(variant)?.total_ms()))
            .filter(|&t| t > 0)
            .max()
    }

    /// The variant for a step from level `from` to level `to`.
    pub fn variant(from: u8, to: u8) -> usize {
        match to.cmp(&from) {
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Less => 1,
            std::cmp::Ordering::Greater => 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the game's file; runs only with `GBOH_DATA` set.
    #[test]
    fn reads_the_walking_data() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let name = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case("WALKANIM.EIF"))
            .unwrap()
            .path();
        let w = WalkAnim::parse(&std::fs::read(name).unwrap()).unwrap();
        let names: Vec<&str> = w.kinds.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, ["Foot", "Mounted", "Companion", "Thessalian", "Leader", "Elephant", "Chariot", "Phalanx", "Oxybeles"]);
        assert_eq!(w.kind("Foot").unwrap().men.len(), 9);
        assert_eq!(w.kind("Phalanx").unwrap().men.len(), 21);
        // the first man on foot, facing NE, stands where the formation puts him
        assert_eq!(w.kind("Foot").unwrap().men[0].facings[1].point, (-22, -6));
        // a step takes foot about a second, horse about 0.6 s
        let foot = w.kind("Foot").unwrap().step_ms(2, 2).unwrap();
        let horse = w.kind("Mounted").unwrap().step_ms(2, 2).unwrap();
        assert!((900..1200).contains(&foot) && (550..700).contains(&horse), "{foot} {horse}");
    }
}
