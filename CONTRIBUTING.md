# Contributing to opengboh

Thanks for wanting to help. opengboh reimplements the engine of the
*Great Battles of History* computer games (Alexander, Hannibal, Caesar);
contributions of code, testing, documentation and research are welcome.

Please read the [code of conduct](CODE_OF_CONDUCT.md) first.

## The ground rules (legal)

opengboh is a clean-room reimplementation, and stays distributable only as
long as these hold:

- **No game data in the repository.** No sprites, maps, scenarios, sounds,
  music, videos, help files, palettes, or the games' text (unit names
  excepted where the code must match them). opengboh reads everything from
  the player's own copy at run time. Tests that need the data skip
  themselves when it isn't there. (The one exception: the few
  screenshots in `docs/screenshots`, which illustrate the README.)
- **No decompiled or disassembled code.** Don't paste or translate code
  from a decompiler, even "cleaned up". Describe what the original does in
  your own words (in `docs/formats.md` or a code comment) and write the
  Rust from that description. Addresses (`FUN_00554496`) are fine as
  references.
- **No proprietary documents.** Quote the manuals only in short excerpts,
  as reference.
- Contributions are licensed under the project's license,
  GPL-3.0-or-later (see `LICENSE`).

## Getting set up

You need a recent stable Rust toolchain and, to run or test against the
games, your own copy of at least one of them (the GOG releases work).

```sh
cargo build --release
cargo run --release -p gboh-viewer          # the launcher (content manager)
cargo test                                  # unit tests
```

The data tests run when these point at a game's data folder:

```sh
GBOH_DATA=/path/to/Alexander/DATA \
GBOH_HANNIBAL=/path/to/Hannibal/Data \
GBOH_CAESAR=/path/to/Caesar/data \
cargo test
```

`GBOH_SHOTS=/some/dir` makes the rendering tests write their images
there. `gboh-tools` (see the README) renders maps, lists scenarios and
runs battles under the computer (`ai`), handy for checking rule changes.

To compare with the original games, see
[docs/running-originals.md](docs/running-originals.md).

## Layout

| crate | what's in it |
|---|---|
| `gboh-formats` | the games' file formats: EIF sprites, maps, scenarios, palettes, the executables' resources |
| `gboh-render` | drawing the battlefield: terrain, objects, units, missiles |
| `gboh-game` | the rules: battle state, activation, movement, combat, AI scripts, campaigns, rule options |
| `gboh-viewer` | the program (`opengboh`): window, launcher, screens, battle interface, sound |
| `gboh-tools` | command-line tools |

`docs/formats.md` documents the formats and rules as the games implement
them; keep it in step with the code.

## Making changes

- **Faithful first.** The default is the games as they shipped, with
  plain bugs fixed. A change that alters the balance (a manual's rule, a
  later game's rule) goes behind a rule option (`gboh_game::options`).
  Say in a comment what the original does and where (`FUN_…`), and mark
  guesses with 🟡.
- **Write like the surrounding code:** its naming, its comment density,
  doc comments on public items. Keep the code game-agnostic where the
  three games share it; branch on the game or rules where they don't.
- Before sending: `cargo fmt`, `cargo clippy --all-targets` (no
  warnings), `cargo test` (with whatever game data you have).
- Commit messages: a short summary line (`area - what changed`), then what
  and why.
- For anything big, open an issue first to talk it over.

## Reporting bugs

Open an issue with what you did, what happened, what you expected, which
game and battle, and (if you can) a save from the game's Save menu: saves
replay exactly. Differences from the original games are bugs too; a
screenshot of the original (see the guide above) helps a lot.
