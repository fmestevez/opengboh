//! Macromedia Director 5 movies, as the game's intro and historical video
//! carry them (`INTRO.EXE`, `HISTORY.EXE`: Director projectors with the
//! movie inside), read far enough to play them: bitmaps, palettes, sounds,
//! the score and the few Lingo commands their frame scripts use.
//!
//! ```text
//! projector   := … "PJ95" header (offset in the file's last 4 bytes):
//!                "PJ95", offset of the application file
//! RIFX file   := "XFIR" size kind, then chunks "fourcc size data"
//!                (fourccs stored reversed, sizes little-endian); its
//!                "imap" gives the offset of its "mmap": a table of
//!                (fourcc, length, offset) for every chunk. The
//!                application ("APPL") holds the movie ("MV93") as its
//!                "File" chunk. Chunk contents are big-endian.
//! KEY*        := (chunk index, owner index, fourcc): the resources of a
//!                cast member (its BITD, CLUT, snd)
//! CAS*        := chunk index of cast member 1, 2, …
//! CASt        := type u32 (1 bitmap, 4 palette, 6 sound, 11 script,
//!                14 transition), info length u32, data length u32, info,
//!                data. A bitmap's data: pitch u16 (& 0x0fff), rect
//!                (top, left, bottom, right), bounding rect, registration
//!                point (y, x), 0, bits per pixel, palette (cast library,
//!                member). A transition's: chunk size u16, type u8, flags
//!                u8, duration u16 (ms).
//! BITD        := rows of `pitch` bytes, run-length coded unless raw size:
//!                n < 0x80: n + 1 bytes follow; else the next byte
//!                257 − n times
//! CLUT        := 256 × (red, green, blue) u16
//! snd         := a Mac sound resource (format 1 or 2) with one buffer
//!                command pointing at a sound header: standard (8-bit
//!                unsigned samples at +22) or extended (encoding 0xFF:
//!                frames at +22, sample size at +48, samples at +64)
//! VWCF        := configuration: frame rate u16 at 54
//! VWSC        := score: total u32, frames offset u32, frame count u32,
//!                version u16, channel size u16 (24), channels u16, then
//!                frames: length u16 and (length u16, offset u16, bytes)
//!                changes to a channel buffer: two main channels (frame
//!                script, sound 1, sound 2, transition as (library,
//!                member) at 0, 4, 8, 12; tempo i8 at 21; palette at 24),
//!                then sprites: type u8, ink u8, member (library i16,
//!                member u16), script, colours, location y, x, height,
//!                width (i16)
//! Lscr        := a script: its cast member u16 at 0x2e; handlers (count
//!                u16 at 0x48, offset u32 at 0x4a; 46 bytes each: name,
//!                vector, code length u32, code offset u32), literals
//!                (count u16 at 0x4e, offset u32 at 0x50, data at the
//!                offset u32 at 0x58); names from Lnam
//! ```

use crate::{Error, Result};

fn be16(d: &[u8], o: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(d.get(o..o + 2).ok_or(Error::Eof(o))?.try_into().unwrap()))
}

fn be32(d: &[u8], o: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(d.get(o..o + 4).ok_or(Error::Eof(o))?.try_into().unwrap()))
}

fn le32(d: &[u8], o: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(d.get(o..o + 4).ok_or(Error::Eof(o))?.try_into().unwrap()))
}

fn le16(d: &[u8], o: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(d.get(o..o + 2).ok_or(Error::Eof(o))?.try_into().unwrap()))
}

fn fourcc(d: &[u8], o: usize) -> Result<[u8; 4]> {
    let mut t: [u8; 4] = d.get(o..o + 4).ok_or(Error::Eof(o))?.try_into().unwrap();
    t.reverse();
    Ok(t)
}

/// A RIFX file's chunks: fourcc, offset of the data, length.
fn chunks(d: &[u8], base: usize) -> Result<Vec<([u8; 4], usize, usize)>> {
    if d.get(base..base + 4) != Some(b"XFIR") {
        return Err(Error::Invalid { offset: base, msg: "not a Director file".into() });
    }
    let imap = base + 12;
    let mmap = le32(d, imap + 12)? as usize;
    let mmap = if d.get(mmap..mmap + 4) == Some(b"pamm") { mmap } else { base + mmap };
    let header = le16(d, mmap + 8)? as usize;
    let entry = le16(d, mmap + 10)? as usize;
    let used = le32(d, mmap + 16)? as usize;
    (0..used)
        .map(|i| {
            let e = mmap + 8 + header + i * entry;
            Ok((fourcc(d, e)?, le32(d, e + 8)? as usize + 8, le32(d, e + 4)? as usize))
        })
        .collect()
}

