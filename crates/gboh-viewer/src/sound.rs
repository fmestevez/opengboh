//! Battle sounds from the original's WAV files in `DATA`.
//!
//! The file names and their groups follow the original's tables (see
//! `docs/formats.md`, "Sounds"); which group a unit type uses is chosen by
//! the sounds' names (🟡 the original picks the movement group in its
//! presentation code, not traced).

use gboh_game::missile::Weapon;
use gboh_game::movement::MoveClass;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Something worth a sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Select,
    Move(MoveClass),
    Volley(Weapon, bool),
    Shock { elephants: bool },
    Start,
    End { won: bool },
}

/// Movement sounds by unit class (groups of the original's table).
fn movement(class: MoveClass) -> &'static [&'static str] {
    use MoveClass::*;
    match class {
        Phalanx | HeavyInfantry | Legion => &["hilup1", "hilup2", "hilup3", "hilup4"],
        MediumInfantry => &["milup1", "milup2"],
        HeavyCavalry | RomanCavalry | Lancer => &["hcav1", "hcav2", "hcav3"],
        LightCavalry => &["cavlup1", "cavlup2", "cavlup3", "cavlup4"],
        Elephant => &["eleflup1", "eleflup2", "eleflup3"],
        Chariot => &["charlup1", "charlup2", "charlup3"],
        Oxybeles => &["oxymove"],
        _ => &["lilup2", "lilup3", "lilup4", "lilup5"],
    }
}

/// A missile weapon's shot and hit sounds.
fn missile(weapon: Weapon) -> (&'static [&'static str], &'static [&'static str]) {
    match weapon {
        Weapon::Bow | Weapon::MountedBow => (&["bowshot", "bowshot1"], &["arohitsa", "arohitsb", "arohitsc"]),
        Weapon::Sling => (&["sling"], &["rokhit1", "rokhit2", "rokhit3"]),
        Weapon::Javelin | Weapon::MountedJavelin => (&["bowshot"], &["javhits1", "javhits2", "javhits3"]),
        Weapon::Bolt => (&["oxybeles"], &["oxyhit", "oxyhit1", "oxyhit2"]),
    }
}

/// What music plays: the original's groups (`0x61b058`): `start`, then
/// `main` and `alex4` in turn through a battle, `victory` or `defeat` at
/// its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Music {
    Silent,
    /// The start screen and the opening of a battle: `start`, then the
    /// battle's.
    Start,
    Battle,
    End { won: bool },
}

/// Plays sounds; silent if there is no audio output.
pub struct Sound {
    output: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
    dir: PathBuf,
    files: HashMap<String, PathBuf>,
    /// Hannibal's CD tracks (`MUSIC` beside `DATA`), if there.
    cd: Option<PathBuf>,
    cache: HashMap<String, Vec<u8>>,
    seed: u32,
    /// Volumes (0 to 1) of the effects, the ambient sounds and the music.
    volumes: [f32; 3],
    music: Option<rodio::Sink>,
    playing: Music,
    /// The battle music's next piece (Alexander's `main`, `alex4`;
    /// Hannibal's `track1`..`track3`).
    next_piece: usize,
    /// When the next river sound is due, if the battlefield has water.
    next_ambient: Option<std::time::Instant>,
    /// A sound played until stopped (the video's).
    data: Option<rodio::Sink>,
    /// A Director movie's sound channels: what plays in each, and a fade
    /// out under way (since when, over how long).
    channels: HashMap<usize, (rodio::Sink, Option<(std::time::Instant, f32)>)>,
}

