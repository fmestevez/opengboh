//! Resources of the game's executable (`alex.exe`): its string table and
//! dialog templates. The rewrite reads text (battle names and descriptions,
//! captions) from the player's own copy rather than carrying it.

use crate::{Error, Result};

fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(d.get(o..o + 2).ok_or(Error::Eof(o))?.try_into().unwrap()))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(d.get(o..o + 4).ok_or(Error::Eof(o))?.try_into().unwrap()))
}

/// A resource name: a number or a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Name {
    Id(u16),
    Text(String),
}

/// One resource's data.
#[derive(Debug, Clone)]
pub struct Resource {
    pub kind: Name,
    pub name: Name,
    pub data: Vec<u8>,
}

/// A control of a dialog template, in dialog units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub id: u16,
    /// Window class: a predefined one by number (0x80 button, 0x81 edit,
    /// 0x82 static, …) or by name.
    pub class: Name,
    pub text: String,
    pub style: u32,
    pub rect: (i16, i16, i16, i16),
}

/// A dialog template (`DLGTEMPLATE`), in dialog units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub title: String,
    pub class: String,
    pub size: (i16, i16),
    pub controls: Vec<Control>,
}

/// The executable's resources.
pub struct Resources {
    pub all: Vec<Resource>,
}

impl Resources {
    pub fn parse(d: &[u8]) -> Result<Self> {
        let pe = u32_at(d, 0x3c)? as usize;
        if d.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err(Error::Invalid { offset: pe, msg: "not a PE file".into() });
        }
        let sections = u16_at(d, pe + 6)? as usize;
        let optional = u16_at(d, pe + 20)? as usize;
        // the resource directory is data directory 2 of the PE32 header
        let rsrc_rva = u32_at(d, pe + 24 + 96 + 16)?;
        let mut secs = Vec::new();
        for i in 0..sections {
            let o = pe + 24 + optional + i * 40;
            let (vsize, va, rsize, raw) = (u32_at(d, o + 8)?, u32_at(d, o + 12)?, u32_at(d, o + 16)?, u32_at(d, o + 20)?);
            secs.push((va, vsize.max(rsize), raw));
        }
        let file_offset = |rva: u32| {
            secs.iter().find(|&&(va, size, _)| va <= rva && rva < va + size).map(|&(va, _, raw)| (raw + rva - va) as usize)
        };
        let base = file_offset(rsrc_rva).ok_or(Error::Invalid { offset: pe, msg: "no resources".into() })?;
        let name_at = |id: u32| -> Result<Name> {
            if id & 0x8000_0000 == 0 {
                return Ok(Name::Id(id as u16));
            }
            let o = base + (id & 0x7fff_ffff) as usize;
            let n = u16_at(d, o)? as usize;
            let units: Vec<u16> = (0..n).map(|k| u16_at(d, o + 2 + 2 * k)).collect::<Result<_>>()?;
            Ok(Name::Text(String::from_utf16_lossy(&units)))
        };
        let entries = |o: usize| -> Result<Vec<(u32, u32)>> {
            let n = u16_at(d, base + o + 12)? as usize + u16_at(d, base + o + 14)? as usize;
            (0..n).map(|k| Ok((u32_at(d, base + o + 16 + 8 * k)?, u32_at(d, base + o + 20 + 8 * k)?))).collect()
        };
        let mut all = Vec::new();
        for (kind, o1) in entries(0)? {
            for (name, o2) in entries((o1 & 0x7fff_ffff) as usize)? {
                for (_, o3) in entries((o2 & 0x7fff_ffff) as usize)? {
                    let (rva, size) = (u32_at(d, base + o3 as usize)?, u32_at(d, base + o3 as usize + 4)? as usize);
                    let Some(o) = file_offset(rva) else { continue };
                    let data = d.get(o..o + size).ok_or(Error::Eof(o))?.to_vec();
                    all.push(Resource { kind: name_at(kind)?, name: name_at(name)?, data });
                }
            }
        }
        Ok(Self { all })
    }

    /// String `id` of the string table (`RT_STRING`), if present.
    pub fn string(&self, id: u16) -> Option<String> {
        let block = Name::Id(id / 16 + 1);
        let r = self.all.iter().find(|r| r.kind == Name::Id(6) && r.name == block)?;
        let mut o = 0;
        for k in 0..16 {
            let n = u16_at(&r.data, o).ok()? as usize;
            if k == id % 16 {
                let units: Vec<u16> = (0..n).filter_map(|j| u16_at(&r.data, o + 2 + 2 * j).ok()).collect();
                return Some(String::from_utf16_lossy(&units));
            }
            o += 2 + 2 * n;
        }
        None
    }

    /// The dialog template named `name` (`RT_DIALOG`), if present.
    pub fn dialog(&self, name: &str) -> Option<Dialog> {
        let r = self.all.iter().find(|r| r.kind == Name::Id(5) && r.name == Name::Text(name.into()))?;
        parse_dialog(&r.data).ok()
    }
}

