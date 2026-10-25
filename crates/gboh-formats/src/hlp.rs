//! `ALEX.HLP`: the game's Windows 3.1 help file (WinHelp 3.1 format,
//! minor version 21), read for its topics' titles and text.
//!
//! ```text
//! file      := header, internal files, directory
//! header    := magic u32 (0x00035F3F), directory u32, free u32, size u32
//! internal  := reserved i32, used i32, flags u8, data[used]
//! directory := an internal file holding a B+ tree of
//!              (name NUL-terminated, offset i32) leaves
//! ```
//!
//! Internal files used: `|SYSTEM` (version, flags: 4 or 8 means the
//! topic blocks are LZ77-compressed), `|Phrases` (common phrases the topic
//! text refers to) and `|TOPIC` (4096-byte blocks of topic links).
//!
//! **LZ77** (topics and phrases): a flag byte, then eight items, a 0 bit
//! a literal byte, a 1 bit a little-endian word: copy `(w >> 12) + 3`
//! bytes from `(w & 0xfff) + 1` back.
//!
//! **Topic blocks:** a 12-byte header (last and first topic link, last
//! topic header), then up to 16 KiB of data once decompressed. A topic
//! position is `block << 14 | (offset into the data + 12)`. Topic links:
//! `block size i32, data length 2 i32, previous i32, next i32, data
//! length 1 i32, record type u8, link data 1 [length 1 − 21], link data
//! 2 [block size − length 1]`; link data 2 is phrase-compressed when
//! length 2 (its expanded length) is the greater (a byte below 0x10 starts a phrase: index `(b − 1) ×
//! 256 + next`, phrase `index / 2`, a space after it if odd). Record 2 is a
//! topic header (title in link data 2), 0x20 a paragraph group: link data
//! 2 its text strings, each NUL ending one at a formatting command of link
//! data 1 (after its paragraph settings): 0x81 line break, 0x82 end of
//! paragraph, 0x83 tab, 0x80 font (2 bytes), 0x86–0x88 picture, 0xE0–0xE7
//! hot spots, 0xFF end.

use crate::{Error, Result};

fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(d.get(o..o + 2).ok_or(Error::Eof(o))?.try_into().unwrap()))
}

fn i32_at(d: &[u8], o: usize) -> Result<i32> {
    Ok(i32::from_le_bytes(d.get(o..o + 4).ok_or(Error::Eof(o))?.try_into().unwrap()))
}

/// Decompress WinHelp LZ77 data, up to `limit` bytes out.
fn lz77(src: &[u8], limit: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(limit.min(1 << 16));
    let mut i = 0;
    while i < src.len() && out.len() < limit {
        let flags = src[i];
        i += 1;
        for bit in 0..8 {
            if i >= src.len() || out.len() >= limit {
                break;
            }
            if flags & (1 << bit) == 0 {
                out.push(src[i]);
                i += 1;
            } else {
                if i + 1 >= src.len() {
                    return out;
                }
                let w = u16::from_le_bytes([src[i], src[i + 1]]) as usize;
                i += 2;
                let (len, dist) = ((w >> 12) + 3, (w & 0xfff) + 1);
                for _ in 0..len {
                    let b = if dist <= out.len() { out[out.len() - dist] } else { b' ' };
                    out.push(b);
                }
            }
        }
    }
    out
}

/// A help topic: its title, its position (for contexts), and its text in
/// paragraphs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    pub title: String,
    pub position: u32,
    pub paragraphs: Vec<String>,
}

/// A help file's title and topics, in order.
#[derive(Debug, Clone)]
pub struct Help {
    pub title: String,
    pub topics: Vec<Topic>,
}

/// The internal files: name and offset.
fn directory(d: &[u8]) -> Result<Vec<(String, usize)>> {
    if u16_at(d, 0)? != 0x5f3f {
        return Err(Error::Invalid { offset: 0, msg: "not a WinHelp file".into() });
    }
    let dir = i32_at(d, 4)? as usize + 9;
    btree(d, dir, |page, o| {
        let e = page[o..].iter().position(|&b| b == 0).map(|p| o + p).ok_or(Error::Eof(o))?;
        let name = String::from_utf8_lossy(&page[o..e]).into_owned();
        Ok(((name, i32_at(page, e + 1)? as usize), e + 5))
    })
}

