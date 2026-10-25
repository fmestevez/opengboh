//! Scenario packages (`.psf`), the *Collector's Edition* editor's way of
//! passing a scenario around: its map, scenario and two AI plans in one
//! file. A u8 1 and a u8 0, an i32 index of the last file, then each
//! file's name (u8 length and bytes) and u32 size, then the files' bytes
//! in that order. The game itself doesn't read them.

use crate::read::Reader;
use crate::{Error, Result};

/// A package of `files` (name, bytes).
pub fn pack(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![1, 0];
    out.extend((files.len() as i32 - 1).to_le_bytes());
    for (name, data) in files {
        out.push(name.len().min(255) as u8);
        out.extend(name.bytes().take(255));
        out.extend((data.len() as u32).to_le_bytes());
    }
    for (_, data) in files {
        out.extend(data);
    }
    out
}

/// The files (name, bytes) of a package.
pub fn unpack(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let mut r = Reader::new(data);
    if r.bytes(2)? != [1, 0] {
        return Err(Error::Invalid { offset: 0, msg: "not a scenario package".into() });
    }
    let last = r.u32()? as i32;
    if !(0..64).contains(&last) {
        return Err(Error::Invalid { offset: 2, msg: format!("{} files", last + 1) });
    }
    let mut heads = Vec::new();
    for _ in 0..=last {
        let len = r.bytes(1)?[0] as usize;
        let name = r.name(len)?;
        heads.push((name, r.u32()? as usize));
    }
    heads.into_iter().map(|(name, size)| Ok((name, r.bytes(size)?.to_vec()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_and_unpacks() {
        let files = vec![("A.map".to_string(), vec![1, 2, 3]), ("A.scn".to_string(), vec![]), ("A_H1.AI".to_string(), b"x".to_vec())];
        let data = pack(&files);
        assert_eq!(&data[..6], &[1, 0, 2, 0, 0, 0]);
        assert_eq!(unpack(&data).unwrap(), files);
        assert!(unpack(&data[..data.len() - 1]).is_err());
    }

    /// The editor's own package unpacks and packs back byte for byte;
    /// runs only with `GBOH_CAESAR`.
    #[test]
    fn the_editors_package_round_trips() {
        let Ok(dir) = std::env::var("GBOH_CAESAR") else { return };
        let data = std::fs::read(std::path::Path::new(&dir).join("Last.psf")).unwrap();
        let files = unpack(&data).unwrap();
        assert_eq!(files.len(), 4);
        assert!(crate::scn::Scenario::parse(&files[1].1).is_ok());
        assert_eq!(pack(&files), data);
    }
}
