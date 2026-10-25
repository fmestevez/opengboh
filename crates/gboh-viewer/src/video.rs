//! The campaign's victory video (`320ALEX.AVI`: Indeo 3.2, 320 × 200 at
//! 15 frames a second, 8-bit 11 kHz sound), which the original plays
//! through Video for Windows in its `VIDEODLG` (`FUN_005a3914`: a 320 × 200
//! window at (32, 10)). The rewrite has no Indeo decoder of its own: it
//! asks `ffmpeg` (`GBOH_FFMPEG`, or on the path) for the frames and the
//! sound, and does without the video if there is none.

use gboh_formats::pal::Palette;
use gboh_render::Sprite;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 200;
/// Seconds a frame.
const FRAME: f32 = 1.0 / 15.0;

pub struct Video {
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
}

fn ffmpeg() -> std::ffi::OsString {
    std::env::var_os("GBOH_FFMPEG").unwrap_or_else(|| "ffmpeg".into())
}

impl Video {
    /// Start decoding `path`; `None` without `ffmpeg`.
    pub fn open(path: &Path, pal: &Palette) -> Option<Self> {
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
            let mut frame = vec![0u8; WIDTH * HEIGHT * 3];
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
        Some(Self { frames, shown: None, number: 0, start: Instant::now(), done: false, sound, lookup })
    }

    /// Bring the frame shown up to date; returns whether it changed.
    pub fn advance(&mut self) -> bool {
        let due = (self.start.elapsed().as_secs_f32() / FRAME) as usize;
        let mut changed = false;
        while self.number <= due {
            match self.frames.try_recv() {
                Ok(rgb) => {
                    let pixels = rgb.chunks(3).map(|p| self.lookup[((p[0] as usize >> 3) << 10) | ((p[1] as usize >> 3) << 5) | (p[2] as usize >> 3)]).collect();
                    self.shown = Some(Sprite { width: WIDTH, height: HEIGHT, pixels });
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
        assert_eq!(f.pixels.len(), WIDTH * HEIGHT);
        assert!(f.pixels.iter().any(|&p| p != f.pixels[0]), "a picture, not a blank");
    }
}