/// The leaves of the B+ tree at `o` (its header), each read by `leaf`
/// (page bytes, offset → entry and the next offset).
fn btree<T>(d: &[u8], o: usize, mut leaf: impl FnMut(&[u8], usize) -> Result<(T, usize)>) -> Result<Vec<T>> {
    let page_size = u16_at(d, o + 4)? as usize;
    let levels = u16_at(d, o + 32)?;
    let first = o + 38;
    // the first leaf: down the first entries of the index pages
    let mut page = u16_at(d, o + 26)? as usize;
    for _ in 1..levels {
        page = u16_at(d, first + page * page_size + 4)? as usize;
    }
    let mut out = Vec::new();
    loop {
        let p = first + page * page_size;
        let bytes = d.get(p..p + page_size).ok_or(Error::Eof(p))?;
        let n = u16_at(bytes, 2)?;
        let next = u16_at(bytes, 6)? as i16;
        let mut q = 8;
        for _ in 0..n {
            let (e, nq) = leaf(bytes, q)?;
            out.push(e);
            q = nq;
        }
        if next < 0 {
            return Ok(out);
        }
        page = next as usize;
    }
}

/// A compressed field reader for link data 1.
struct Fields<'a> {
    d: &'a [u8],
    o: usize,
}

impl Fields<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.d.get(self.o)?;
        self.o += 1;
        Some(b)
    }
    fn word(&mut self) -> Option<u16> {
        let w = u16::from_le_bytes([*self.d.get(self.o)?, *self.d.get(self.o + 1)?]);
        self.o += 2;
        Some(w)
    }
    fn dword(&mut self) -> Option<u32> {
        let w = u32::from_le_bytes(self.d.get(self.o..self.o + 4)?.try_into().ok()?);
        self.o += 4;
        Some(w)
    }
    /// A compressed unsigned short (and signed: the same bytes).
    fn cshort(&mut self) -> Option<()> {
        if *self.d.get(self.o)? & 1 == 0 { self.byte().map(drop) } else { self.word().map(drop) }
    }
    fn clong(&mut self) -> Option<u32> {
        if *self.d.get(self.o)? & 1 == 0 { self.word().map(|w| (w >> 1) as u32) } else { self.dword().map(|w| w >> 1) }
    }
}

/// The formatting commands of a paragraph group's link data 1 (record
/// 0x20, or 0x23 for a table: its columns, then each cell's settings and
/// commands, ended by 0xFF): what each NUL of the text stands for.
fn commands(d: &[u8], table: bool) -> Vec<u8> {
    let mut f = Fields { d, o: 0 };
    let mut out = Vec::new();
    let _ = (|| -> Option<()> {
        f.clong()?; // topic size
        f.cshort()?; // topic length
        if !table {
            return paragraph(&mut f, &mut out);
        }
        let columns = f.byte()? as usize;
        let kind = f.byte()?;
        if kind == 0 || kind == 2 {
            f.word()?;
        }
        f.o += columns * 4;
        loop {
            if f.word()? as i16 == -1 {
                return Some(());
            }
            paragraph(&mut f, &mut out)?;
        }
    })();
    out
}

