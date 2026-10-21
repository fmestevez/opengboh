//! EIF ("Erudite Image File") sprite containers.
//!
//! Layout (all little-endian):
//! ```text
//! file   := title[128] u16 unk_a u16 unk_b strip*
//! strip  := name[128] u16 cel_count u32 start u32 end cel{cel_count}
//! cel    := name[128] u16 width u16 height u16 hot_x u16 hot_y u16 kind
//!           u32 size u32 prev_end u32 end pixels[size]
//! ```
//! Strip names are usually facings (`N`, `NE`, ...) and cels are the
//! animation frames for that facing.
//!
//! Pixel encodings by `kind`:
//! * 1, 2, 3, 5: raw 8-bit palette indices, rows padded to 4 bytes, top-down.
//! * 4, 6: row-based RLE (see [`decode_rle`]).
//!
//! Index 0 is transparent in sprites. RLE cels may also contain "effect"
//! spans (RLE ops 3/4) with no stored colour, reported via
//! [`Image::effect`]. They appear under arrows, javelins and some units and
//! are most likely shadow/translucency spans; exact semantics are TBD.

use crate::read::Reader;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Eif {
    pub title: String,
    pub unk_a: u16,
    pub unk_b: u16,
    pub strips: Vec<Strip>,
}

#[derive(Debug, Clone)]
pub struct Strip {
    pub name: String,
    pub cels: Vec<Cel>,
}

#[derive(Debug, Clone)]
pub struct Cel {
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub hot_x: u16,
    pub hot_y: u16,
    pub kind: u16,
    /// Encoded pixel data as stored in the file.
    pub data: Vec<u8>,
}

impl Eif {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let title = r.name(128)?;
        let unk_a = r.u16()?;
        let unk_b = r.u16()?;
        let mut strips = Vec::new();
        while r.remaining() > 0 {
            let name = r.name(128)?;
            let count = r.u16()?;
            let _start = r.u32()?;
            let _end = r.u32()?;
            let mut cels = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let name = r.name(128)?;
                let width = r.u16()?;
                let height = r.u16()?;
                let hot_x = r.u16()?;
                let hot_y = r.u16()?;
                let kind = r.u16()?;
                let size = r.u32()? as usize;
                let _prev_end = r.u32()?;
                let _end = r.u32()?;
                let data = r.bytes(size)?.to_vec();
                cels.push(Cel { name, width, height, hot_x, hot_y, kind, data });
            }
            strips.push(Strip { name, cels });
        }
        Ok(Self { title, unk_a, unk_b, strips })
    }
}

/// A decoded cel: `width * height` palette indices (row-major, top-down)
/// plus a parallel effect layer (0 = none, otherwise the RLE op, 3 or 4).
#[derive(Debug, Clone)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    pub effect: Vec<u8>,
}

impl Cel {
    pub fn decode(&self) -> Result<Image> {
        let (w, h) = (self.width as usize, self.height as usize);
        match self.kind {
            1 | 2 | 3 | 5 => {
                let stride = (w + 3) & !3;
                if self.data.len() < stride * h {
                    return Err(Error::Invalid {
                        offset: 0,
                        msg: format!("raw cel {}x{} needs {} bytes, has {}", w, h, stride * h, self.data.len()),
                    });
                }
                let mut pixels = Vec::with_capacity(w * h);
                for row in self.data.chunks(stride).take(h) {
                    pixels.extend_from_slice(&row[..w]);
                }
                Ok(Image { width: w, height: h, pixels, effect: vec![0; w * h] })
            }
            4 | 6 => decode_rle(&self.data, w, h),
            k => Err(Error::Invalid { offset: 0, msg: format!("unknown cel kind {k}") }),
        }
    }
}

/// Row-based RLE used by cel kinds 4 and 6.
///
/// Each row is `u16 len` (including itself) followed by `u16` commands whose
/// top 4 bits are the op and low 12 bits a pixel count: op 1 skips `count`
/// transparent pixels, op 2 copies the next `count` literal bytes, ops 3 and
/// 4 mark `count` effect pixels with no stored data.
pub fn decode_rle(data: &[u8], width: usize, height: usize) -> Result<Image> {
    let mut pixels = vec![0u8; width * height];
    let mut effect = vec![0u8; width * height];
    let mut r = Reader::new(data);
    for y in 0..height {
        let row_start = r.pos;
        let len = r.u16()? as usize;
        let row_end = row_start + len;
        let mut x = 0;
        while r.pos < row_end {
            let at = r.pos;
            let word = r.u16()?;
            let (count, op) = ((word & 0x0fff) as usize, (word >> 12) as u8);
            if x + count > width {
                return Err(Error::Invalid { offset: at, msg: format!("row {y} overflows width {width}") });
            }
            match op {
                1 => {}
                2 => pixels[y * width + x..][..count].copy_from_slice(r.bytes(count)?),
                3 | 4 => effect[y * width + x..][..count].fill(op),
                _ => return Err(Error::Invalid { offset: at, msg: format!("unknown RLE op {op}") }),
            }
            x += count;
        }
        if r.pos != row_end {
            return Err(Error::Invalid { offset: r.pos, msg: format!("row {y} overruns its length") });
        }
    }
    Ok(Image { width, height, pixels, effect })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_ops() {
        // One 6-pixel row: skip 1, literal [7, 8], effect(3) x2, skip 1.
        let row = [0x0c, 0x00, 0x01, 0x10, 0x02, 0x20, 7, 8, 0x02, 0x30, 0x01, 0x10];
        let img = decode_rle(&row, 6, 1).unwrap();
        assert_eq!(img.pixels, [0, 7, 8, 0, 0, 0]);
        assert_eq!(img.effect, [0, 0, 0, 3, 3, 0]);
    }

    #[test]
    fn rle_count_uses_low_12_bits() {
        // Skip 0x105 pixels in a single command (op 1, count 0x105).
        let row = [0x04, 0x00, 0x05, 0x11];
        let img = decode_rle(&row, 0x105, 1).unwrap();
        assert!(img.pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn rle_rejects_overflow() {
        let row = [0x04, 0x00, 0x05, 0x10];
        assert!(decode_rle(&row, 4, 1).is_err());
    }
}