/// A bitmap cast member, decoded: 8-bit pixels, size, where its
/// registration point is in its own pixels, and its palette member.
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    pub reg: (i32, i32),
    pub palette: Option<u16>,
}

/// A transition: its type (1–4 wipes right, left, down, up; others
/// dissolves) and duration in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub kind: u8,
    pub millis: u16,
}

/// A sound: mono samples and their rate.
#[derive(Debug, Clone)]
pub struct Sound {
    pub rate: u32,
    pub samples: Vec<i16>,
}

/// What a frame script does on leaving its frame (the commands the game's
/// movies use).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `sound playFile channel, "file"`
    PlayFile { channel: u16, file: String },
    /// `sound fadeOut channel, ticks`
    FadeOut { channel: u16, ticks: u16 },
    Quit,
}

/// A sprite of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sprite {
    pub member: u16,
    pub ink: u8,
    /// Where its registration point goes on the stage.
    pub loc: (i32, i32),
}

/// A frame of the score.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frame {
    pub script: u16,
    pub sounds: [u16; 2],
    pub transition: u16,
    pub tempo: i8,
    pub palette: u16,
    pub sprites: Vec<Sprite>,
}

/// A movie.
pub struct Movie {
    data: Vec<u8>,
    chunks: Vec<([u8; 4], usize, usize)>,
    /// Chunk index of each cast member (1-based: `cast[0]` is member 1).
    cast: Vec<u32>,
    keys: Vec<(u32, u32, [u8; 4])>,
    pub frame_rate: u16,
    pub stage: (usize, usize),
    pub frames: Vec<Frame>,
    /// Frame scripts' commands by cast member.
    pub scripts: Vec<(u16, Vec<Command>)>,
}

impl Movie {
    /// The movie inside a Director 5 projector.
    pub fn from_projector(data: Vec<u8>) -> Result<Self> {
        let n = data.len();
        let header = le32(&data, n.checked_sub(4).ok_or(Error::Eof(0))?)? as usize;
        if data.get(header..header + 4) != Some(b"59JP") && data.get(header..header + 4) != Some(b"PJ95") {
            return Err(Error::Invalid { offset: header, msg: "not a Director 5 projector".into() });
        }
        let app = le32(&data, header + 4)? as usize;
        let file = chunks(&data, app)?
            .into_iter()
            .find(|(t, _, _)| t == b"File")
            .ok_or(Error::Invalid { offset: app, msg: "no movie in the projector".into() })?;
        // the File chunk is the movie's own RIFX
        Self::parse(data, file.1 - 8)
    }

