use crate::{Error, Result};

/// Little-endian cursor over a byte slice.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        let end = end.ok_or(Error::Eof(self.pos))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    /// Fixed-size, NUL-terminated name field. Erudite's tools reused name
    /// buffers, so bytes after the first NUL are garbage and are ignored.
    pub fn name(&mut self, len: usize) -> Result<String> {
        let raw = self.bytes(len)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(len);
        Ok(raw[..end].iter().map(|&b| b as char).collect())
    }
}
