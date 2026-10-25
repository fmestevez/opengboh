//! Playing the game's Director movies (the intro and the historical
//! video, `gboh_formats::director`) as the Director 5 player would: each
//! frame's sprites composed on the stage in the movie's palette, at the
//! movie's frame rate (a negative tempo holds the frame that many seconds,
//! 🟡 as the scores read), transitions (wipes; the rest as dissolves) over
//! their durations, sounds started from the score's two sound channels and
//! by the frame scripts (`sound playFile`, `sound fadeOut`, `quit`). A key
//! or a click ends it, as the movies' own scripts do.

use gboh_formats::director::{Bitmap, Command, Movie};
use gboh_render::{Canvas, Sprite};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A transition under way: the old stage, the order its pixels give way
/// in (dissolves), its kind, start and length.
type Changing = (Vec<u8>, Vec<u32>, u8, Instant, Duration);

pub struct Slides {
    movie: Movie,
    data_dir: PathBuf,
    /// The frame shown (0-based) and when it ends.
    frame: usize,
    until: Instant,
    fps: u16,
    stage: Vec<u8>,
    palette: [[u8; 3]; 256],
    transition: Option<Changing>,
    bitmaps: HashMap<u16, Option<Bitmap>>,
    pub done: bool,
    started: bool,
}

impl Slides {
    /// The movie in projector `exe`, ready to play.
    pub fn open(exe: &Path, data_dir: &Path) -> Option<Self> {
        let movie = Movie::from_projector(std::fs::read(exe).ok()?).ok()?;
        let (w, h) = movie.stage;
        // the palette of the first frame that names one
        let palette = movie.frames.iter().find(|f| f.palette != 0).and_then(|f| movie.palette(f.palette)).unwrap_or([[0; 3]; 256]);
        Some(Self {
            fps: movie.frame_rate,
            movie,
            data_dir: data_dir.into(),
            frame: 0,
            until: Instant::now(),
            stage: vec![255; w * h],
            palette,
            transition: None,
            bitmaps: HashMap::new(),
            done: false,
            started: false,
        })
    }

    /// The movie's colours as a game palette (for the window while it
    /// plays).
    pub fn palette(&self, base: &gboh_formats::pal::Palette) -> gboh_formats::pal::Palette {
        let mut p = base.clone();
        p.colors = self.palette;
        p
    }

    fn bitmap(&mut self, member: u16) -> Option<&Bitmap> {
        let movie = &self.movie;
        self.bitmaps.entry(member).or_insert_with(|| movie.bitmap(member)).as_ref()
    }

    /// Compose frame `k` on the stage.
    fn compose(&mut self, k: usize) {
        let (w, h) = self.movie.stage;
        let mut stage = vec![255u8; w * h];
        let sprites = self.movie.frames[k].sprites.clone();
        for s in sprites {
            let Some(b) = self.bitmap(s.member).cloned() else { continue };
            let (x0, y0) = (s.loc.0 - b.reg.0, s.loc.1 - b.reg.1);
            // ink 1, transparent: the background colour (index 0) shows
            // through
            let transparent = s.ink & 0x3f == 1;
            for y in 0..b.height as i32 {
                let sy = y0 + y;
                if sy < 0 || sy >= h as i32 {
                    continue;
                }
                for x in 0..b.width as i32 {
                    let sx = x0 + x;
                    if sx < 0 || sx >= w as i32 {
                        continue;
                    }
                    let p = b.pixels[y as usize * b.width + x as usize];
                    if transparent && p == 0 {
                        continue;
                    }
                    stage[sy as usize * w + sx as usize] = p;
                }
            }
        }
        self.stage = stage;
    }

    /// Enter frame `k`: palette, picture, transition, sounds, timing.
    fn enter(&mut self, k: usize, sound: &mut crate::sound::Sound) {
        let f = self.movie.frames[k].clone();
        let previous = k.checked_sub(1).map(|p| self.movie.frames[p].clone()).unwrap_or_default();
        if f.palette != 0
            && let Some(p) = self.movie.palette(f.palette)
        {
            self.palette = p;
        }
        let old = self.stage.clone();
        self.compose(k);
        let mut hold = Duration::from_secs_f32(1.0 / self.fps.max(1) as f32);
        if f.transition != 0
            && let Some(t) = self.movie.transition(f.transition)
        {
            let n = self.stage.len() as u32;
            // a fixed shuffle of the pixels for dissolves
            let mut order: Vec<u32> = (0..n).collect();
            let mut seed = 0x1234_5678u32;
            for i in (1..order.len()).rev() {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                order.swap(i, (seed >> 8) as usize % (i + 1));
            }
            let length = Duration::from_millis(t.millis as u64);
            self.transition = Some((old, order, t.kind, Instant::now(), length));
            hold += length;
        }
        for ch in 0..2 {
            if f.sounds[ch] != 0
                && f.sounds[ch] != previous.sounds[ch]
                && let Some(s) = self.movie.sound(f.sounds[ch])
            {
                sound.channel_samples(ch + 1, s.rate, s.samples);
            }
        }
        if f.tempo > 0 {
            self.fps = f.tempo as u16;
            hold = hold.max(Duration::from_secs_f32(1.0 / self.fps as f32));
        } else if f.tempo < 0 {
            hold = hold.max(Duration::from_secs(-(f.tempo as i32) as u64));
        }
        self.until = Instant::now() + hold;
    }

