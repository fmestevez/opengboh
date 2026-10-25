//! Network play: the host and join dialogs, the link, and the chat.

use crate::*;

impl Viewer {
    /// Multiplayer: create a game or join one.
    pub(crate) fn open_network(&mut self) {
        let res = crate::ui::exe_resources(&self.data_dir);
        let list = [204, 205].iter().map(|&id| res.as_ref().and_then(|r| r.string(id)).unwrap_or_default()).collect();
        self.plain = Some((hud::Plain { name: "ENUMDIALOG", list, selected: Some(0), ..Default::default() }, Asked::Network(net::Step::Choose)));
        self.update_hud();
        self.redraw();
    }

    /// A click on a network dialog.
    pub(crate) fn network_click(&mut self, d: &hud::Plain, at: Option<u16>, (w, h, cx, cy): (usize, usize, i32, i32)) {
        let Some((_, Asked::Network(step))) = self.plain.clone() else { return };
        match at {
            Some(1) => self.network_ok(step, d),
            Some(2) => {
                self.plain = None;
                self.net = None;
            }
            Some(101) if step == net::Step::Choose => {
                if let (Some(k), Some((p, _))) = (self.hud.plain_line_at(d, w, h, cx, cy), &mut self.plain) {
                    p.selected = Some(k);
                }
            }
            Some(id @ (101..=103)) => {
                if let Some((p, _)) = &mut self.plain {
                    p.focus = Some(id);
                }
            }
            Some(9) => self.open_help(),
            _ => {}
        }
        self.update_hud();
        self.redraw();
    }

    /// OK on a network dialog: on to hosting or joining; then listen, or
    /// connect.
    pub(crate) fn network_ok(&mut self, step: net::Step, d: &hud::Plain) {
        let get = |id: u16| d.texts.iter().find(|(k, _)| *k == id).map(|(_, t)| t.trim().to_string()).unwrap_or_default();
        let name = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
        match step {
            net::Step::Choose => {
                let (dialog, texts, next) = if d.selected == Some(1) {
                    ("STARTCLIENT", vec![(101, name), (102, "localhost".into()), (103, net::PORT.to_string())], net::Step::Join)
                } else {
                    ("STARTSERVER", vec![(101, name), (103, net::PORT.to_string())], net::Step::Host)
                };
                self.plain = Some((hud::Plain { name: dialog, texts, focus: Some(101), ..Default::default() }, Asked::Network(next)));
            }
            net::Step::Host => {
                let port = get(103).parse().unwrap_or(net::PORT);
                match net::Waiting::listen(port) {
                    Ok(waiting) => {
                        println!("waiting for the other player on port {}", waiting.port);
                        self.net = Some(NetGame {
                            link: None,
                            waiting: Some(waiting),
                            name: get(101),
                            other: String::new(),
                            chosen: None,
                            sent: 0,
                            mine: [true, false],
                        });
                        // the host chooses the battle meanwhile
                        self.plain = None;
                        if let Some(setup) = &mut self.setup {
                            setup.screen = setup::Screen::Scenario { selected: None };
                        }
                        self.show_setup();
                    }
                    Err(e) => {
                        eprintln!("network: {e}");
                        self.message(&[201]);
                    }
                }
            }
            net::Step::Join => {
                let port = get(103).parse().unwrap_or(net::PORT);
                match net::Link::connect(&get(102), port) {
                    Ok(mut link) => {
                        let _ = link.send(&net::Message::Hello(get(101)));
                        self.net =
                            Some(NetGame { link: Some(link), waiting: None, name: get(101), other: String::new(), chosen: None, sent: 0, mine: [false, true] });
                        // waiting for the host's battle
                        self.message(&[109]);
                    }
                    Err(e) => {
                        eprintln!("network: {e}");
                        self.message(&[216]);
                    }
                }
            }
        }
    }

