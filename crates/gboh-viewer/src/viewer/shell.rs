//! opengboh's own screens (content manager, settings, rules, All Battles), importing the games, and switching between them.

use crate::*;

impl Viewer {
    /// Show one of the rewrite's own screens.
    pub(crate) fn open_shell(&mut self, screen: shell::Screen) {
        self.shell = Some(screen);
        self.news = (String::new(), false);
        self.show_shell();
    }

    /// Draw the shell screen up, the size of the window, in its palette.
    pub(crate) fn show_shell(&mut self) {
        let Some(screen) = &self.shell else { return };
        self.scale = self.display_scale;
        let (w, h) = self.window_px();
        let imported = library::imported();
        let view = shell::View {
            imported: &imported,
            game: self.settings.game_in_force(&imported),
            options: self.settings.options,
            status: self.news.clone(),
            busy: self.importing.is_some(),
            pressed: self.shell_pressed,
        };
        let (canvas, buttons) = shell::draw(screen, &view, w.max(1), h.max(1));
        self.shell_buttons = buttons;
        self.centre = [canvas.width as f32 / 2.0, canvas.height as f32 / 2.0];
        self.canvases = vec![canvas];
        self.level = 0;
        if let Some(g) = &mut self.gpu {
            g.set_levels(&self.canvases, &[]);
            g.set_palette(&shell::palette());
            g.set_overlay(None);
        }
        self.redraw();
    }

    /// A button of a shell screen.
    pub(crate) fn shell_click(&mut self, id: u16) {
        use library::Title;
        let imported = library::imported();
        match (id, self.shell.clone()) {
            (shell::QUIT, _) => self.quit = true,
            // the rules: back to the settings
            (shell::BACK, Some(shell::Screen::Rules)) => self.open_shell(shell::Screen::Settings),
            (shell::RULES, _) => self.open_shell(shell::Screen::Rules),
            (id, _) if (shell::OPTION..shell::OPTION + gboh_game::options::ALL.len() as u16).contains(&id) => {
                let key = gboh_game::options::ALL[(id - shell::OPTION) as usize].0;
                if let Some(f) = self.settings.options.get_mut(key) {
                    *f = !*f;
                }
                self.settings.save();
                self.show_shell();
            }
            (shell::CONTINUE | shell::BACK, _) => {
                if let Some(t) = self.settings.game_in_force(&imported) {
                    self.shell = None;
                    self.switch_game(t);
                }
            }
            (shell::CONTENT, _) => self.open_shell(shell::Screen::Content),
            // the game's own Preferences, over its start screen
            (shell::PREFERENCES, _) => {
                if let Some(t) = self.settings.game_in_force(&imported) {
                    self.shell = None;
                    self.switch_game(t);
                    self.open_preferences();
                    self.update_hud();
                    self.redraw();
                }
            }
            (id, _) if (shell::IMPORT..shell::IMPORT + 3).contains(&id) => {
                let t = Title::ALL[(id - shell::IMPORT) as usize];
                self.start_import(t);
            }
            (id, _) if (shell::SELECT_GAME..shell::SELECT_GAME + 3).contains(&id) => {
                self.settings.game = Some(Title::ALL[(id - shell::SELECT_GAME) as usize]);
                self.settings.save();
                self.show_shell();
            }
            (id, Some(shell::Screen::Selector { entries, .. })) if (shell::SCROLL_AT..shell::ENTRY).contains(&id) => {
                let scroll = ((id - shell::SCROLL_AT) as usize).min(entries.len().saturating_sub(1));
                self.shell = Some(shell::Screen::Selector { entries, scroll });
                self.show_shell();
            }
            (shell::SCROLL_UP | shell::SCROLL_DOWN, Some(shell::Screen::Selector { entries, scroll })) => {
                let scroll = if id == shell::SCROLL_UP { scroll.saturating_sub(5) } else { (scroll + 5).min(entries.len().saturating_sub(1)) };
                self.shell = Some(shell::Screen::Selector { entries, scroll });
                self.show_shell();
            }
            (id, Some(shell::Screen::Selector { entries, .. })) if id >= shell::ENTRY => {
                let Some(e) = entries.get((id - shell::ENTRY) as usize).cloned() else { return };
                self.shell = None;
                self.switch_game(e.title);
                match e.scenario {
                    None => self.start_campaign(),
                    Some(file) => {
                        let data_dir = self.data_dir.clone();
                        if self.setup.as_mut().is_some_and(|s| s.choose_custom(&data_dir, &file)) {
                            self.show_setup();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Ask for the folder of title `t` and copy it in.
    pub(crate) fn start_import(&mut self, t: library::Title) {
        let Some(folder) = rfd::FileDialog::new().set_title(format!("Choose the folder where {} is installed", t.name())).pick_folder() else { return };
        match library::detect(&folder, t) {
            Ok(found) => {
                self.news = (format!("Importing {}...", t.name()), false);
                self.importing = Some((t, library::import(found)));
            }
            Err(e) => self.news = (e, true),
        }
        self.show_shell();
    }

    /// An import's progress, and its end.
    pub(crate) fn poll_import(&mut self) {
        let Some((t, p)) = &self.importing else { return };
        let (t, p) = (*t, p.clone());
        let done = p.done.lock().unwrap().take();
        match done {
            None => {
                let (c, total) = (p.copied.load(std::sync::atomic::Ordering::Relaxed), p.total.load(std::sync::atomic::Ordering::Relaxed).max(1));
                self.news = (format!("Importing {}: {}%", t.name(), c * 100 / total), false);
            }
            Some(Ok(())) => {
                self.importing = None;
                self.news = (format!("{} imported.", t.name()), false);
                // a first title imported becomes the game selected
                if self.settings.game.is_none() {
                    self.settings.game = Some(t);
                    self.settings.save();
                }
            }
            Some(Err(e)) => {
                self.importing = None;
                self.news = (format!("Import failed: {e}"), true);
            }
        }
        self.show_shell();
    }

    /// Use title `t`'s data: its look (menus, art, sounds, palette) and its
    /// start screen.
    pub(crate) fn switch_game(&mut self, t: library::Title) {
        let Some(dir) = library::data_dir(t) else { return };
        if self.game != Some(t) || self.data_dir != dir {
            self.data_dir = dir.clone();
            self.hud = hud::Hud::new(&dir);
            self.menu_bar = menu::MenuBar::load(&dir, "CHILDMENU");
            if let Some(pal) = palette_path(&dir).and_then(|p| std::fs::read(p).ok()).and_then(|d| Palette::parse(&d).ok()) {
                self.base_palette = pal;
            }
            self.sound = sound::Sound::new(&dir);
            self.apply_prefs();
            self.campaign_art = None;
            self.help = None;
            self.game = Some(t);
        }
        self.setup = Some(setup::Setup::new(&dir));
        self.title = self.hud_title();
        self.show_setup();
        self.upload_palette();
        self.sound.set_music(sound::Music::Start);
        self.update_title();
        self.update_hud();
    }
}