/// One paragraph's settings (skipped) and its commands, up to 0xFF.
fn paragraph(f: &mut Fields, out: &mut Vec<u8>) -> Option<()> {
    f.byte()?;
    f.byte()?;
    f.word()?; // id
    let bits = f.word()?;
    if bits & 1 != 0 {
        f.clong()?;
    }
    for b in [2, 4, 8, 0x10, 0x20, 0x40] {
        if bits & b != 0 {
            f.cshort()?;
        }
    }
    if bits & 0x100 != 0 {
        f.byte()?;
        f.word()?;
    }
    if bits & 0x200 != 0 {
        // the count is a compressed signed short
        let n = if *f.d.get(f.o)? & 1 == 0 { (f.byte()? >> 1) as i32 - 0x40 } else { (f.word()? >> 1) as i32 - 0x4000 };
        for _ in 0..n.max(0) {
            let stop = if *f.d.get(f.o)? & 1 == 0 { (f.byte()? >> 1) as u16 } else { f.word()? >> 1 };
            if stop & 0x4000 != 0 {
                f.cshort()?;
            }
        }
    }
    loop {
        let c = f.byte()?;
        out.push(c);
        match c {
            0xff => return Some(()),
            0x20 => {
                f.dword()?;
            }
            0x21 | 0x80 => {
                f.word()?;
            }
            0x81 | 0x82 | 0x83 | 0x89 | 0x8b | 0x8c => {}
            0x86..=0x88 => {
                let kind = f.byte()?;
                let size = f.clong()? as usize;
                if kind == 0x22 {
                    f.cshort()?;
                }
                f.o += size;
            }
            0xc8 | 0xcc => {
                let n = f.word()? as usize;
                f.o += n;
            }
            0xe0..=0xe7 => {
                f.dword()?;
            }
            0xea..=0xef => {
                let n = f.word()? as usize;
                f.o += n;
            }
            _ => return None,
        }
    }
}

