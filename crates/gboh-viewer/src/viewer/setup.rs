//! The setup screens (start screen, battles, sides, difficulty), and starting a battle.

use crate::*;

impl Viewer {
    /// More...: the scenario files in `DATA` besides the battles' (the
    /// original's `VARIANTDLG` listing them); none, nothing happens.
    pub(crate) fn choose_custom(&mut self) {
        let files = setup::Setup::custom_files(&self.data_dir);
        if files.is_empty() {
            return;
        }
        let d = hud::Plain { name: "VARIANTDLG", list: files.clone(), selected: Some(0), ..Default::default() };
        self.plain = Some((d, Asked::Custom { files }));
        self.update_hud();
        self.redraw();
    }

    /// A click on the More... list: a file (again to choose it), OK, Cancel.
    pub(crate) fn custom_click(&mut self, files: &[String], d: &hud::Plain, at: Option<u16>, (w, h, cx, cy): (usize, usize, i32, i32)) {
        let chosen = match at {
            Some(1) => d.selected,
            Some(2) => {
                self.plain = None;
                None
            }
            Some(101) => {
                let k = self.hud.plain_line_at(d, w, h, cx, cy);
                let again = k.is_some() && k == d.selected;
                if let (Some(k), Some((p, _))) = (k, &mut self.plain) {
                    p.selected = Some(k);
                }
                if again { k } else { None }
            }
            _ => None,
        };
        if let Some(file) = chosen.and_then(|k| files.get(k)) {
            self.plain = None;
            let data_dir = self.data_dir.clone();
            if self.setup.as_mut().is_some_and(|s| s.choose_custom(&data_dir, file)) {
                self.show_setup();
            }
        }
    }

    /// `VARIANTDLG` for a battle with variants: Standard (string 47), then
    /// theirs by description.
    pub(crate) fn choose_variant(&mut self, variants: Vec<(String, String)>) {
        let standard = crate::ui::exe_resources(&self.data_dir).and_then(|r| r.string(47)).unwrap_or_else(|| "Standard".into());
        let list = std::iter::once(standard).chain(variants.iter().map(|v| v.1.clone())).collect();
        let files = std::iter::once(None).chain(variants.into_iter().map(|v| Some(v.0))).collect();
        self.plain = Some((hud::Plain { name: "VARIANTDLG", list, selected: Some(0), ..Default::default() }, Asked::Variants { files }));
        self.update_hud();
        self.redraw();
    }

    /// A click on the variants' dialog: OK (or a second click on a line)
    /// goes on with the chosen one, Cancel back to the battles.
    pub(crate) fn variant_click(&mut self, files: &[Option<String>], d: &hud::Plain, at: Option<u16>, (w, h, cx, cy): (usize, usize, i32, i32)) {
        let chosen = match at {
            Some(1) => d.selected,
            Some(2) => {
                self.plain = None;
                None
            }
            Some(101) => {
                let k = self.hud.plain_line_at(d, w, h, cx, cy);
                let again = k.is_some() && k == d.selected;
                if let (Some(k), Some((p, _))) = (k, &mut self.plain) {
                    p.selected = Some(k);
                }
                if again { k } else { None }
            }
            _ => None,
        };
        let Some(file) = chosen.and_then(|k| files.get(k)) else {
            self.update_hud();
            self.redraw();
            return;
        };
        self.plain = None;
        let data_dir = self.data_dir.clone();
        if let Some(s) = &mut self.setup {
            match file {
                None => s.choose_standard(),
                Some(f) => {
                    s.choose_custom(&data_dir, f);
                }
            }
        }
        self.show_setup();
        self.update_hud();
        self.redraw();
    }

    /// Leave the battle for the start screen.
    pub(crate) fn back_to_start(&mut self) {
        self.play = None;
        self.net = None;
        self.scenes.clear();
        (self.ending, self.ended, self.game_overview, self.army, self.results, self.details) = (false, false, false, None, None, None);
        self.queued.clear();
        self.playing = None;
        self.plain = None;
        self.runners.clear();
        self.title = String::from("The Great Battles of Alexander");
        self.save_name = None;
        self.sound.set_ambient(false);
        self.sound.set_music(sound::Music::Start);
        self.setup = Some(setup::Setup::new(&self.data_dir));
        self.show_setup();
        self.update_title();
        self.update_hud();
    }

