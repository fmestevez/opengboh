//! The preferences, and saving and loading games.

use crate::*;

impl Viewer {
    /// "Don't show this again" on the finish leader reminder.
    pub(crate) fn set_quiet_finish(&mut self, quiet: bool) {
        if quiet && self.prefs.finish_leader_dialog {
            self.prefs.finish_leader_dialog = false;
            self.save_prefs();
        }
    }

    pub(crate) fn save_prefs(&mut self) {
        if let Err(e) = self.prefs.save(&prefs::Prefs::path()) {
            eprintln!("preferences: {e}");
        }
        self.apply_prefs();
    }

    /// Put the preferences in force: the sounds' volumes and the music.
    pub(crate) fn apply_prefs(&mut self) {
        // Hannibal's options: the Triarii doctrine and sparse trees
        if let Some(p) = &mut self.play {
            p.game.triarii_doctrine = self.prefs.triarii;
        }
        if self.scenes.iter().any(|s| s.sparse != self.prefs.sparse_trees) {
            for s in self.scenes.iter_mut() {
                s.sparse = self.prefs.sparse_trees;
            }
            self.stale = [true; 3];
            self.refresh();
        }
        let volume = |on: bool, v: u16| if on { v as f32 / hud::SLIDER_MAX as f32 } else { 0.0 };
        self.sound.set_volumes(
            volume(self.prefs.sounds, self.prefs.volume),
            volume(self.prefs.ambient_sounds, self.prefs.volume),
            volume(self.prefs.music, self.prefs.music_volume),
        );
    }

    /// The Preferences dialog.
    pub(crate) fn open_preferences(&mut self) {
        let game = gboh_formats::game::Game::detect(&self.data_dir).unwrap_or_default();
        self.plain = Some((self.prefs.dialog(game), Asked::Preferences));
    }

    /// A click on the Preferences dialog: a check box, a slider, OK (keep
    /// them), Cancel, Help.
    pub(crate) fn preferences_click(&mut self, d: &hud::Plain, at: Option<u16>, (w, h, cx, cy): (usize, usize, i32, i32)) {
        if let Some((k, v)) = self.hud.plain_slider_at(d, w, h, cx, cy) {
            if let Some((d, _)) = &mut self.plain {
                d.sliders[k].2 = v;
            }
            self.sliding = Some(k);
        } else {
            match at {
                Some(1) => {
                    self.prefs = self.prefs.with_dialog(d);
                    self.plain = None;
                    self.save_prefs();
                }
                Some(2) => self.plain = None,
                Some(9) => self.open_help(),
                // Hannibal's trees: one of the two
                Some(id @ (prefs::SPARSE | prefs::DENSE)) => {
                    if let Some((d, _)) = &mut self.plain {
                        d.checked.retain(|&c| c != prefs::SPARSE && c != prefs::DENSE);
                        d.checked.push(id);
                    }
                }
                Some(id) if prefs::Prefs::is_box(id) => {
                    if let Some((d, _)) = &mut self.plain {
                        if let Some(k) = d.checked.iter().position(|&c| c == id) {
                            d.checked.remove(k);
                        } else {
                            d.checked.push(id);
                        }
                    }
                }
                _ => {}
            }
        }
        self.update_hud();
        self.redraw();
    }

    /// The game as it stands, to save under `description`.
    pub(crate) fn current_save(&self, description: String) -> Option<saves::Save> {
        let p = self.play.as_ref()?;
        Some(saves::Save {
            description,
            scenario: self.battle.0.clone(),
            computer: self.battle.1,
            levels: [p.game.armies[0].1, p.game.armies[1].1],
            deploy: p.deployed,
            options: p.game.battle.options,
            orders: p.orders.clone(),
            campaign: self.campaign.as_ref().map(|c| c.to_text()),
        })
    }

    /// A description of the game as it stands: battle, turn and active
    /// leader, as the original titles its saves.
    pub(crate) fn describe(&self) -> String {
        let Some(p) = &self.play else { return String::new() };
        let leader = p.leader().map(|l| p.game.battle.units[l].name.clone()).unwrap_or_default();
        format!("{}; Turn #{}; {leader}", self.title, p.game.activation.turn + 1)
    }

    /// Save the game as `Autosave.sav` at the start of each turn after the
    /// first (as the original autosaves).
    pub(crate) fn autosave(&mut self) {
        let Some(p) = &self.play else { return };
        let turn = p.game.activation.turn;
        if turn <= self.autosaved || p.over || !self.prefs.auto_save || !self.battle.0.to_ascii_lowercase().ends_with(".scn") {
            return;
        }
        self.autosaved = turn;
        if let Some(save) = self.current_save(format!("Autosaved Game: {}", self.describe()))
            && let Err(e) = saves::write(&saves::dir(), "Autosave", &save)
        {
            eprintln!("autosave: {e}");
        }
    }

