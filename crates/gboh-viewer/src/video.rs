//! The games' AVI videos: Alexander's campaign victory video
//! (`320ALEX.AVI`: Indeo 3.2, 320 × 200 at 15 frames a second, 8-bit 11 kHz
//! sound), which the original plays through Video for Windows in its
//! `VIDEODLG` (`FUN_005a3914`: a 320 × 200 window at (32, 10)), and
//! Hannibal's and Caesar's intros (`INTRO.AVI`, 512 × 384). The rewrite has
//! no Indeo decoder of its own: it asks `ffmpeg` (`GBOH_FFMPEG`, or on the
//! path; `ffprobe` beside it for the size) for the frames and the sound,
//! and does without the video if there is none.

use gboh_formats::pal::Palette;
use gboh_render::Sprite;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

/// The size assumed when `ffprobe` can't tell (the victory video's).
const WIDTH: usize = 320;
const HEIGHT: usize = 200;
/// Seconds a frame.
const FRAME: f32 = 1.0 / 15.0;

pub struct Video {
    pub width: usize,
    pub height: usize,
    frames: Receiver<Vec<u8>>,
    /// The frame shown, in the palette's colours, and its number.
    pub shown: Option<Sprite>,
    number: usize,
    start: Instant,
    pub done: bool,
    /// The sound, as a WAV file in memory.
    pub sound: Option<Vec<u8>>,
    /// Palette index for each 5-bit-per-channel colour.
    lookup: Vec<u8>,
    /// Shown in a colour cube of its own (dithered) rather than the game's
    /// palette: the palette to show it in.
    pub palette: Option<Palette>,
}

/// The colour cube's levels: red, green, blue (6 × 7 × 6 = 252 colours, from
/// index 1).
const CUBE: [usize; 3] = [6, 7, 6];

/// A 4 × 4 ordered dither, in sixteenths.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// The colour cube as a palette (index 0 black).
fn cube_palette(base: &Palette) -> Palette {
    let mut p = base.clone();
    p.colors = [[0; 3]; 256];
    for r in 0..CUBE[0] {
        for g in 0..CUBE[1] {
            for b in 0..CUBE[2] {
                let level = |v: usize, n: usize| (v * 255 / (n - 1)) as u8;
                p.colors[1 + (r * CUBE[1] + g) * CUBE[2] + b] = [level(r, CUBE[0]), level(g, CUBE[1]), level(b, CUBE[2])];
            }
        }
    }
    p
}

/// A colour's cube index at pixel `(x, y)`, dithered.
fn cube_index(rgb: &[u8], x: usize, y: usize) -> u8 {
    let d = BAYER[y & 3][x & 3] as usize;
    let level = |v: u8, n: usize| ((v as usize * (n - 1) * 16 + d * 255) / (255 * 16)).min(n - 1);
    let (r, g, b) = (level(rgb[0], CUBE[0]), level(rgb[1], CUBE[1]), level(rgb[2], CUBE[2]));
    (1 + (r * CUBE[1] + g) * CUBE[2] + b) as u8
}

fn ffmpeg() -> std::ffi::OsString {
    std::env::var_os("GBOH_FFMPEG").unwrap_or_else(|| "ffmpeg".into())
}

