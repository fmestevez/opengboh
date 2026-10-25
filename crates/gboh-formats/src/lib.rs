//! Readers for the data files of Erudite's *Great Battles of History* games
//! (verified against *The Great Battles of Alexander* so far).
//!
//! This crate contains no game data; it parses files from a user's own copy.

pub mod crt;
pub mod director;
pub mod eif;
pub mod game;
pub mod hlp;
pub mod map;
pub mod pal;
pub mod pe;
pub mod rtf;
pub mod scn;
pub mod units;
pub mod walkanim;
mod read;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unexpected end of data at offset {0:#x}")]
    Eof(usize),
    #[error("invalid data at offset {offset:#x}: {msg}")]
    Invalid { offset: usize, msg: String },
}

pub type Result<T> = std::result::Result<T, Error>;