impl Help {
    pub fn parse(d: &[u8]) -> Result<Self> {
        let files = directory(d)?;
        let file = |name: &str| -> Result<&[u8]> {
            let &(_, o) = files.iter().find(|(n, _)| n == name).ok_or(Error::Invalid { offset: 0, msg: format!("no {name}") })?;
            let used = i32_at(d, o + 4)? as usize;
            d.get(o + 9..o + 9 + used).ok_or(Error::Eof(o))
        };
        let system = file("|SYSTEM")?;
        let minor = u16_at(system, 2)?;
        let flags = u16_at(system, 10)?;
        let compressed = minor > 16 && flags & 0xc != 0;
        let mut title = String::new();
        let mut o = 12;
        while o + 4 <= system.len() {
            let (kind, size) = (u16_at(system, o)?, u16_at(system, o + 2)? as usize);
            if kind == 1 {
                let t = system.get(o + 4..o + 4 + size).unwrap_or_default();
                title = String::from_utf8_lossy(t.split(|&b| b == 0).next().unwrap_or_default()).into_owned();
            }
            o += 4 + size;
        }
        // the phrases
        let phrases: Vec<Vec<u8>> = match file("|Phrases") {
            Ok(p) => {
                let n = u16_at(p, 0)? as usize;
                let (offsets_at, text) = if minor > 16 { (8, true) } else { (4, false) };
                let offsets: Vec<usize> = (0..=n).map(|k| u16_at(p, offsets_at + 2 * k).map(|v| v as usize)).collect::<Result<_>>()?;
                let base = offsets[0];
                let data = if text { lz77(&p[offsets_at + base..], i32_at(p, 4)? as usize) } else { p[offsets_at + base..].to_vec() };
                offsets.windows(2).map(|w| data.get(w[0] - base..w[1] - base).unwrap_or_default().to_vec()).collect()
            }
            Err(_) => Vec::new(),
        };
        // the topic blocks, decompressed and laid end to end
        let topic = file("|TOPIC")?;
        let mut data = Vec::new();
        let mut starts = Vec::new();
        for block in topic.chunks(4096) {
            starts.push(data.len());
            let body = block.get(12..).unwrap_or_default();
            if compressed {
                data.extend(lz77(body, 0x4000 - 12));
            } else {
                data.extend_from_slice(body);
            }
        }
        let at = |pos: i32| -> Option<usize> {
            let (b, off) = ((pos as u32 >> 14) as usize, (pos as u32 & 0x3fff) as usize);
            Some(*starts.get(b)? + off.checked_sub(12)?)
        };
        let unphrase = |src: &[u8]| -> Vec<u8> {
            let mut out = Vec::new();
            let mut i = 0;
            while i < src.len() {
                let c = src[i];
                if (1..0x10).contains(&c) && i + 1 < src.len() {
                    let idx = (c as usize - 1) * 256 + src[i + 1] as usize;
                    out.extend_from_slice(phrases.get(idx / 2).map_or(&[][..], |p| p.as_slice()));
                    if idx & 1 == 1 {
                        out.push(b' ');
                    }
                    i += 2;
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            out
        };
        let first = i32_at(topic, 4)?;
        let mut topics: Vec<Topic> = Vec::new();
        let mut pos = first;
        let mut guard = 0;
        while let Some(o) = at(pos) {
            guard += 1;
            if guard > 200_000 || o + 21 > data.len() {
                break;
            }
            let block_size = i32_at(&data, o)? as usize;
            let len2 = i32_at(&data, o + 4)? as usize;
            let next = i32_at(&data, o + 12)?;
            let len1 = i32_at(&data, o + 16)? as usize;
            let kind = data[o + 20];
            let link1 = data.get(o + 21..o + len1).unwrap_or_default();
            // link data 2 as stored, and phrase-compressed if it holds less
            // than its length once expanded
            let stored = block_size.saturating_sub(len1);
            let raw2 = data.get(o + len1..o + len1 + stored).unwrap_or_default();
            let link2 = if len2 > stored { unphrase(raw2) } else { raw2.to_vec() };
            match kind {
                2 => {
                    let t = link2.split(|&b| b == 0).next().unwrap_or_default();
                    topics.push(Topic { title: cp1252(t), position: pos as u32, paragraphs: Vec::new() });
                }
                0x20 | 0x23 => {
                    if let Some(t) = topics.last_mut() {
                        let cmds = commands(link1, kind == 0x23);
                        let mut para = String::new();
                        for (k, piece) in link2.split(|&b| b == 0).enumerate() {
                            para.push_str(&cp1252(piece));
                            match cmds.get(k) {
                                Some(0x82) | Some(0xff) => {
                                    let p = para.trim().to_string();
                                    if !p.is_empty() {
                                        t.paragraphs.push(p);
                                    }
                                    para.clear();
                                }
                                Some(0x81) | Some(0x89) => para.push('\n'),
                                Some(0x83) => para.push('\t'),
                                _ => {}
                            }
                        }
                        let p = para.trim().to_string();
                        if !p.is_empty() {
                            t.paragraphs.push(p);
                        }
                    }
                }
                _ => {}
            }
            if next <= pos && next != -1 || next == -1 {
                break;
            }
            pos = next;
        }
        Ok(Self { title, topics })
    }
}

fn cp1252(b: &[u8]) -> String {
    b.iter()
        .map(|&c| match c {
            0x91 | 0x92 => '\'',
            0x93 | 0x94 => '"',
            0x96 | 0x97 => '-',
            0x85 => '…',
            c => c as char,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hannibal's help file reads too, with its tutorial topic; runs only
    /// with `GBOH_HANNIBAL`.
    #[test]
    fn reads_hannibal_help() {
        let Ok(dir) = std::env::var("GBOH_HANNIBAL") else { return };
        let path = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("HANNIBAL.HLP")).unwrap();
        let h = Help::parse(&std::fs::read(path).unwrap()).unwrap();
        assert!(h.topics.len() > 10);
        assert!(h.topics.iter().any(|t| t.title.contains("Tutorial")), "{:?}", h.topics.iter().map(|t| &t.title).take(40).collect::<Vec<_>>());
    }

    #[test]
    fn decompresses() {
        // "abcabcabc": literals a b c, then a copy of 6 from 3 back
        assert_eq!(lz77(&[0b1000, b'a', b'b', b'c', 0x02, 0x30], 100), b"abcabcabc");
    }

    /// Reads the game's help file; runs only with `GBOH_DATA` set.
    #[test]
    fn reads_the_help() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let d = std::fs::read(std::path::Path::new(&dir).join("ALEX.HLP")).unwrap();
        let h = Help::parse(&d).unwrap();
        assert!(h.title.contains("Alexander"));
        assert!(h.topics.len() > 50, "{} topics", h.topics.len());
        if let Some(t) = h.topics.iter().find(|t| t.title.starts_with("Hydaspes  (")) {
            for p in &t.paragraphs {
                println!("| {p}");
            }
        }
        for t in h.topics.iter().take(400) {
            println!("== {} ({} paragraphs) {:?}", t.title, t.paragraphs.len(), t.paragraphs.first().map(|p| p.chars().take(80).collect::<String>()));
        }
    }
}
