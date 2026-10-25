//! The window's events: input, resizing, redrawing and the frame clock.

use crate::*;

impl ApplicationHandler for Viewer {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_inner_size(LogicalSize::new(1280.0, 800.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        // one battlefield pixel per logical pixel, like the original on a
        // 1x display
        self.scale = window.scale_factor().round().max(1.0) as f32;
        self.display_scale = self.scale;
        let tiles = self.tiles();
        match Gpu::new(window, &self.canvases, &tiles) {
            Ok(g) => self.gpu = Some(g),
            Err(e) => {
                eprintln!("error: {e:#}");
                event_loop.exit();
                return;
            }
        }
        self.upload_palette();
        self.apply_prefs();
        if self.shell.is_some() {
            self.show_shell();
        }
        if self.setup.is_some() {
            self.title = self.hud_title();
        }
        // the game's own cursors (`alex.exe`'s cursor resources)
        if let Some(res) = crate::ui::exe_resources(&self.data_dir) {
            for name in CURSORS {
                let Some(c) = res.cursor(name) else { continue };
                if let Ok(src) = winit::window::CustomCursor::from_rgba(c.rgba, c.width, c.height, c.hot.0, c.hot.1) {
                    self.cursors.insert(name, event_loop.create_custom_cursor(src));
                }
            }
        }
        if self.setup.is_some() {
            // the start screen fits the window from the first
            self.show_setup();
            self.sound.set_music(sound::Music::Start);
            // the intro (a Director projector the original's launcher ran
            // first), where it can run and the preferences want it
            if self.prefs.intro_video {
                self.play_intro();
            }
        }
        if let Some(p) = &mut self.play {
            self.sound.set_ambient(p.game.map.elevation.contains(&0));
            for e in p.events.drain(..) {
                self.sound.play(e);
            }
        }
        self.stale = [true; 3];
        self.refresh();
        self.update_title();
        self.update_hud();
        self.redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        const STEP: f32 = 64.0;
        // buttons show pressed while the left button is held on them
        if let WindowEvent::MouseInput { state, button: MouseButton::Left, .. } = &event {
            self.set_pressed(*state == ElementState::Pressed);
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(g) = &mut self.gpu {
                    g.resize(size);
                }
                // the setup screens fit themselves to the window again
                if self.editor.is_some() {
                    self.show_editor();
                } else if self.shell.is_some() {
                    self.show_shell();
                } else if self.setup.is_some() {
                    self.show_setup();
                }
                self.update_hud();
                self.redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Some(g) = &self.gpu {
                    g.draw(self.level, self.origin(), self.scale);
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && self.slides.is_some() => self.end_slides(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && self.intro => self.end_intro(),
            WindowEvent::MouseInput { state: ElementState::Pressed, .. } if self.intro => self.end_intro(),
            WindowEvent::MouseInput { state: ElementState::Pressed, .. } if self.slides.is_some() => self.end_slides(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && self.plain_key(&event.logical_key, event.text.as_deref()) => {}
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && self.editor.is_some() => {
                self.editor_key(&event.logical_key, event.text.as_deref())
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && self.setup.is_some() => {
                if event.logical_key.as_ref() == Key::Named(NamedKey::Escape) {
                    event_loop.exit();
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && (self.modifiers.control_key() || self.modifiers.super_key()) =>
            {
                // Ctrl (or Cmd) S saves, L or O loads
                if let Key::Character(c) = event.logical_key.as_ref() {
                    match c.to_ascii_lowercase().as_str() {
                        "s" => self.command(226),
                        "l" | "o" => self.open_files(false),
                        "q" if self.play.is_some() => self.command(211),
                        "m" if self.play.is_some() => self.command(219),
                        "c" if self.play.is_some() => self.command(239),
                        "z" if self.play.is_some() => self.command(255),
                        _ => {}
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => match event.logical_key.as_ref() {
                Key::Named(NamedKey::Escape) if self.menu_open.is_some() || self.reader.is_some() => {
                    self.menu_open = None;
                    self.reader = None;
                    self.update_hud();
                    self.redraw();
                }
                Key::Named(NamedKey::Escape) => event_loop.exit(),
                Key::Named(NamedKey::ArrowLeft) => self.scroll(-STEP, 0.0),
                Key::Named(NamedKey::ArrowRight) => self.scroll(STEP, 0.0),
                Key::Named(NamedKey::ArrowUp) => self.scroll(0.0, -STEP),
                Key::Named(NamedKey::ArrowDown) => self.scroll(0.0, STEP),
                Key::Named(NamedKey::Space) => self.animate = !self.animate,
                Key::Named(NamedKey::Enter) if self.plain.is_none() => self.finish_leader(),
                Key::Character(c) if !self.my_turn() && "mgrcqe".contains(c.to_ascii_lowercase().as_str()) => {}
                Key::Character(c) => match c.to_ascii_lowercase().as_str() {
                    "a" => self.scroll(-STEP, 0.0),
                    "d" => self.scroll(STEP, 0.0),
                    "w" => self.scroll(0.0, -STEP),
                    "s" => self.scroll(0.0, STEP),
                    "1" => self.set_level(0),
                    "2" => self.set_level(1),
                    "3" => self.set_level(2),
                    "m" => {
                        if self.play.as_mut().is_some_and(|p| p.finish(true)) {
                            self.changed();
                        }
                    }
                    "g" => {
                        if self.play.as_mut().is_some_and(|p| p.group_move()) {
                            self.changed();
                        }
                    }
                    "r" => {
                        if self.play.as_mut().is_some_and(|p| p.rally()) {
                            self.changed();
                        }
                    }
                    "c" => {
                        if self.play.as_mut().is_some_and(|p| p.recover()) {
                            self.changed();
                        }
                    }
                    "q" | "e" => {
                        if self.play.as_mut().is_some_and(|p| p.turn(c.eq_ignore_ascii_case("q"))) {
                            self.changed();
                        }
                    }
                    "+" | "=" => {
                        self.scale = (self.scale + 1.0).min(8.0);
                        self.update_title();
                        self.redraw();
                    }
                    "-" => {
                        self.scale = (self.scale - 1.0).max(1.0);
                        self.update_title();
                        self.redraw();
                    }
                    _ => {}
                },
                _ => {}
            },
            // the editor acts as the button goes down (a brush), and a drag
            // from there paints on, or pans its battlefield
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } if self.editor.is_some() => {
                if state == ElementState::Pressed {
                    self.drag = Some(self.cursor);
                    self.editor_click();
                } else {
                    self.drag = None;
                    if let Some(e) = &mut self.editor {
                        e.drag_end();
                    }
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } if self.map_drag(state == ElementState::Pressed) => {}
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                if state == ElementState::Released {
                    self.sliding = None;
                }
                // no dragging the battlefield from under a dialog or a menu
                self.drag = (state == ElementState::Pressed && !self.modal() && self.menu_open.is_none()).then_some(self.cursor);
                if state == ElementState::Pressed {
                    self.press = Some(self.cursor);
                } else if let Some(p) = self.press.take() {
                    // a click, not a drag
                    if (p.x - self.cursor.x).abs() + (p.y - self.cursor.y).abs() < 6.0 {
                        self.click(false);
                    }
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } => {
                self.click(true);
            }
            WindowEvent::CursorMoved { position, .. } if self.campaign_map.as_ref().is_some_and(|m| m.dragging.is_some()) => {
                self.cursor = position;
                self.map_move();
            }
            WindowEvent::CursorMoved { position, .. } if self.menu_open.is_some() || self.sliding.is_some() => {
                self.cursor = position;
                let (cx, cy) = ((position.x as f32 / self.scale) as i32, (position.y as f32 / self.scale) as i32);
                if let Some(k) = self.sliding {
                    let (w, h) = self.window_px();
                    let value = self.plain.as_ref().and_then(|(d, _)| self.hud.plain_slider_value(d, w, h, k, cx));
                    if let (Some((d, _)), Some(v)) = (&mut self.plain, value)
                        && let Some(sl) = d.sliders.get_mut(k)
                    {
                        sl.2 = v;
                    }
                } else if let Some((k, _)) = self.menu_open {
                    let hover = self.menu_bar.item_at(k, cx, cy);
                    let k = self.menu_bar.bar_at(cx, cy).unwrap_or(k);
                    self.menu_open = Some((k, hover));
                }
                self.update_hud();
                self.redraw();
            }
            // the editor paints, or pans its battlefield, under a drag
            WindowEvent::CursorMoved { position, .. } if self.editor.is_some() && self.drag.is_some() => {
                let (w, h) = self.window_px();
                let at = |p: PhysicalPosition<f64>| ((p.x as f32 / self.scale) as i32, (p.y as f32 / self.scale) as i32);
                let (from, to) = (at(self.drag.unwrap_or(position)), at(position));
                if let Some(e) = &mut self.editor {
                    e.drag(from, to, w, h);
                }
                // (drawn once the events waiting are handled)
                self.drag = Some(position);
                self.cursor = position;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(from) = self.drag {
                    self.scroll((from.x - position.x) as f32, (from.y - position.y) as f32);
                    self.drag = Some(position);
                }
                self.cursor = position;
                self.update_cursor();
            }
            WindowEvent::CursorLeft { .. } => {
                self.edge = None;
                self.update_cursor();
            }
            // the editor: its list over the panel, else the battlefield
            WindowEvent::MouseWheel { delta, .. } if self.editor.is_some() => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x * 40.0, -y * 40.0),
                    MouseScrollDelta::PixelDelta(p) => (-p.x as f32, -p.y as f32),
                };
                let (w, h) = self.window_px();
                let cx = (self.cursor.x as f32 / self.scale) as i32;
                if let Some(e) = &mut self.editor {
                    if editor::EditorScreen::on_panel(cx, w) {
                        e.scroll_list((dy / 18.0) as i32);
                    } else {
                        e.scroll_by(dx as i32, dy as i32, w, h);
                    }
                }
                self.show_editor();
            }
            // the battles' list scrolls a row a notch
            WindowEvent::MouseWheel { delta, .. } if matches!(self.shell, Some(shell::Screen::Selector { .. })) => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -(y.round()) as i32,
                    MouseScrollDelta::PixelDelta(p) => -(p.y / 22.0) as i32,
                };
                let (_, h) = self.window_px();
                if let Some(shell::Screen::Selector { entries, scroll }) = &mut self.shell {
                    let last = entries.len().saturating_sub(shell::selector_rows(h as i32));
                    *scroll = (*scroll as i32 + lines).clamp(0, last as i32) as usize;
                }
                self.show_shell();
            }
            WindowEvent::MouseWheel { delta, .. } if self.reader.is_some() => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -(y * 3.0) as i32,
                    MouseScrollDelta::PixelDelta(p) => -(p.y / 13.0) as i32,
                };
                let (w, h) = self.window_px();
                if let Some(r) = &mut self.reader {
                    let last = self.hud.reader_len(r, w, h).saturating_sub(hud::Hud::reader_rows(w, h));
                    r.scroll = (r.scroll as i32 + lines).clamp(0, last as i32) as usize;
                }
                self.update_hud();
                self.redraw();
            }
            WindowEvent::MouseWheel { delta, .. } if self.army.is_some() => {
                // the army details' list scrolls a line at a time
                let down = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y < 0.0,
                    MouseScrollDelta::PixelDelta(p) => p.y < 0.0,
                };
                if let (Some((side, scroll)), Some(p)) = (self.army, &self.play) {
                    let lines = Self::army_details(&p.game, side, 0).rows.len();
                    let last = lines.saturating_sub(hud::ARMY_LINES);
                    self.army = Some((side, if down { (scroll + 1).min(last) } else { scroll.saturating_sub(1) }));
                    self.update_hud();
                    self.redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x * STEP, -y * STEP),
                    MouseScrollDelta::PixelDelta(p) => (-p.x as f32, -p.y as f32),
                };
                self.scroll(dx, dy);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.quit {
            event_loop.exit();
            return;
        }
        if self.editor.as_mut().is_some_and(|e| e.flush()) {
            self.show_editor();
        }
        if !self.script.is_empty() {
            if Instant::now() >= self.script_next {
                self.script_step();
                self.script_next = Instant::now() + std::time::Duration::from_secs(2);
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.script_next));
        }
        // an import under way: its progress, then its end
        if self.importing.is_some() {
            self.poll_import();
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(200)));
            return;
        }
        // the music goes on and the river murmurs (checked at least once a
        // second, below)
        self.sound.tick();
        // a toolbar button's tooltip after the mouse rests on it half a
        // second (its status line in the status bar)
        if let Some((tool, since, false)) = self.hover {
            if since.elapsed() >= std::time::Duration::from_millis(500) {
                let grid = self.scenes.get(self.level).is_some_and(|s| s.grid_on);
                if let Some((short, _long)) = self.hud.tip(tool, self.show_overview, grid) {
                    let at = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
                    *self.hud.tooltip.borrow_mut() = Some((short, at));
                    self.update_hud();
                    self.redraw();
                }
                self.hover = Some((tool, since, true));
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(since + std::time::Duration::from_millis(500)));
            }
        }
        // the campaign map's provinces changing hands dissolve
        if let Some(m) = &mut self.campaign_map
            && m.advance()
        {
            let next = m.changes.front().map(|&(p, _)| p);
            if let (Some(p), Some(art)) = (next, &self.campaign_art) {
                let s = art.centre_on(p);
                if let Some(m) = &mut self.campaign_map {
                    m.scroll = s;
                }
            }
            self.update_hud();
            self.redraw();
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(40)));
            return;
        }
        // the intro or the historical video plays on
        if let Some(s) = &mut self.slides {
            let changed = s.tick(&mut self.sound);
            if s.done {
                self.end_slides();
            } else if changed {
                let palette = s.palette(&self.base_palette);
                if let Some(g) = &self.gpu {
                    g.set_palette(&palette);
                }
                self.update_hud();
                self.redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(15)));
            return;
        }
        // the video plays on
        if let Some(v) = &mut self.video {
            if v.done && self.intro {
                self.end_intro();
                return;
            }
            if v.advance() {
                self.update_hud();
                self.redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(30)));
            return;
        }
        // the other player's messages
        if self.net.is_some() {
            self.poll_net();
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(100)));
        }
        // the mouse at the battlefield's edge scrolls it
        if let Some((dx, dy)) = self.edge {
            const EDGE_STEP: f32 = 16.0;
            self.scroll(dx * EDGE_STEP * self.scale, dy * EDGE_STEP * self.scale);
            if self.playing.is_none() && self.runners.is_empty() {
                event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(30)));
                return;
            }
        }
        // once the fighting has been shown, the combat results come up
        if self.results.is_none() && self.playing.is_none() && self.queued.is_empty() && self.play.as_ref().is_some_and(|p| !p.combats.is_empty() && !p.demo) {
            if !self.prefs.overall_combat_results {
                if let Some(p) = &mut self.play {
                    p.combats.clear();
                }
                return;
            }
            self.results = Some((None, None));
            self.update_hud();
            self.redraw();
        }
        // once the last moves have been shown, the end of the battle
        if !self.ended
            && self.results.is_none()
            && self.playing.is_none()
            && self.queued.is_empty()
            && self.play.as_ref().is_some_and(|p| p.over && p.combats.is_empty())
        {
            self.ended = true;
            self.ending = true;
            self.update_hud();
            self.redraw();
        }
        // and after them, at the end of a turn, the game overview
        if !self.game_overview
            && self.results.is_none()
            && self.playing.is_none()
            && self.queued.is_empty()
            && self.play.as_ref().is_some_and(|p| p.turn_ended && p.combats.is_empty() && !p.demo)
        {
            self.game_overview = true;
            self.update_hud();
            self.redraw();
        }
        // a walking unit: redraw until it arrives
        // demo: when the animations are done, play the next computer leader
        if self.playing.is_none() && self.queued.is_empty() && self.play.as_ref().is_some_and(|p| p.demo) && self.next_step <= Instant::now() {
            if self.play.as_mut().is_some_and(|p| p.step_computer()) {
                self.changed();
            }
            self.next_step = Instant::now() + std::time::Duration::from_millis(400);
        }
        // loose horses and runaway chariots run on until off the map
        self.runners.retain(|(_, path, since)| since.elapsed().as_secs_f32() / RUN_HEX < path.len() as f32);
        if self.playing.is_some() || !self.runners.is_empty() {
            self.loose_runaways();
            let done = self.playing.as_ref().is_some_and(|(_, start, _, total)| start.elapsed().as_secs_f32() >= *total);
            if done {
                // a fight's men are down for good; then the next animation
                if let (Some((anim @ (play::Anim::Fight { .. } | play::Anim::Drop { .. } | play::Anim::Volley { .. }), ..)), Some(p)) =
                    (self.playing.take(), self.play.as_mut())
                {
                    p.land(&anim);
                }
                self.loosed.clear();
                self.playing = self.next_anim();
            }
            self.stale = [true; 3];
            self.refresh();
            self.redraw();
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(40)));
            return;
        }
        // the active leader's flags wave (the sprite layer's 10 cels a second)
        let flags_up = self.setup.is_none() && self.play.as_ref().is_some_and(|p| !p.flagged().is_empty());
        if self.animate && flags_up && Instant::now() >= self.next_flag {
            self.next_flag = Instant::now() + std::time::Duration::from_millis(gboh_render::units::FLAG_CEL_MS);
            for s in self.scenes.iter_mut() {
                s.flag_frame = s.flag_frame.wrapping_add(1);
            }
            self.stale[self.level] = true;
            self.refresh();
            self.redraw();
        }
        if !self.animate {
            let wait = if self.net.is_some() { 100 } else { 1000 };
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(wait)));
            return;
        }
        let now = Instant::now();
        if now >= self.next_frame {
            self.water_frame = (self.water_frame + 1) % 4;
            self.next_frame = now + WATER_FRAME;
            self.upload_palette();
            self.redraw();
        }
        let wake = if flags_up { self.next_frame.min(self.next_flag) } else { self.next_frame };
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}