    /// Open the save (or load) dialog (`LOADSAVEDLG`): a description and a
    /// file name to type, the saves found, OK, Cancel and Delete.
    pub(crate) fn open_files(&mut self, save: bool) {
        if save && self.play.is_none() {
            return;
        }
        let entries = saves::list(&saves::dir());
        let description = if save { self.describe() } else { String::new() };
        let file = if save { "Save1".to_string() } else { String::new() };
        let d = hud::Plain {
            name: "LOADSAVEDLG",
            texts: vec![(0, (if save { "Save Game" } else { "Load Game" }).into()), (10, description), (12, file)],
            list: entries.iter().map(|(f, d)| format!("{f}  {d}")).collect(),
            focus: Some(if save { 10 } else { 12 }),
            ..Default::default()
        };
        self.plain = Some((d, Asked::Files { save, entries }));
        self.update_hud();
        self.redraw();
    }

    /// A click on the save or load dialog.
    pub(crate) fn files_click(&mut self, save: bool, entries: &[(String, String)], d: &hud::Plain, at: Option<u16>, (w, h, cx, cy): (usize, usize, i32, i32)) {
        let text = |id: u16| d.texts.iter().find(|(k, _)| *k == id).map(|(_, t)| t.clone()).unwrap_or_default();
        match at {
            Some(1) => self.files_ok(save, &text(12), &text(10)),
            Some(2) | None => self.plain = None,
            Some(13) => {
                if let Some(k) = d.selected {
                    let _ = std::fs::remove_file(saves::dir().join(&entries[k].0));
                    self.plain = None;
                    self.open_files(save);
                    return;
                }
            }
            Some(id @ (10 | 12)) => {
                if let Some((d, _)) = &mut self.plain {
                    d.focus = Some(id);
                }
            }
            Some(11) => {
                if let (Some(k), Some((d, _))) = (self.hud.plain_line_at(d, w, h, cx, cy), &mut self.plain) {
                    let (file, description) = &entries[k];
                    let again = d.selected == Some(k);
                    d.selected = Some(k);
                    for (id, t) in d.texts.iter_mut() {
                        match id {
                            10 => *t = description.clone(),
                            12 => *t = file.trim_end_matches(".sav").to_string(),
                            _ => {}
                        }
                    }
                    // a second click on a save loads it
                    if again && !save {
                        let name = file.clone();
                        self.files_ok(false, &name, "");
                        return;
                    }
                }
            }
            _ => {}
        }
        self.update_hud();
        self.redraw();
    }

    /// OK on the save or load dialog: save under `file`, or load it.
    pub(crate) fn files_ok(&mut self, save: bool, file: &str, description: &str) {
        let file = file.trim();
        if file.is_empty() {
            return;
        }
        if save {
            match self.current_save(description.to_string()).map(|s| saves::write(&saves::dir(), file, &s)) {
                Some(Ok(path)) => {
                    println!("saved {}", path.display());
                    self.save_name = Some((file.to_string(), description.to_string()));
                }
                Some(Err(e)) => eprintln!("save: {e}"),
                None => {}
            }
            self.plain = None;
        } else {
            let mut path = saves::dir().join(file);
            if path.extension().is_none_or(|x| x != "sav") {
                path.set_extension("sav");
            }
            self.plain = None;
            if let Err(e) = self.load_game(&path) {
                eprintln!("load {}: {e:#}", path.display());
            }
        }
        self.update_hud();
        self.redraw();
    }

    /// Load a saved game: start its battle afresh and replay its orders.
    pub(crate) fn load_game(&mut self, path: &Path) -> Result<()> {
        let text = std::fs::read_to_string(path)?;
        let save = saves::Save::parse(&text).map_err(|e| anyhow::anyhow!(e))?;
        let reversed = self.scenes.get(self.level).is_some_and(|s| s.layout.reversed);
        self.campaign = save.campaign.as_deref().and_then(gboh_game::campaign::Campaign::parse);
        self.net = None;
        self.deploy_next = save.deploy;
        self.options_next = Some(save.options);
        self.start_battle(&save.scenario, save.computer, save.levels)?;
        self.campaign_penalty();
        if let Some(p) = &mut self.play {
            p.replay(&save.orders);
            self.autosaved = p.game.activation.turn;
            self.ended = p.over;
        }
        if reversed {
            self.reverse();
        }
        self.changed();
        Ok(())
    }
}
