//! `.MAP` battle maps.
//!
//! ```text
//! file  := u16 name_len, name[name_len], u8 width, u8 height,
//!          u16 hex_id_origin, u16 unk_b, u8 elevation[width*height],
//!          cell[(width*2+2) * (height*4+1)]
//! cell  := u16 count, u16 feature[count]
//! ```
//!
//! Hexes are in columns; odd columns are shifted down by half a hex. The
//! feature grid uses "doubled" coordinates where hex `(x, y)` sits at
//! `(2x + 2, 4y + 2 + 2*(x&1))`; the cells between hex centres hold features
//! of hex sides (river banks, walls, ...). Elevation 0 marks water (sea,
//! rivers). The offset was confirmed by matching streams against the game.
//!
//! Hexes are identified elsewhere (scenarios) by `hex_id_origin + x*100 + y`.
//!
//! Feature code meanings are not yet confirmed; see `docs/formats.md`.

use crate::read::Reader;
use crate::{Error, Result};

/// Neighbour directions, in the order the original loader assigns side bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    N,
    NE,
    SE,
    S,
    SW,
    NW,
}

impl Side {
    pub const ALL: [Side; 6] = [Side::N, Side::NE, Side::SE, Side::S, Side::SW, Side::NW];

    /// Offset from a hex centre to the side cell, in doubled coordinates.
    pub fn offset(self) -> (i32, i32) {
        match self {
            Side::N => (0, -2),
            Side::NE => (1, -1),
            Side::SE => (1, 1),
            Side::S => (0, 2),
            Side::SW => (-1, 1),
            Side::NW => (-1, -1),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Map {
    pub name: String,
    pub width: usize,
    pub height: usize,
    /// Added to `x*100 + y` to form hex ids used by scenarios (1000 or 3600).
    pub hex_id_origin: u16,
    pub unk_b: u16,
    /// Row-major, `width * height`; 0 = water, 1..=5 = land levels.
    pub elevation: Vec<u8>,
    /// Feature grid, `grid_width() * grid_height()` cells.
    pub cells: Vec<Vec<u16>>,
    /// Whether its terrain is read by Caesar's rules (its codes: 3 marsh,
    /// 4 berm, 12 road; sides 9 and 11 walls, 10 ditches). Set by the game
    /// for Caesar's battles; the file doesn't say.
    pub caesar: bool,
    /// Whether road to road (hex code 12) costs a flat movement point
    /// (Caesar's rule; an option for the others). Set by the game.
    pub fast_roads: bool,
}

impl Map {
    /// The map as a file: the games' layout, behind the *Collector's
    /// Edition* editor's header (a u32 0 and `guid`) if given one.
    pub fn write(&self, editor_guid: Option<[u8; 16]>) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(guid) = editor_guid {
            out.extend(0u32.to_le_bytes());
            out.extend(guid);
        }
        out.extend((self.name.len() as u16).to_le_bytes());
        out.extend(self.name.bytes());
        out.extend([self.width as u8, self.height as u8]);
        out.extend(self.hex_id_origin.to_le_bytes());
        out.extend(self.unk_b.to_le_bytes());
        out.extend(&self.elevation);
        for cell in &self.cells {
            out.extend((cell.len() as u16).to_le_bytes());
            for c in cell {
                out.extend(c.to_le_bytes());
            }
        }
        out
    }

    /// A new flat map, `width` × `height` hexes at elevation 1.
    pub fn blank(name: &str, width: usize, height: usize) -> Self {
        let n = (width * 2 + 2) * (height * 4 + 1);
        Self {
            name: name.into(),
            width,
            height,
            hex_id_origin: 1000,
            unk_b: 0,
            elevation: vec![1; width * height],
            cells: vec![Vec::new(); n],
            caesar: false,
            fast_roads: false,
        }
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        // (the Collector's Edition's editor writes a u32 0 and a GUID first)
        let data = if data.len() > 20 && data[..4] == [0; 4] { &data[20..] } else { data };
        let mut r = Reader::new(data);
        let len = r.u16()? as usize;
        let name = r.bytes(len)?.iter().map(|&b| b as char).collect();
        let dims = r.bytes(2)?;
        let (width, height) = (dims[0] as usize, dims[1] as usize);
        if width == 0 || height == 0 {
            return Err(Error::Invalid { offset: r.pos - 2, msg: "zero map dimension".into() });
        }
        let hex_id_origin = r.u16()?;
        let unk_b = r.u16()?;
        let elevation = r.bytes(width * height)?.to_vec();
        let n = (width * 2 + 2) * (height * 4 + 1);
        let mut cells = Vec::with_capacity(n);
        for _ in 0..n {
            let count = r.u16()? as usize;
            cells.push((0..count).map(|_| r.u16()).collect::<Result<_>>()?);
        }
        if r.remaining() != 0 {
            return Err(Error::Invalid { offset: r.pos, msg: "trailing bytes after feature grid".into() });
        }
        Ok(Self { name, width, height, hex_id_origin, unk_b, elevation, cells, caesar: false, fast_roads: false })
    }

    pub fn grid_width(&self) -> usize {
        self.width * 2 + 2
    }

    pub fn grid_height(&self) -> usize {
        self.height * 4 + 1
    }

    /// Convert a scenario hex id to `(x, y)`, if it lies on this map.
    pub fn hex_from_id(&self, id: u16) -> Option<(usize, usize)> {
        let rel = id.checked_sub(self.hex_id_origin)? as usize;
        let (x, y) = (rel / 100, rel % 100);
        (x < self.width && y < self.height).then_some((x, y))
    }

    pub fn elevation_at(&self, x: usize, y: usize) -> u8 {
        self.elevation[y * self.width + x]
    }

    /// Doubled-grid coordinates of hex `(x, y)`'s centre. Grid column 0 is
    /// unused. The column is the original's `id / 50 + 2` with
    /// `id = 100x + y`: `2x + 2`, but one more from `y = 50` on (only
    /// Paraitacene is that tall).
    pub fn hex_cell(x: usize, y: usize) -> (i32, i32) {
        (((x * 100 + y) / 50 + 2) as i32, (y * 4 + 2 + (x & 1) * 2) as i32)
    }

    pub fn cell(&self, gx: i32, gy: i32) -> &[u16] {
        if gx < 0 || gy < 0 || gx as usize >= self.grid_width() || gy as usize >= self.grid_height() {
            return &[];
        }
        &self.cells[gy as usize * self.grid_width() + gx as usize]
    }

    /// Features of the hex itself (terrain).
    pub fn hex_features(&self, x: usize, y: usize) -> &[u16] {
        let (gx, gy) = Self::hex_cell(x, y);
        self.cell(gx, gy)
    }

    /// Features on one side of a hex (rivers, walls, ...).
    pub fn side_features(&self, x: usize, y: usize, side: Side) -> &[u16] {
        let (gx, gy) = Self::hex_cell(x, y);
        let (dx, dy) = side.offset();
        self.cell(gx + dx, gy + dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_column_steps_at_y_50() {
        assert_eq!(Map::hex_cell(3, 49), (8, 200));
        assert_eq!(Map::hex_cell(3, 50), (9, 204));
    }

    #[test]
    fn parses_minimal_map() {
        // 1x1 map: name "a", elevation 2, grid 4x5 cells, the hex centre
        // (2, 2) with a feature.
        let mut d = vec![1, 0, b'a', 1, 1, 0xe8, 0x03, 1, 0, 2];
        for i in 0..4 * 5 {
            if i == 2 * 4 + 2 {
                d.extend_from_slice(&[1, 0, 20, 0]);
            } else {
                d.extend_from_slice(&[0, 0]);
            }
        }
        let m = Map::parse(&d).unwrap();
        assert_eq!((m.width, m.height, m.hex_id_origin), (1, 1, 1000));
        assert_eq!(m.hex_from_id(1000), Some((0, 0)));
        assert_eq!(m.hex_from_id(1100), None);
        assert_eq!(m.elevation_at(0, 0), 2);
        assert_eq!(m.hex_features(0, 0), &[20]);
        assert!(m.side_features(0, 0, Side::S).is_empty());
    }
}