    fn parse(data: Vec<u8>, base: usize) -> Result<Self> {
        let chunks = chunks(&data, base)?;
        let find = |t: &[u8; 4]| chunks.iter().find(|c| &c.0 == t).map(|&(_, o, l)| (o, l));
        let bytes = |t: &[u8; 4]| find(t).and_then(|(o, l)| data.get(o..o + l));
        let cast = bytes(b"CAS*").map(|c| c.chunks(4).map(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]])).collect()).unwrap_or_default();
        let keys = match bytes(b"KEY*") {
            Some(k) => {
                let used = u32::from_le_bytes(k[8..12].try_into().unwrap()) as usize;
                (0..used)
                    .filter_map(|i| {
                        let o = 12 + 12 * i;
                        let mut t: [u8; 4] = k.get(o + 8..o + 12)?.try_into().ok()?;
                        t.reverse();
                        Some((u32::from_le_bytes(k[o..o + 4].try_into().ok()?), u32::from_le_bytes(k[o + 4..o + 8].try_into().ok()?), t))
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        let config = bytes(b"VWCF").unwrap_or_default();
        let frame_rate = be16(config, 54).unwrap_or(15).max(1);
        let stage = match (be16(config, 4), be16(config, 6), be16(config, 8), be16(config, 10)) {
            (Ok(t), Ok(l), Ok(b), Ok(r)) => ((r as i16 - l as i16) as usize, (b as i16 - t as i16) as usize),
            _ => (640, 480),
        };
        let frames = bytes(b"VWSC").map(score).transpose()?.unwrap_or_default();
        let names = bytes(b"Lnam").map(lingo_names).unwrap_or_default();
        let scripts = chunks
            .iter()
            .filter(|c| &c.0 == b"Lscr")
            .filter_map(|&(_, o, l)| data.get(o..o + l))
            .filter_map(|s| script(s, &names).ok())
            .collect();
        Ok(Self { data, chunks, cast, keys, frame_rate, stage, frames, scripts })
    }

    /// Cast member `member`'s CASt: type, info, data.
    fn member(&self, member: u16) -> Option<(u32, &[u8], &[u8])> {
        let idx = *self.cast.get((member as usize).checked_sub(1)?)? as usize;
        if idx == 0 {
            return None;
        }
        let &(_, o, l) = self.chunks.get(idx)?;
        let c = self.data.get(o..o + l)?;
        let (t, il, dl) = (be32(c, 0).ok()?, be32(c, 4).ok()? as usize, be32(c, 8).ok()? as usize);
        Some((t, c.get(12..12 + il)?, c.get(12 + il..12 + il + dl)?))
    }

    /// The resource of type `t` that cast member `member` owns.
    fn resource(&self, member: u16, t: &[u8; 4]) -> Option<&[u8]> {
        let owner = *self.cast.get((member as usize).checked_sub(1)?)?;
        let &(sec, _, _) = self.keys.iter().find(|&&(_, o, kt)| o == owner && &kt == t)?;
        let &(_, o, l) = self.chunks.get(sec as usize)?;
        self.data.get(o..o + l)
    }

    pub fn bitmap(&self, member: u16) -> Option<Bitmap> {
        let (t, _, spec) = self.member(member)?;
        if t != 1 {
            return None;
        }
        let pitch = (be16(spec, 0).ok()? & 0x0fff) as usize;
        let r = |o| be16(spec, o).map(|v| v as i16 as i32).ok();
        let (top, left, bottom, right) = (r(2)?, r(4)?, r(6)?, r(8)?);
        let (reg_y, reg_x) = (r(18)?, r(20)?);
        let bits = spec.get(23).copied().unwrap_or(1);
        let palette = be16(spec, 26).ok().filter(|&p| p > 0 && p < 0x8000);
        let (width, height) = ((right - left).max(0) as usize, (bottom - top).max(0) as usize);
        if bits != 8 || pitch < width {
            return None;
        }
        let raw = self.resource(member, b"BITD")?;
        let rows = if raw.len() == pitch * height {
            raw.to_vec()
        } else {
            let mut out = Vec::with_capacity(pitch * height);
            let mut i = 0;
            while i < raw.len() && out.len() < pitch * height {
                let n = raw[i] as usize;
                i += 1;
                if n < 0x80 {
                    out.extend_from_slice(raw.get(i..i + n + 1).unwrap_or_default());
                    i += n + 1;
                } else {
                    let b = raw.get(i).copied().unwrap_or(0);
                    out.extend(std::iter::repeat_n(b, 257 - n));
                    i += 1;
                }
            }
            out.resize(pitch * height, 0);
            out
        };
        let pixels = (0..height).flat_map(|y| rows[y * pitch..y * pitch + width].to_vec()).collect();
        Some(Bitmap { width, height, pixels, reg: (reg_x - left, reg_y - top), palette })
    }

    /// A palette member's colours.
    pub fn palette(&self, member: u16) -> Option<[[u8; 3]; 256]> {
        let c = self.resource(member, b"CLUT")?;
        let mut out = [[0; 3]; 256];
        for (k, e) in out.iter_mut().enumerate() {
            *e = [*c.get(6 * k)?, *c.get(6 * k + 2)?, *c.get(6 * k + 4)?];
        }
        Some(out)
    }

    pub fn transition(&self, member: u16) -> Option<Transition> {
        let (t, _, spec) = self.member(member)?;
        (t == 14).then(|| Transition { kind: spec.get(2).copied().unwrap_or(0), millis: be16(spec, 4).unwrap_or(1000) })
    }

    pub fn sound(&self, member: u16) -> Option<Sound> {
        let s = self.resource(member, b"snd ")?;
        let format = be16(s, 0).ok()?;
        let mut p = 2;
        if format == 1 {
            let mods = be16(s, p).ok()? as usize;
            p += 2 + 6 * mods;
        } else {
            p += 2;
        }
        let commands = be16(s, p).ok()?;
        if commands == 0 {
            return None;
        }
        let h = be32(s, p + 2 + 4).ok()? as usize;
        let rate = be32(s, h + 8).ok()? >> 16;
        let encoding = *s.get(h + 20)?;
        let samples = if encoding == 0xff {
            let frames = be32(s, h + 22).ok()? as usize;
            let channels = be32(s, h + 4).ok()?.max(1) as usize;
            let bits = be16(s, h + 48).ok()?;
            let data = s.get(h + 64..)?;
            if bits == 16 {
                data.chunks(2 * channels).take(frames).map(|c| i16::from_be_bytes([c[0], c.get(1).copied().unwrap_or(0)])).collect()
            } else {
                data.iter().step_by(channels).take(frames).map(|&b| ((b as i16) - 128) << 8).collect()
            }
        } else {
            let length = be32(s, h + 4).ok()? as usize;
            s.get(h + 22..h + 22 + length)?.iter().map(|&b| ((b as i16) - 128) << 8).collect()
        };
        Some(Sound { rate, samples })
    }
}

/// The score's frames.
fn score(s: &[u8]) -> Result<Vec<Frame>> {
    let total = be32(s, 0)? as usize;
    let offset = be32(s, 4)? as usize;
    let size = be16(s, 14)? as usize;
    let channels = be16(s, 16)? as usize;
    let mut buf = vec![0u8; size * (channels + 2)];
    let mut frames = Vec::new();
    let mut o = offset;
    while o + 2 <= total.min(s.len()) {
        let len = be16(s, o)? as usize;
        if len < 2 {
            break;
        }
        let end = o + len;
        let mut p = o + 2;
        while p + 4 <= end {
            let (n, at) = (be16(s, p)? as usize, be16(s, p + 2)? as usize);
            let bytes = s.get(p + 4..p + 4 + n).ok_or(Error::Eof(p))?;
            if let Some(dst) = buf.get_mut(at..at + n) {
                dst.copy_from_slice(bytes);
            }
            p += 4 + n;
        }
        let m = |k: usize| be16(&buf, k).unwrap_or(0);
        let sprites = (0..channels)
            .filter_map(|c| {
                let b = &buf[(c + 2) * size..(c + 3) * size];
                let member = be16(b, 4).ok()?;
                (b[0] != 0 && member != 0).then(|| Sprite {
                    member,
                    ink: b[1],
                    loc: (be16(b, 14).unwrap_or(0) as i16 as i32, be16(b, 12).unwrap_or(0) as i16 as i32),
                })
            })
            .collect();
        frames.push(Frame { script: m(2), sounds: [m(6), m(10)], transition: m(14), tempo: buf[21] as i8, palette: m(26), sprites });
        o = end;
    }
    Ok(frames)
}

/// The names of `Lnam`.
fn lingo_names(l: &[u8]) -> Vec<String> {
    let (Ok(off), Ok(count)) = (be16(l, 16), be16(l, 18)) else { return Vec::new() };
    let mut p = off as usize;
    let mut out = Vec::new();
    for _ in 0..count {
        let Some(&n) = l.get(p) else { break };
        out.push(String::from_utf8_lossy(l.get(p + 1..p + 1 + n as usize).unwrap_or_default()).into_owned());
        p += 1 + n as usize;
    }
    out
}

/// A script's `exitFrame` commands, run on a small stack machine that
/// knows the opcodes the game's frame scripts use: push integer (0x41,
/// 0x81), constant (0x44), symbol (0x45), argument lists (0x42, 0x43) and
/// calls (0x57) of `sound` and `quit`.
fn script(s: &[u8], names: &[String]) -> Result<(u16, Vec<Command>)> {
    let member = be16(s, 0x2e)?;
    let (hc, ho) = (be16(s, 0x48)? as usize, be32(s, 0x4a)? as usize);
    let (lo, ld) = (be32(s, 0x50)? as usize, be32(s, 0x58)? as usize);
    let literal = |k: usize| -> Option<String> {
        let (t, o) = (be32(s, lo + 8 * k).ok()?, be32(s, lo + 8 * k + 4).ok()? as usize);
        (t == 1).then(|| {
            let n = be32(s, ld + o).unwrap_or(0) as usize;
            String::from_utf8_lossy(s.get(ld + o + 4..ld + o + 4 + n).unwrap_or_default()).trim_end_matches('\0').to_string()
        })
    };
    let mut commands = Vec::new();
    for h in 0..hc {
        let r = ho + 46 * h;
        let name = names.get(be16(s, r)? as usize).map(String::as_str);
        if name != Some("exitFrame") {
            continue;
        }
        let (len, off) = (be32(s, r + 4)? as usize, be32(s, r + 8)? as usize);
        let code = s.get(off..off + len).ok_or(Error::Eof(off))?;
        #[derive(Clone)]
        enum V {
            Int(i32),
            Str(String),
            Sym(String),
            List(Vec<V>),
        }
        let mut stack: Vec<V> = Vec::new();
        let mut i = 0;
        while i < code.len() {
            let op = code[i];
            let (arg, step) = match op {
                0x00..=0x3f => (0, 1),
                0x40..=0x7f => (*code.get(i + 1).unwrap_or(&0) as usize, 2),
                0x80..=0xbf => (be16(code, i + 1).unwrap_or(0) as usize, 3),
                _ => (0, 4),
            };
            i += step;
            match op & 0x3f {
                0x01 if op < 0x40 => break,
                0x01 => stack.push(V::Int(if op >= 0x80 { arg as i16 as i32 } else { arg as i8 as i32 })),
                0x04 if op >= 0x40 => stack.push(literal(arg).map_or(V::Int(0), V::Str)),
                0x05 if op >= 0x40 => stack.push(V::Sym(names.get(arg).cloned().unwrap_or_default())),
                0x02 | 0x03 if op >= 0x40 => {
                    let at = stack.len().saturating_sub(arg);
                    let args = stack.split_off(at);
                    stack.push(V::List(args));
                }
                0x17 if op >= 0x40 => {
                    let args = match stack.pop() {
                        Some(V::List(a)) => a,
                        _ => Vec::new(),
                    };
                    match (names.get(arg).map(String::as_str), args.as_slice()) {
                        (Some("quit"), _) => commands.push(Command::Quit),
                        (Some("sound"), [V::Sym(what), V::Int(ch), V::Str(file)]) if what.eq_ignore_ascii_case("playFile") => {
                            commands.push(Command::PlayFile { channel: *ch as u16, file: file.clone() })
                        }
                        (Some("sound"), [V::Sym(what), V::Int(ch), V::Int(ticks)]) if what.eq_ignore_ascii_case("fadeOut") => {
                            commands.push(Command::FadeOut { channel: *ch as u16, ticks: *ticks as u16 })
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
    Ok((member, commands))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the intro and the historical video; runs only with
    /// `GBOH_DATA` set.
    #[test]
    fn reads_the_movies() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = std::path::Path::new(&dir);
        let intro = Movie::from_projector(std::fs::read(dir.join("INTRO.EXE")).unwrap()).unwrap();
        assert_eq!((intro.frame_rate, intro.stage, intro.frames.len()), (15, (640, 480), 73));
        let back = intro.bitmap(4).unwrap();
        assert_eq!((back.width, back.height, back.reg), (640, 480, (320, 240)));
        assert!(intro.palette(2).is_some());
        assert_eq!(intro.sound(47).map(|s| (s.rate, s.samples.len())), Some((22050, 661503)));
        assert!(intro.scripts.iter().any(|(m, c)| *m == 46 && c == &[Command::PlayFile { channel: 3, file: "intro.wav".into() }]));
        assert!(intro.scripts.iter().any(|(m, c)| *m == 50 && c == &[Command::Quit]));
        assert_eq!(intro.frames[3].transition, 3);
        assert_eq!(intro.transition(3), Some(Transition { kind: 0x33, millis: 0x226 }));
        assert_eq!(intro.frames[4].tempo, -2);
        let history = Movie::from_projector(std::fs::read(dir.join("HISTORY.EXE")).unwrap()).unwrap();
        assert_eq!(history.frames.len(), 1340);
        assert!(history.scripts.iter().any(|(_, c)| c.iter().any(|x| matches!(x, Command::PlayFile { file, .. } if file == "history.wav"))));
    }
}
