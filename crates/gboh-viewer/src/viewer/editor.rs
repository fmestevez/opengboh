//! The scenario editor screen: opening it, drawing it, its clicks and keys.

use crate::*;

impl Viewer {
    /// Open the scenario editor, on Caesar's data (where the three games'
    /// art and the editor's unit catalogue are).
    pub(crate) fn open_editor(&mut self) {
        if library::data_dir(library::Title::Caesar).is_none() {
            self.notice(&["The scenario editor needs The Great Battles of", "Caesar imported (its data has every game's art)."]);
            return;
        }
        self.switch_game(library::Title::Caesar);
        self.setup = None;
        self.editor = Some(editor::EditorScreen::new(&self.data_dir));
        self.show_editor();
    }

    /// Draw the editor's screen, the size of the window.
    pub(crate) fn show_editor(&mut self) {
        let Some(e) = &self.editor else { return };
        self.scale = self.display_scale;
        let (w, h) = self.window_px();
        let (canvas, _) = e.canvas(w.max(1), h.max(1));
        let palette = e.palette().clone();
        self.centre = [canvas.width as f32 / 2.0, canvas.height as f32 / 2.0];
        self.canvases = vec![canvas];
        self.level = 0;
        if let Some(g) = &mut self.gpu {
            g.set_levels(&self.canvases, &[]);
            g.set_palette(&palette);
            g.set_overlay(None);
        }
        self.redraw();
    }

    /// A click on the editor's screen.
    pub(crate) fn editor_click(&mut self) {
        let (w, h) = self.window_px();
        let (cx, cy) = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
        let Some(e) = &mut self.editor else { return };
        match e.click(cx, cy, w, h) {
            editor::Outcome::Nothing => {}
            editor::Outcome::Redraw => self.show_editor(),
            editor::Outcome::Play(path) => {
                self.editor = None;
                let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                let levels = Scenario::parse(&std::fs::read(&path).unwrap_or_default()).map_or([100, 100], |s| [s.home.rout_level, s.visitor.rout_level]);
                if let Err(err) = self.start_battle(&file, [false, true], levels) {
                    eprintln!("editor: {err:#}");
                    self.back_to_start();
                }
                self.update_hud();
                self.redraw();
            }
            editor::Outcome::Quit => {
                self.editor = None;
                self.back_to_start();
            }
        }
    }

    /// A key on the editor's screen: typing into its fields, else arrows
    /// scroll the battlefield.
    pub(crate) fn editor_key(&mut self, key: &Key, text: Option<&str>) {
        let (w, h) = self.window_px();
        let Some(e) = &mut self.editor else { return };
        let named = |n: NamedKey| key.as_ref() == Key::Named(n);
        if !e.key(text, named(NamedKey::Backspace), named(NamedKey::Enter)) {
            let (dx, dy) = match key.as_ref() {
                Key::Named(NamedKey::ArrowLeft) => (-60, 0),
                Key::Named(NamedKey::ArrowRight) => (60, 0),
                Key::Named(NamedKey::ArrowUp) => (0, -60),
                Key::Named(NamedKey::ArrowDown) => (0, 60),
                _ => (0, 0),
            };
            e.scroll_by(dx, dy, w, h);
        }
        self.show_editor();
    }
}