/// The video's frame size, from `ffprobe` (beside `ffmpeg`).
fn size(path: &Path) -> Option<(usize, usize)> {
    let ffmpeg = std::path::PathBuf::from(ffmpeg());
    let probe = ffmpeg.with_file_name(ffmpeg.file_name()?.to_string_lossy().replace("ffmpeg", "ffprobe"));
    let out = Command::new(probe)
        .args(["-v", "quiet", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let (w, h) = text.trim().split_once(',')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

impl Video {
    /// Start decoding `path`; `None` without `ffmpeg`.
    /// Start decoding `path` to show in the game's palette `pal`; `None`
    /// without `ffmpeg`.
    pub fn open(path: &Path, pal: &Palette) -> Option<Self> {
        Self::start(path, pal, false)
    }

    /// Start decoding `path` to show in a colour cube of its own (see
    /// [`Video::palette`]).
    pub fn open_true_colour(path: &Path, base: &Palette) -> Option<Self> {
        Self::start(path, base, true)
    }

    fn start(path: &Path, pal: &Palette, cube: bool) -> Option<Self> {
        let (width, height) = size(path).unwrap_or((WIDTH, HEIGHT));
        let mut child = Command::new(ffmpeg())
            .args(["-loglevel", "quiet", "-i"])
            .arg(path)
            .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut out = child.stdout.take()?;
        let (tx, frames) = channel();
        std::thread::spawn(move || {
            let mut frame = vec![0u8; width * height * 3];
            while out.read_exact(&mut frame).is_ok() {
                if tx.send(frame.clone()).is_err() {
                    break;
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        });
        let sound = Command::new(ffmpeg())
            .args(["-loglevel", "quiet", "-i"])
            .arg(path)
            .args(["-vn", "-f", "wav", "-"])
            .stderr(Stdio::null())
            .output()
            .ok()
            .map(|o| o.stdout)
            .filter(|w| w.len() > 44);
        // nearest palette colour (of the game's, leaving out index 0, which
        // the overlay leaves see-through) for each 15-bit colour
        let lookup = (0..1 << 15)
            .map(|c: usize| {
                let (r, g, b) = (((c >> 10) & 31) * 255 / 31, ((c >> 5) & 31) * 255 / 31, (c & 31) * 255 / 31);
                (1..=255u8)
                    .min_by_key(|&i| {
                        let p = pal.colors[i as usize];
                        (p[0] as i32 - r as i32).pow(2) + (p[1] as i32 - g as i32).pow(2) + (p[2] as i32 - b as i32).pow(2)
                    })
                    .unwrap_or(1)
            })
            .collect();
        let palette = cube.then(|| cube_palette(pal));
        Some(Self { width, height, frames, shown: None, number: 0, start: Instant::now(), done: false, sound, lookup, palette })
    }

    /// The palette index nearest black (for the margins round the frame).
    pub fn black(&self) -> u8 {
        if self.palette.is_some() { 1 } else { self.lookup[0] }
    }

    /// Bring the frame shown up to date; returns whether it changed.
    pub fn advance(&mut self) -> bool {
        let due = (self.start.elapsed().as_secs_f32() / FRAME) as usize;
        let mut changed = false;
        while self.number <= due {
            match self.frames.try_recv() {
                Ok(rgb) => {
                    let w = self.width;
                    let pixels = if self.palette.is_some() {
                        rgb.chunks(3).enumerate().map(|(i, p)| cube_index(p, i % w, i / w)).collect()
                    } else {
                        rgb.chunks(3).map(|p| self.lookup[((p[0] as usize >> 3) << 10) | ((p[1] as usize >> 3) << 5) | (p[2] as usize >> 3)]).collect()
                    };
                    self.shown = Some(Sprite { width: self.width, height: self.height, pixels });
                    self.number += 1;
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.done = true;
                    break;
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodes the first frames of the victory video; runs only with
    /// `GBOH_DATA` set and `ffmpeg` at hand.
    #[test]
    fn plays_the_video() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = Path::new(&dir);
        let pal = Palette::parse(&std::fs::read(dir.join("GAME.PAL")).unwrap()).unwrap();
        let Some(mut v) = Video::open(&dir.join("320ALEX.AVI"), &pal) else { return };
        assert!(v.sound.is_some());
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert!(v.advance());
        let f = v.shown.as_ref().unwrap();
        assert_eq!((v.width, v.height), (WIDTH, HEIGHT));
        assert_eq!(f.pixels.len(), WIDTH * HEIGHT);
        assert!(f.pixels.iter().any(|&p| p != f.pixels[0]), "a picture, not a blank");
    }
}