    /// Leave frame `k`: its script's commands.
    fn leave(&mut self, k: usize, sound: &mut crate::sound::Sound) {
        let script = self.movie.frames[k].script;
        if script == 0 {
            return;
        }
        let commands = self.movie.scripts.iter().find(|(m, _)| *m == script).map(|(_, c)| c.clone()).unwrap_or_default();
        for c in commands {
            match c {
                Command::PlayFile { channel, file } => {
                    if let Some(p) = crate::find_ci(&self.data_dir, &file) {
                        sound.channel_file(channel as usize, &p);
                    }
                }
                Command::FadeOut { channel, ticks } => sound.channel_fade(channel as usize, ticks as f32 / 60.0),
                Command::Quit => self.done = true,
            }
        }
    }

    /// Play on; returns whether the picture changed.
    pub fn tick(&mut self, sound: &mut crate::sound::Sound) -> bool {
        if self.done {
            return false;
        }
        if !self.started {
            self.started = true;
            self.enter(0, sound);
            return true;
        }
        let mut changed = self.transition.is_some();
        if let Some((_, _, _, start, length)) = &self.transition
            && start.elapsed() >= *length
        {
            self.transition = None;
        }
        if Instant::now() >= self.until {
            self.leave(self.frame, sound);
            if self.done {
                return true;
            }
            self.frame += 1;
            if self.frame >= self.movie.frames.len() {
                self.done = true;
                return true;
            }
            self.enter(self.frame, sound);
            changed = true;
        }
        changed
    }

    /// The stage as it shows now (mid-transition, part old, part new), on
    /// a `width` × `height` overlay, centred. Index 0 (see-through in the
    /// overlay) is drawn as the palette's nearest other colour.
    pub fn canvas(&self, width: usize, height: usize) -> Canvas {
        let (w, h) = self.movie.stage;
        let mut shown = self.stage.clone();
        if let Some((old, order, kind, start, length)) = &self.transition {
            let t = (start.elapsed().as_secs_f32() / length.as_secs_f32().max(0.001)).clamp(0.0, 1.0);
            let mut from = old.clone();
            match kind {
                // wipes: the new picture comes in from one side
                1..=4 => {
                    for y in 0..h {
                        for x in 0..w {
                            let new = match kind {
                                1 => (x as f32) < t * w as f32,
                                2 => (x as f32) >= (1.0 - t) * w as f32,
                                3 => (y as f32) < t * h as f32,
                                _ => (y as f32) >= (1.0 - t) * h as f32,
                            };
                            if new {
                                from[y * w + x] = shown[y * w + x];
                            }
                        }
                    }
                }
                _ => {
                    let n = (t * order.len() as f32) as usize;
                    for &i in &order[..n.min(order.len())] {
                        from[i as usize] = shown[i as usize];
                    }
                }
            }
            shown = from;
        }
        let white = (1..=255usize)
            .min_by_key(|&i| self.palette[i].iter().zip(self.palette[0]).map(|(&a, b)| (a as i32 - b as i32).pow(2)).sum::<i32>())
            .unwrap_or(1) as u8;
        let black = (1..=255usize).min_by_key(|&i| self.palette[i].iter().map(|&c| c as i32).sum::<i32>()).unwrap_or(255) as u8;
        let mut canvas = Canvas::new(width, height, black);
        let pixels = shown.into_iter().map(|p| if p == 0 { white } else { p }).collect();
        canvas.blit(&Sprite { width: w, height: h, pixels }, (width as i32 - w as i32) / 2, (height as i32 - h as i32) / 2);
        canvas
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Composes frames of the intro and the historical video; runs only
    /// with `GBOH_DATA` set (and writes them to `GBOH_SHOTS`, if set).
    #[test]
    fn composes_the_movies() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let dir = Path::new(&dir);
        let base = gboh_formats::pal::Palette::parse(&std::fs::read(dir.join("GAME.PAL")).unwrap()).unwrap();
        for (exe, picks) in [("INTRO.EXE", [5, 20, 40]), ("HISTORY.EXE", [100, 600, 1100])] {
            let mut s = Slides::open(&dir.join(exe), dir).unwrap();
            for k in picks {
                for f in 0..=k {
                    if let Some(p) = s.movie.frames[f].palette.checked_sub(0).filter(|&p| p != 0).and_then(|p| s.movie.palette(p)) {
                        s.palette = p;
                    }
                }
                s.compose(k);
                assert!(s.stage.iter().any(|&p| p != s.stage[0]), "{exe} frame {k} is blank");
                if let Ok(out) = std::env::var("GBOH_SHOTS") {
                    let c = s.canvas(640, 480);
                    crate::ui::write_ppm(&c, &s.palette(&base), &Path::new(&out).join(format!("{}_{k}.ppm", exe.trim_end_matches(".EXE")))).unwrap();
                }
            }
        }
    }
}