/// A string or ordinal field of a dialog template; returns it and the next
/// offset.
fn sz_or_ord(d: &[u8], o: usize) -> Result<(Name, usize)> {
    match u16_at(d, o)? {
        0 => Ok((Name::Text(String::new()), o + 2)),
        0xffff => Ok((Name::Id(u16_at(d, o + 2)?), o + 4)),
        _ => {
            let mut e = o;
            let mut units = Vec::new();
            while u16_at(d, e)? != 0 {
                units.push(u16_at(d, e)?);
                e += 2;
            }
            Ok((Name::Text(String::from_utf16_lossy(&units)), e + 2))
        }
    }
}

fn text(n: Name) -> String {
    match n {
        Name::Text(s) => s,
        Name::Id(i) => format!("#{i}"),
    }
}

fn parse_dialog(d: &[u8]) -> Result<Dialog> {
    let style = u32_at(d, 0)?;
    let count = u16_at(d, 8)? as usize;
    let size = (u16_at(d, 14)? as i16, u16_at(d, 16)? as i16);
    let (_menu, o) = sz_or_ord(d, 18)?;
    let (class, o) = sz_or_ord(d, o)?;
    let (title, mut o) = sz_or_ord(d, o)?;
    if style & 0x40 != 0 {
        // DS_SETFONT: point size and face name
        o = sz_or_ord(d, o + 2)?.1;
    }
    let mut controls = Vec::new();
    for _ in 0..count {
        o = (o + 3) & !3;
        let style = u32_at(d, o)?;
        let rect = (u16_at(d, o + 8)? as i16, u16_at(d, o + 10)? as i16, u16_at(d, o + 12)? as i16, u16_at(d, o + 14)? as i16);
        let id = u16_at(d, o + 16)?;
        let (class, p) = sz_or_ord(d, o + 18)?;
        let (caption, p) = sz_or_ord(d, p)?;
        let extra = u16_at(d, p)? as usize;
        o = p + 2 + extra;
        controls.push(Control { id, class, text: text(caption), style, rect });
    }
    Ok(Dialog { title: text(title), class: text(class), size, controls })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the game's executable; runs only with `GBOH_EXE` set to it.
    #[test]
    fn reads_strings_and_dialogs() {
        let Ok(path) = std::env::var("GBOH_EXE") else { return };
        let r = Resources::parse(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(r.string(1).as_deref(), Some("The Great Battles of Alexander"));
        assert_eq!(r.string(97).as_deref(), Some("Issus"));
        let d = r.dialog("SCENARIO").unwrap();
        assert_eq!(d.title, "Choose Scenario");
        assert_eq!(d.size, (229, 243));
        assert!(d.controls.iter().any(|c| c.id == 1 && c.text == "OK"));
    }
}