impl Sound {
    pub fn new(data_dir: &Path) -> Self {
        let files = std::fs::read_dir(data_dir)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter_map(|p| Some((p.file_name()?.to_str()?.to_ascii_lowercase(), p)))
                    .collect()
            })
            .unwrap_or_default();
        // GBOH_MUTE silences everything
        let output = if std::env::var_os("GBOH_MUTE").is_some() { None } else { rodio::OutputStream::try_default().ok() };
        let music = output.as_ref().and_then(|(_, h)| rodio::Sink::try_new(h).ok());
        let cd = gboh_formats::game::Game::detect(data_dir)
            .is_some_and(|g| g != gboh_formats::game::Game::Alexander)
            .then(|| data_dir.parent().and_then(|p| crate::find_ci(p, "MUSIC")))
            .flatten();
        Self { output, dir: data_dir.into(), files, cd, cache: HashMap::new(), seed: 1, volumes: [0.5; 3], music, playing: Music::Silent, next_piece: 0, next_ambient: None, data: None, channels: HashMap::new() }
    }

    /// Set the volumes (0 to 1) of the effects, the ambient sounds and the
    /// music (0 turns it off).
    pub fn set_volumes(&mut self, effects: f32, ambient: f32, music: f32) {
        self.volumes = [effects, ambient, music];
        if let Some(m) = &self.music {
            m.set_volume(music);
        }
    }

    fn decoded(&mut self, name: &str) -> Option<rodio::Decoder<Cursor<Vec<u8>>>> {
        let key = format!("{name}.wav");
        if !self.cache.contains_key(&key) {
            let path = self.files.get(&key).cloned().unwrap_or_else(|| self.dir.join(&key));
            let data = std::fs::read(path).ok()?;
            self.cache.insert(key.clone(), data);
        }
        rodio::Decoder::new(Cursor::new(self.cache[&key].clone())).ok()
    }

    /// Play a WAV file held in memory (the video's sound) at the music's
    /// volume, until stopped.
    pub fn play_data(&mut self, wav: Vec<u8>) {
        let Some((_, h)) = &self.output else { return };
        let Ok(sink) = rodio::Sink::try_new(h) else { return };
        if let Ok(source) = rodio::Decoder::new(Cursor::new(wav)) {
            sink.append(rodio::Source::convert_samples::<f32>(source));
            sink.set_volume(self.volumes[2].max(0.2));
            self.data = Some(sink);
        }
    }

    fn channel_sink(&mut self, ch: usize) -> Option<&rodio::Sink> {
        let (_, h) = self.output.as_ref()?;
        let sink = rodio::Sink::try_new(h).ok()?;
        sink.set_volume(self.volumes[2].max(0.2));
        self.channels.insert(ch, (sink, None));
        self.channels.get(&ch).map(|(s, _)| s)
    }

    /// Play `samples` (mono, `rate` a second) in a movie's sound channel
    /// `ch`, replacing what played there.
    pub fn channel_samples(&mut self, ch: usize, rate: u32, samples: Vec<i16>) {
        if let Some(sink) = self.channel_sink(ch) {
            sink.append(rodio::buffer::SamplesBuffer::new(1, rate.max(1), samples));
        }
    }

    /// Play the WAV file `path` in a movie's sound channel `ch`.
    pub fn channel_file(&mut self, ch: usize, path: &Path) {
        let Ok(data) = std::fs::read(path) else { return };
        let Ok(source) = rodio::Decoder::new(Cursor::new(data)) else { return };
        if let Some(sink) = self.channel_sink(ch) {
            sink.append(rodio::Source::convert_samples::<f32>(source));
        }
    }

    /// Fade a movie's sound channel out over `secs`.
    pub fn channel_fade(&mut self, ch: usize, secs: f32) {
        if let Some((_, fade)) = self.channels.get_mut(&ch) {
            *fade = Some((std::time::Instant::now(), secs.max(0.01)));
        }
    }

    /// Silence the movie's channels.
    pub fn channels_stop(&mut self) {
        self.channels.clear();
    }

    pub fn stop_data(&mut self) {
        self.data = None;
    }

    /// Change the music to `music` (from its start).
    pub fn set_music(&mut self, music: Music) {
        if music == self.playing {
            return;
        }
        self.playing = music;
        if let Some(m) = &self.music {
            m.stop();
        }
        let first = match music {
            Music::Silent => return,
            Music::Start => "start",
            Music::Battle => return self.tick(),
            Music::End { won: true } => "victory",
            Music::End { won: false } => "defeat",
        };
        self.queue_music(first);
    }

    /// Hannibal's music from its CD (`HanSoundManager`, `FUN_0058486c`):
    /// the start screen track 2, the battle 3 to 5 in turn, a defeat 6, a
    /// victory 7; GOG's copy has them as `MUSIC/TrackNN.ogg` beside `DATA`.
    fn cd_track(&self, name: &str) -> Option<PathBuf> {
        let music = self.cd.as_ref()?;
        let n = match name {
            "start" => 2,
            "track1" => 3,
            "track2" => 4,
            "track3" => 5,
            "defeat" => 6,
            "victory" => 7,
            _ => return None,
        };
        crate::find_ci(music, &format!("Track{n:02}.ogg"))
    }

    fn queue_music(&mut self, name: &str) {
        let cd = self.cd_track(name).and_then(|p| std::fs::read(p).ok()).and_then(|d| rodio::Decoder::new(Cursor::new(d)).ok());
        let Some(source) = cd.or_else(|| self.decoded(name)) else { return };
        if let Some(m) = &self.music {
            m.append(rodio::Source::convert_samples::<f32>(source));
            m.set_volume(self.volumes[2]);
            m.play();
        }
    }

    /// Keep the music going (the battle's pieces in turn, after `start`)
    /// and the river murmuring now and then; call every so often.
    pub fn tick(&mut self) {
        // the movie channels fading out
        let base = self.volumes[2].max(0.2);
        self.channels.retain(|_, (sink, fade)| match fade {
            Some((since, secs)) => {
                let left = 1.0 - since.elapsed().as_secs_f32() / *secs;
                sink.set_volume(base * left.max(0.0));
                left > 0.0
            }
            None => !sink.empty(),
        });
        let idle = self.music.as_ref().is_some_and(|m| m.empty());
        if idle && matches!(self.playing, Music::Start | Music::Battle) {
            self.playing = Music::Battle;
            // Alexander's two pieces, or Hannibal's three
            let pieces: &[&str] = if self.files.keys().any(|k| k.starts_with("track1")) { &["track1", "track2", "track3"] } else { &["main", "alex4"] };
            let piece = pieces[self.next_piece % pieces.len()];
            self.next_piece += 1;
            self.queue_music(piece);
        }
        // 🟡 the original plays a river sound from its ambient group
        // (`FUN_005127b8`) on a timer not traced: here every 8 to 16 s
        if let Some(due) = self.next_ambient
            && due <= std::time::Instant::now()
        {
            let name = self.pick(&["river", "river1", "river2"]);
            let volume = self.volumes[1];
            self.play_at(name, volume);
            let wait = 8 + (self.seed >> 16) % 9;
            self.next_ambient = Some(std::time::Instant::now() + std::time::Duration::from_secs(wait as u64));
        }
    }

    /// Whether the battlefield has water to murmur.
    pub fn set_ambient(&mut self, water: bool) {
        self.next_ambient = water.then(|| std::time::Instant::now() + std::time::Duration::from_secs(3));
    }

    fn pick<'a>(&mut self, names: &[&'a str]) -> &'a str {
        self.seed = self.seed.wrapping_mul(22695477).wrapping_add(1);
        names[((self.seed >> 16) as usize) % names.len()]
    }

    /// Play `name` (without `.wav`) once, at the effects' volume.
    pub fn play_file(&mut self, name: &str) {
        let volume = self.volumes[0];
        self.play_at(name, volume);
    }

    fn play_at(&mut self, name: &str, volume: f32) {
        if self.output.is_none() || volume <= 0.0 {
            return;
        }
        let Some(source) = self.decoded(name) else { return };
        if let Some((_, handle)) = &self.output {
            let _ = handle.play_raw(rodio::Source::convert_samples(rodio::Source::amplify(source, volume)));
        }
    }

    pub fn play(&mut self, event: Event) {
        let name = match event {
            Event::Select => "select",
            Event::Move(class) => self.pick(movement(class)),
            Event::Volley(weapon, hit) => {
                let (shot, hits) = missile(weapon);
                let s = self.pick(shot);
                self.play_file(s);
                if !hit {
                    return;
                }
                self.pick(hits)
            }
            Event::Shock { elephants } => {
                if elephants {
                    let e = self.pick(&["elef1", "elef2", "elef3", "elef4", "elef5", "elefblow"]);
                    self.play_file(e);
                }
                self.pick(&["shoklup1", "shoklup2", "shoklup3", "shoklup4", "shoklup5"])
            }
            Event::Start => return self.set_music(Music::Start),
            Event::End { won } => return self.set_music(Music::End { won }),
        };
        self.play_file(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hannibal's music comes from its CD tracks where they are; runs only
    /// with `GBOH_HANNIBAL`.
    #[test]
    fn hannibal_music_from_the_cd() {
        let Ok(dir) = std::env::var("GBOH_HANNIBAL") else { return };
        let s = Sound::new(Path::new(&dir));
        for name in ["start", "track1", "victory"] {
            let path = s.cd_track(name).expect(name);
            assert!(rodio::Decoder::new(Cursor::new(std::fs::read(path).unwrap())).is_ok(), "{name}");
        }
    }

    /// Every sound named here exists in the game's DATA directory; runs
    /// only with `GBOH_DATA` set.
    #[test]
    fn sounds_exist() {
        let Ok(dir) = std::env::var("GBOH_DATA") else { return };
        let have: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .map(|n| n.to_ascii_lowercase())
            .collect();
        use MoveClass::*;
        let mut names: Vec<&str> = vec!["select", "start", "victory", "defeat", "shoklup1", "shoklup5", "elef1", "elefblow"];
        for c in [Phalanx, HeavyInfantry, MediumInfantry, LightInfantry, Skirmisher, HeavyCavalry, Lancer, LightCavalry, Elephant, Chariot, Oxybeles] {
            names.extend(movement(c));
        }
        for w in [Weapon::Bow, Weapon::Sling, Weapon::Javelin, Weapon::MountedBow, Weapon::MountedJavelin, Weapon::Bolt] {
            let (a, b) = missile(w);
            names.extend(a);
            names.extend(b);
        }
        for n in names {
            assert!(have.contains(&format!("{n}.wav")), "{n}.wav missing");
        }
    }
}
