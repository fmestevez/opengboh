//! Network play between two copies of the rewrite. The original went
//! through DirectPlay (IPX, TCP, modem or serial) and proxied its message
//! bus; the rewrite's play is deterministic, so the two machines only
//! exchange the players' orders and run the same battle. One TCP
//! connection, a line of text per message:
//!
//! ```text
//! hello <name>                        joining player, first
//! game <scenario> <home level> <visitor level> <home> <visitor> <host name>
//!                                     the host's answer: the battle, and
//!                                     which armies the joining player
//!                                     leads (1 or 0 each)
//! o <order>                           an order, as in saved games
//! chat <text>                         a line of chat
//! ```
//!
//! The host chooses who leads which army on the Leader Control screen: the
//! sides marked Player are the host's, those marked Computer the other
//! player's (🟡 the original assigned players leader by leader,
//! `NETCHOOSELEADERS`).
//! The dialogs are the original's `STARTSERVER` (name, port 5000) and
//! `STARTCLIENT` (name, server, port).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, channel};

/// The port the original's server dialog offers.
pub const PORT: u16 = 5000;

/// A step of setting up a network game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Create or join (the original's "Create New Game" and "Locate
    /// Remote Game", strings 204 and 205).
    Choose,
    Host,
    Join,
}

/// A message from the other machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Hello(String),
    Game { scenario: String, levels: [u16; 2], joiner: [bool; 2], host: String },
    Order(crate::play::Order),
    Chat(String),
    /// The rule options the next game is played by (sent before it).
    Options(gboh_game::options::Options),
    /// The connection is gone.
    Closed,
}

impl Message {
    pub fn to_line(&self) -> String {
        match self {
            Message::Hello(n) => format!("hello {n}"),
            Message::Game { scenario, levels, joiner, host } => {
                format!("game {scenario} {} {} {} {} {host}", levels[0], levels[1], joiner[0] as u8, joiner[1] as u8)
            }
            Message::Order(o) => format!("o {}", crate::saves::order_line(o)),
            Message::Chat(t) => format!("chat {t}"),
            Message::Options(o) => format!("options {}", o.to_text()),
            Message::Closed => String::new(),
        }
    }

    pub fn parse(line: &str) -> Option<Self> {
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        Some(match word {
            "hello" => Message::Hello(rest.to_string()),
            "game" => {
                let mut w = rest.splitn(6, ' ');
                let scenario = w.next()?.to_string();
                let levels = [w.next()?.parse().ok()?, w.next()?.parse().ok()?];
                let joiner = [w.next()? == "1", w.next()? == "1"];
                Message::Game { scenario, levels, joiner, host: w.next().unwrap_or("").to_string() }
            }
            "o" => Message::Order(crate::saves::parse_order(rest).ok()?),
            "chat" => Message::Chat(rest.to_string()),
            "options" => Message::Options(gboh_game::options::Options::parse(rest)),
            _ => return None,
        })
    }
}

/// One end of the connection: lines are read on a thread of their own.
pub struct Link {
    stream: TcpStream,
    incoming: Receiver<Message>,
}

impl Link {
    fn new(stream: TcpStream) -> std::io::Result<Self> {
        let reader = stream.try_clone()?;
        let (tx, incoming) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                if let Some(m) = Message::parse(line.trim_end())
                    && tx.send(m).is_err()
                {
                    return;
                }
            }
            let _ = tx.send(Message::Closed);
        });
        Ok(Self { stream, incoming })
    }

    /// Connect to a host.
    pub fn connect(server: &str, port: u16) -> std::io::Result<Self> {
        Self::new(TcpStream::connect((server, port))?)
    }

    pub fn send(&mut self, m: &Message) -> std::io::Result<()> {
        writeln!(self.stream, "{}", m.to_line())
    }

    /// The messages that have come in.
    pub fn poll(&self) -> Vec<Message> {
        self.incoming.try_iter().collect()
    }
}

impl Drop for Link {
    /// Close the connection (the reader thread holds a copy of it).
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// A host waiting for the other player: accepts on a thread of its own.
pub struct Waiting {
    pub port: u16,
    accepted: Receiver<std::io::Result<TcpStream>>,
}

impl Waiting {
    pub fn listen(port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        let port = listener.local_addr()?.port();
        let (tx, accepted) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(listener.accept().map(|(s, _)| s));
        });
        Ok(Self { port, accepted })
    }

    /// The other player's connection, once made.
    pub fn poll(&self) -> Option<std::io::Result<Link>> {
        self.accepted.try_recv().ok().map(|r| r.and_then(Link::new))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::Order;
    use gboh_game::hex::Hex;

    #[test]
    fn talks_over_tcp() {
        for m in [
            Message::Hello("Porus".into()),
            Message::Game { scenario: "HYDASPES.SCN".into(), levels: [80, 70], joiner: [false, true], host: "Alexander the Great".into() },
            Message::Order(Order::Click { hex: Hex::new(3, 4), right: true }),
            Message::Chat("ready when you are".into()),
            Message::Options(gboh_game::options::Options { pursuit: true, ..Default::default() }),
        ] {
            assert_eq!(Message::parse(&m.to_line()), Some(m.clone()));
        }
        let waiting = Waiting::listen(0).unwrap();
        let mut client = Link::connect("127.0.0.1", waiting.port).unwrap();
        let mut host = loop {
            if let Some(l) = waiting.poll() {
                break l.unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        client.send(&Message::Hello("Porus".into())).unwrap();
        host.send(&Message::Order(Order::Finish { momentum: true })).unwrap();
        let wait = |l: &Link| loop {
            let got = l.poll();
            if !got.is_empty() {
                break got;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(wait(&host), vec![Message::Hello("Porus".into())]);
        assert_eq!(wait(&client), vec![Message::Order(Order::Finish { momentum: true })]);
        drop(host);
        assert_eq!(wait(&client), vec![Message::Closed]);
    }
}
