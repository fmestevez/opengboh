//! `GAME.PAL`: the 256-colour game palette plus colour remap tables.
//!
//! ```text
//! file := name[15] ("Erudite Palette", no NUL), RGBQUAD[256] (B, G, R, 0),
//!         remap[5][256]
//! ```
//! Remap tables map a palette index to another index. Table 0 darkens
//! (shadows), table 1 darkens slightly, tables 2..=4 brighten.

/// The palette's file name in each game's `DATA` directory (Alexander's,
/// Hannibal's, Caesar's).
pub const FILES: [&str; 3] = ["GAME.PAL", "HANNIBAL.PAL", "CAESAR.PAL"];

use crate::read::Reader;
use crate::{Error, Result};

pub const REMAP_TABLES: usize = 5;

#[derive(Debug, Clone)]
pub struct Palette {
    pub name: String,
    pub colors: [[u8; 3]; 256],
    pub remap: Vec<[u8; 256]>,
}

impl Palette {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let name = r.bytes(15)?.iter().map(|&b| b as char).collect();
        let mut colors = [[0u8; 3]; 256];
        for c in colors.iter_mut() {
            let q = r.bytes(4)?;
            *c = [q[2], q[1], q[0]];
        }
        let mut remap = Vec::with_capacity(REMAP_TABLES);
        while r.remaining() >= 256 {
            remap.push(r.bytes(256)?.try_into().unwrap());
        }
        if r.remaining() != 0 {
            return Err(Error::Invalid { offset: r.pos, msg: "trailing bytes after remap tables".into() });
        }
        Ok(Self { name, colors, remap })
    }
}