    /// The other machine's messages: a connection made, the battle, orders,
    /// chat, or the connection lost.
    pub(crate) fn poll_net(&mut self) {
        let Some(n) = &mut self.net else { return };
        if let Some(r) = n.waiting.as_ref().and_then(|w| w.poll()) {
            n.waiting = None;
            match r {
                Ok(link) => n.link = Some(link),
                Err(e) => eprintln!("network: {e}"),
            }
            self.host_start();
        }
        let Some(n) = &mut self.net else { return };
        let messages = n.link.as_ref().map(|l| l.poll()).unwrap_or_default();
        for m in messages {
            match m {
                net::Message::Hello(name) => {
                    if let Some(n) = &mut self.net {
                        n.other = name;
                    }
                }
                net::Message::Options(o) => self.net_options = Some(o),
                net::Message::Game { scenario, levels, joiner, host } => {
                    self.net_start = self.net_options.take();
                    if let Some(n) = &mut self.net {
                        n.other = host;
                        n.mine = joiner;
                    }
                    self.plain = None;
                    if let Err(e) = self.start_battle(&scenario, [false; 2], levels) {
                        eprintln!("network: {e:#}");
                    }
                    if let Some(n) = &mut self.net {
                        n.sent = 0;
                    }
                }
                net::Message::Order(o) => {
                    if let Some(p) = &mut self.play {
                        p.apply(o);
                        if let Some(n) = &mut self.net {
                            n.sent = p.orders.len();
                        }
                    }
                    self.changed();
                }
                net::Message::Chat(line) => {
                    let who = self.net.as_ref().map(|n| n.other.clone()).unwrap_or_default();
                    if let Some(p) = &mut self.play {
                        p.commentary.push(format!("{who}: {line}"));
                    }
                    self.update_hud();
                    self.redraw();
                }
                net::Message::Closed => {
                    self.net = None;
                    self.message(&[219]);
                    return;
                }
            }
        }
    }

    /// The host has its battle and its opponent: send the battle and begin.
    pub(crate) fn host_start(&mut self) {
        let Some(n) = &mut self.net else { return };
        let (Some(link), Some((scenario, levels, joiner))) = (&mut n.link, n.chosen.clone()) else { return };
        // (the host's rule options, both sides alike)
        let _ = link.send(&net::Message::Options(self.settings.options));
        let _ = link.send(&net::Message::Game { scenario: scenario.clone(), levels, joiner, host: n.name.clone() });
        n.mine = joiner.map(|j| !j);
        n.sent = 0;
        self.plain = None;
        if let Err(e) = self.start_battle(&scenario, [false; 2], levels) {
            eprintln!("network: {e:#}");
        }
    }

    /// Send the orders given here since the last sent.
    pub(crate) fn send_orders(&mut self) {
        let (Some(n), Some(p)) = (&mut self.net, &self.play) else { return };
        let Some(link) = &mut n.link else { return };
        for o in p.orders.iter().skip(n.sent) {
            let _ = link.send(&net::Message::Order(*o));
        }
        n.sent = p.orders.len();
    }

    /// Whether the player here may give orders: always, but in a network
    /// game only while the active leader is of an army led here.
    pub(crate) fn my_turn(&self) -> bool {
        let (Some(n), Some(p)) = (&self.net, &self.play) else { return true };
        let Some(l) = p.leader() else { return false };
        let nation = p.game.battle.units[l].side;
        let army = if p.game.armies[0].0 == nation { 0 } else { 1 };
        n.mine[army]
    }

    /// The chat (`NETCHAT`): a line to send.
    pub(crate) fn open_chat(&mut self) {
        self.plain = Some((hud::Plain { name: "NETCHAT", focus: Some(107), ..Default::default() }, Asked::Chat));
    }

    pub(crate) fn send_chat(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        let Some(n) = &mut self.net else { return };
        if let Some(l) = &mut n.link {
            let _ = l.send(&net::Message::Chat(line.to_string()));
        }
        let me = n.name.clone();
        if let Some(p) = &mut self.play {
            p.commentary.push(format!("{me}: {line}"));
        }
    }
}
