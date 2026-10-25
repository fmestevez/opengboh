//! The campaigns: their map screen, its clicks and drags, and their battles.

use crate::*;

impl Viewer {
    /// The campaign map or results, while up, over a clear overlay (the
    /// results not while the video plays).
    pub(crate) fn campaign_overlay(&self, w: usize, h: usize) -> Option<Canvas> {
        let art = self.campaign_art.as_ref()?;
        if self.video.is_some() {
            return None;
        }
        if let Some(r) = &self.campaign_results {
            return Some(art.draw_results(r, w, h));
        }
        Some(art.draw_map(self.campaign.as_ref()?, self.campaign_map.as_ref()?, w, h))
    }

    /// Start Campaign: a new campaign opens on its map (Macedon, its
    /// battles queued).
    pub(crate) fn start_campaign(&mut self) {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_secs() as u32);
        let mut c = match gboh_formats::game::Game::detect(&self.data_dir) {
            Some(gboh_formats::game::Game::Hannibal) => gboh_game::campaign::Campaign::new_hannibal(seed),
            Some(gboh_formats::game::Game::Caesar) => gboh_game::campaign::Campaign::new_caesar(seed),
            _ => gboh_game::campaign::Campaign::new(seed),
        };
        let next = c.after_battle(true);
        self.campaign = Some(c);
        self.campaign_next(next);
    }

    /// Go on with the campaign: a battle, the map for a new round, or the
    /// results.
    pub(crate) fn campaign_next(&mut self, next: gboh_game::campaign::Next) {
        use gboh_game::campaign::Next;
        self.campaign_art();
        match next {
            Next::Battle(b) => {
                self.campaign_map = None;
                if let Err(e) = self.campaign_battle(b) {
                    eprintln!("campaign battle: {e:#}");
                    self.campaign = None;
                }
            }
            Next::Map => {
                let names = self.campaign_art().province_names.clone();
                let Some(c) = &mut self.campaign else { return };
                let events = c.opening();
                let first = c.round == 1;
                let over = c.round > gboh_game::campaign::ROUNDS;
                let kind = c.kind;
                // (Caesar's messages are the game's own text)
                let strings = crate::ui::exe_resources(&self.data_dir);
                let word = |e: &gboh_game::campaign::Event| match e {
                    gboh_game::campaign::Event::Message(id) => strings.as_ref().and_then(|r| r.string(*id)).unwrap_or_default(),
                    _ => campaign::describe(e, &names, kind),
                };
                let mut log: Vec<String> = c.log.iter().map(word).filter(|l| !l.is_empty()).collect();
                if log.is_empty() {
                    log = events.iter().map(word).filter(|l| !l.is_empty()).collect();
                }
                // the first round opens with Battle (Macedon's battles are
                // queued), the others with Subjugate until the army moves
                // the provinces whose hold changed dissolve in turn, the
                // map scrolled to the first
                let changes: std::collections::VecDeque<(usize, u8)> = events
                    .iter()
                    .filter_map(|e| match e {
                        gboh_game::campaign::Event::Subjugation { province, level, rose } => Some((*province, if *rose { level - 1 } else { level + 1 })),
                        _ => None,
                    })
                    .collect();
                // what to do next, in the rewrite's words (the original
                // left it to its help)
                if !over {
                    log.push(match kind {
                        gboh_game::campaign::Kind::Alexander if first => "Press Battle to fight for Macedon.".into(),
                        gboh_game::campaign::Kind::Caesar => "Drag Caesar's flag to march on a province, then press Finish.".into(),
                        _ => "Drag the army's flag to march on a province, or press Subjugate to stay.".into(),
                    });
                }
                let scroll = changes.front().map_or(0, |&(p, _)| self.campaign_art().centre_on(p));
                // (Hannibal's map always opens on Subjugate)
                // (Caesar's Finish is always there)
                let first = (first && kind == gboh_game::campaign::Kind::Alexander) || kind == gboh_game::campaign::Kind::Caesar;
                let here = self.campaign.as_ref().map_or(0, |c| c.current);
                let scroll = if kind == gboh_game::campaign::Kind::Caesar { self.campaign_art().centre_on(here) } else { scroll };
                self.campaign_map = Some(campaign::MapState { battle: first, subjugate: !first, log, changes, scroll, ..Default::default() });
                if over {
                    let next = self.campaign.as_mut().map(|c| c.end_round());
                    if let Some(n) = next {
                        return self.campaign_next(n);
                    }
                }
            }
            Next::Results => {
                self.campaign_map = None;
                self.campaign_results = self.campaign.take().map(|c| c.results());
                // victory: first the video (`VideoDlg`), the music paused
                if let Some(r) = self.campaign_results {
                    // (Hannibal's has no video)
                    let alexander = self.campaign_art.as_ref().is_some_and(|a| a.kind() == gboh_game::campaign::Kind::Alexander);
                    let video = if r.victory && alexander {
                        find_ci(&self.data_dir, "320ALEX.AVI").and_then(|p| video::Video::open(&p, &self.base_palette))
                    } else {
                        None
                    };
                    match video {
                        Some(mut v) => {
                            self.sound.set_music(sound::Music::Silent);
                            if let Some(wav) = v.sound.take() {
                                self.sound.play_data(wav);
                            }
                            self.video = Some(v);
                            self.plain = Some((hud::Plain { name: "VIDEODLG", ..Default::default() }, Asked::Video));
                        }
                        None => self.sound.set_music(sound::Music::End { won: r.victory }),
                    }
                }
            }
        }
        self.update_hud();
        self.redraw();
    }

    /// Fight campaign battle `b` (scenario table index): the player leads
    /// the Macedonians, the computer the enemy, at the scenario's rout
    /// levels.
    pub(crate) fn campaign_battle(&mut self, b: usize) -> Result<()> {
        let (file, player) = match &self.campaign {
            Some(c) if c.kind == gboh_game::campaign::Kind::Hannibal => (gboh_game::campaign::HANNIBAL_BATTLES[b], c.player_nation()),
            Some(c) if c.kind == gboh_game::campaign::Kind::Caesar => (gboh_game::campaign::CAESAR_BATTLES[b], c.player_nation()),
            _ => (setup::SCENARIOS[b], MACEDONIAN),
        };
        let data = find_ci(&self.data_dir, file).and_then(|p| std::fs::read(p).ok()).context("scenario not found")?;
        let scn = Scenario::parse(&data)?;
        let computer = [scn.home.nation != player, scn.visitor.nation != player];
        let levels = [scn.home.rout_level, scn.visitor.rout_level];
        let campaign = self.campaign.take();
        let result = self.start_battle(file, computer, levels);
        self.campaign = campaign;
        self.campaign_penalty();
        result
    }

    /// Caesar's campaign: his troops' quality lowered by the riots and
    /// hunger met on the way (`GetCaesarianPenalty`; at least 1).
    pub(crate) fn campaign_penalty(&mut self) {
        let (Some(c), Some(p)) = (&self.campaign, &mut self.play) else { return };
        if c.kind != gboh_game::campaign::Kind::Caesar || c.penalty == 0 {
            return;
        }
        for u in p.game.battle.units.iter_mut().filter(|u| u.nation == 9 && u.leader.is_none()) {
            u.tq = u.tq.saturating_sub(c.penalty).max(1);
        }
    }

    /// A button of the campaign map.
    pub(crate) fn campaign_button(&mut self, at: Option<u16>) {
        let Some(m) = &self.campaign_map else { return };
        match at {
            Some(10) if m.battle => {
                if let Some(c) = &mut self.campaign {
                    c.subjugate = false;
                }
                self.campaign_end_round()
            }
            Some(11) if m.subjugate => {
                if let Some(c) = &mut self.campaign {
                    c.subjugate = true;
                }
                self.campaign_end_round()
            }
            Some(2) => {
                self.campaign = None;
                self.campaign_map = None;
                self.sound.set_music(sound::Music::Start);
            }
            Some(9) => {
                let topic = self.help().and_then(|h| h.topics.iter().position(|t| t.title.starts_with("Campaign")));
                if let Some(k) = topic {
                    self.open_topic(k);
                }
            }
            _ => {}
        }
        self.update_hud();
        self.redraw();
    }

    pub(crate) fn campaign_end_round(&mut self) {
        if let Some(next) = self.campaign.as_mut().map(|c| c.end_round()) {
            self.campaign_next(next);
        }
    }

    /// The left button on the campaign map: a press picks up the flag, an
    /// icon or the map; a release drops it. Returns whether it was the map's.
    pub(crate) fn map_drag(&mut self, pressed: bool) -> bool {
        if self.campaign_map.is_none() || self.plain.is_some() || self.reader.is_some() || self.setup.is_none() {
            return false;
        }
        let (w, h) = self.window_px();
        let (cx, cy) = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
        let (Some(art), Some(c), Some(m)) = (&self.campaign_art, &self.campaign, &self.campaign_map) else { return false };
        if pressed {
            // a double-click splits or merges a garrison icon (once the
            // opening's changes are over)
            let double = self.map_press.is_some_and(|(t, (px, py))| t.elapsed().as_millis() < 400 && (px - cx).abs() < 5 && (py - cy).abs() < 5);
            self.map_press = Some((std::time::Instant::now(), (cx, cy)));
            if double
                && m.changes.is_empty()
                && let Some(m) = &mut self.campaign_map
                && art.split(c, m, w, h, cx, cy)
            {
                self.map_press = None;
                self.update_hud();
                self.redraw();
                return true;
            }
            let (Some(art), Some(c), Some(m)) = (&self.campaign_art, &self.campaign, &self.campaign_map) else { return false };
            let Some(what) = art.press(c, m, w, h, cx, cy) else { return false };
            let at = art.to_dialog(w, h, cx, cy);
            if let Some(m) = &mut self.campaign_map {
                m.dragging = Some((what, at));
            }
            self.update_hud();
            self.redraw();
            return true;
        }
        let Some((what, _)) = m.dragging else { return false };
        let at = art.to_dialog(w, h, cx, cy);
        let target = art.drop_target(c, m, at.0, at.1);
        if let (Some(c), Some(m)) = (&mut self.campaign, &mut self.campaign_map) {
            match (what, target) {
                (campaign::Dragging::Flag, Some(p)) => {
                    if c.march(p) {
                        // battles to fight: Battle; none: Subjugate
                        m.battle = c.battle_offered();
                        m.subjugate = !m.battle;
                        let name = art.province_names.get(p).cloned().unwrap_or_default();
                        m.log.push(match c.kind {
                            gboh_game::campaign::Kind::Caesar if c.battle_offered() => format!("Press Finish to fight the Pompeians in {name}."),
                            gboh_game::campaign::Kind::Caesar => format!("Press Finish to march into {name}."),
                            _ if m.battle => format!("Press Battle to fight for {name}."),
                            _ => format!("Press Subjugate to hold {name} this round."),
                        });
                        if c.kind == gboh_game::campaign::Kind::Caesar {
                            (m.battle, m.subjugate) = (true, false);
                        }
                    }
                }
                // (both garrisons regroup)
                (campaign::Dragging::Unit { from, kind, value }, Some(p)) if c.move_units(from, p, kind, value) => {
                    for q in [from, p] {
                        m.split.remove(&(q, kind));
                    }
                }
                _ => {}
            }
            m.dragging = None;
            m.hover = None;
        }
        self.update_hud();
        self.redraw();
        true
    }

    /// The mouse moving with something picked up on the campaign map.
    pub(crate) fn map_move(&mut self) {
        let (w, h) = self.window_px();
        let (cx, cy) = ((self.cursor.x as f32 / self.scale) as i32, (self.cursor.y as f32 / self.scale) as i32);
        let (Some(art), Some(c), Some(m)) = (&self.campaign_art, &self.campaign, &mut self.campaign_map) else { return };
        let at = art.to_dialog(w, h, cx, cy);
        let max = art.max_scroll();
        if let Some((what, pos)) = &mut m.dragging {
            *pos = at;
            match *what {
                campaign::Dragging::Map { scroll, x } => m.scroll = (scroll + x - at.0).clamp(0, max),
                // the map scrolls under a dragged piece near its sides
                _ => {
                    if at.0 < 35 + 15 {
                        m.scroll = (m.scroll - 20).max(0);
                    } else if at.0 >= 35 + 529 - 15 {
                        m.scroll = (m.scroll + 20).min(max);
                    }
                }
            }
        }
        m.hover = art.drop_target(c, m, at.0, at.1);
        self.update_hud();
        self.redraw();
    }

    // --- network play ---
}