    /// The setup screen fitted to the window: scaled to fit whole (the
    /// scale need not be whole), centred on the screens' marble filling the
    /// rest.
    pub(crate) fn setup_canvas(&mut self) -> Option<Canvas> {
        let setup = self.setup.as_ref()?;
        let screen = setup.canvas();
        let size = self.gpu.as_ref().map_or(PhysicalSize::new(screen.width as u32, screen.height as u32), |g| g.window.inner_size());
        // every screen at the scale that fits the start screen (the
        // original's 640 × 480): the smaller dialogs keep their size
        // against it, in the middle of the marble
        let (fw, fh) = (screen.width.max(640) as f32, screen.height.max(480) as f32);
        let fit = (size.width as f32 / fw).min(size.height as f32 / fh).max(0.25);
        self.scale = fit;
        let (w, h) = ((size.width as f32 / fit).ceil() as usize, (size.height as f32 / fit).ceil() as usize);
        let (cw, ch) = (w.max(screen.width), h.max(screen.height));
        // (a dialog smaller than the start screen stands out from what's
        // behind it)
        let mut canvas =
            if screen.width < 640 || screen.height < 480 { setup.dialog_backdrop(cw, ch, (screen.width, screen.height)) } else { setup.backdrop(cw, ch) };
        let off = ((canvas.width as i32 - screen.width as i32) / 2, (canvas.height as i32 - screen.height as i32) / 2);
        canvas.blit(&gboh_render::Sprite { width: screen.width, height: screen.height, pixels: screen.pixels }, off.0, off.1);
        self.setup_offset = off;
        Some(canvas)
    }

    pub(crate) fn show_setup(&mut self) {
        let Some(canvas) = self.setup_canvas() else { return };
        self.centre = [canvas.width as f32 / 2.0, canvas.height as f32 / 2.0];
        self.canvases = vec![canvas];
        self.level = 0;
        if let Some(g) = &mut self.gpu {
            g.set_levels(&self.canvases, &[]);
        }
        self.redraw();
    }

    /// Leave the setup screens for `scenario`, the computer playing the
    /// sides marked (home, visitor), with the armies' rout levels.
    pub(crate) fn start_battle(&mut self, scenario: &str, computer: [bool; 2], levels: [u16; 2]) -> Result<()> {
        // a saved game's own options, else the settings' (with which an
        // Alexander battle may begin with a deployment phase)
        let saved = self.options_next.take();
        let options = saved.or(self.net_start.take()).unwrap_or(self.settings.options);
        let alexander = gboh_formats::game::Game::detect(&self.data_dir) == Some(gboh_formats::game::Game::Alexander);
        let deploy = std::mem::take(&mut self.deploy_next) || (saved.is_none() && alexander && options.deployment);
        let mut b = load_battle(&self.data_dir, scenario, computer, deploy)?;
        // (Alexander's menus lack the commands of options from Hannibal)
        self.menu_bar = menu::MenuBar::load(&self.data_dir, "CHILDMENU");
        if alexander {
            if options.group_orders {
                self.menu_bar.add_after(212, 260, "Group Restore &Cohesion");
                self.menu_bar.add_after(212, 259, "Group &Attack");
                self.menu_bar.add_after(212, 248, "Group &Retreat");
            }
            if options.backward {
                self.menu_bar.add_after(235, 246, "&Backward Movement");
            }
        }
        if let Some(p) = &mut b.play {
            p.game.set_options(options);
            p.game.armies[0].1 = levels[0];
            p.game.armies[1].1 = levels[1];
            // Hannibal's set-up option
            p.game.turn_limits = self.setup.as_ref().is_some_and(|s| s.turn_limits);
        }
        self.setup = None;
        self.battle = (scenario.to_string(), computer);
        self.autosaved = 0;
        (self.ending, self.ended, self.game_overview, self.army, self.results, self.details, self.plain) = (false, false, false, None, None, None, None);
        self.queued.clear();
        self.playing = None;
        self.runners.clear();
        self.title = b.title;
        self.save_name = None;
        self.sound.set_ambient(b.play.as_ref().is_some_and(|p| p.game.map.elevation.contains(&0)));
        self.map_file = b.map_file;
        self.scenes = b.scenes;
        self.canvases = b.canvases;
        self.play = b.play;
        for s in self.scenes.iter_mut() {
            s.sparse = self.prefs.sparse_trees;
        }
        if let Some(p) = &mut self.play {
            p.game.triarii_doctrine = self.prefs.triarii;
        }
        self.level = 1;
        self.scale = self.display_scale;
        self.centre = [self.canvases[1].width as f32 / 2.0, self.canvases[1].height as f32 / 2.0];
        let tiles = self.tiles();
        if let Some(g) = &mut self.gpu {
            g.set_levels(&self.canvases, &tiles);
        }
        if let Some(p) = &mut self.play {
            for e in p.events.drain(..) {
                self.sound.play(e);
            }
        }
        self.stale = [true; 3];
        self.refresh();
        self.update_title();
        self.update_hud();
        self.redraw();
        Ok(())
    }
}
