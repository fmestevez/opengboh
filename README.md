# opengboh

An open-source reimplementation, written in Rust, of the Erudite Software engine
behind the *Great Battles of History* (GBoH) series of computer games:

* *The Great Battles of Alexander* (1997): current focus
* *The Great Battles of Hannibal* (1997): planned
* *The Great Battles of Caesar* (1998): planned

**This repository contains no game data.** You need your own copy of a
supported game (e.g. from GOG); the tools here read the original files
from its `DATA` directory.

## Status

Early. Currently:

* `gboh-formats`: parsers for the original data (EIF sprites, palette, battle
  maps, scenarios).
* `gboh-render`: a faithful software renderer for the battlefield (terrain,
  streams, cliffs, roads, river rocks, trees, stones and units in formation),
  checked pixel by pixel against the original game.
* `gboh-game`: game rules, independent of rendering (so far: battle state,
  leader ratings, command, activation order, movement, turning,
  command points, shock and missile combat, routs, rally, rout points
  and a turn loop).
* `gboh-viewer`: the game's setup screens and battle window (wgpu), with
  animation, sound and the original's dialogs.
* `gboh-tools`: command-line utilities.

See [docs/formats.md](docs/formats.md) for format notes.

## Usage

```sh
cargo run --release -p gboh-tools -- extract "/path/to/game/DATA" out/eif
```

Writes every sprite cel as PNG plus a `manifest.json` (names, hotspots) per file.

```sh
cargo run --release -p gboh-tools -- render "/path/to/game/DATA" GRANICUS.SCN granicus.png normal
cargo run --release -p gboh-viewer -- "/path/to/game/DATA" GRANICUS.SCN
cargo run --release -p gboh-viewer -- "/path/to/game/DATA"
```

Without a scenario the viewer opens on the game's start screen: Start Battle,
then pick a battle on the map and who the computer plays. The screens use the
game's own art, and its text and layouts are read from `alex.exe` next to the
`DATA` directory.

`render` draws a battle map, or a scenario's map with its units, to a PNG (`in`, `normal` or `out` zoom). The viewer
opens the map in a window: arrows/WASD, drag or the mouse wheel to scroll,
1/2/3 to zoom in/normal/out, +/- for pixel scale, space to pause the water
animation, Esc to quit.

Given a scenario, the viewer also plays the battle hot seat (both sides by
hand, early and incomplete): the window title shows the turn, the active
leader and his command points. Click one of his flagged units to select it
and light its reachable hexes; click a lit hex to move, or an enemy in front
to designate a shock; right click an enemy to volley; Q/E turn the unit,
R rallies it, C recovers its cohesion; click the active leader and then a
lit hex to move him; G tries a group move (all his orders; a lit hex then
moves the whole group); Enter finishes the leader (his shocks are fought;
if he may, he is first asked whether to try for momentum), M finishes him
and tries for momentum. Sounds come from the game's own WAV
files. Add `ai` after the
scenario to have the computer play the visitor army with the original's AI
scripts, or `demo` to watch it play both armies (one leader at a time, once
the previous moves have been shown); set `GBOH_MUTE=1` to silence it; `gboh-tools ai <DATA> <SCN> <turns>` lets them play both sides.

The battle window follows the original's: its toolbar (finish leader,
centre on leader, turns, missile volley, recover cohesion, rally, unit
details, overview map, hex grid, reverse view, zoom), status bar, overview
map and running commentary; after the fighting, the combat results; at the
end of each turn, the game overview with each army's details; and at the
end, the victory or defeat screen.

## License

opengboh is free software: you can redistribute it and/or modify it under the
terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version. See [LICENSE](LICENSE).

## Legal

This is an independent reimplementation. It does not include or redistribute
any of the original games' code or assets. All game titles are trademarks of
their respective owners.
