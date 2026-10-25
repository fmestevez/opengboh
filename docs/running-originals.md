# Running the original games

How to run the original *Great Battles of Alexander*, *Hannibal* and
*Caesar* on a current system, worked out while comparing opengboh with
them. Tested with the GOG releases under Wine 11 on macOS (Apple silicon);
the same settings should apply to Wine on Linux. On Windows the GOG
installers set all of this up themselves.

## What the games need

All three read their installation from the Windows registry, under
`HKEY_LOCAL_MACHINE\SOFTWARE\I-Magic\<Game>` (`Alexander`, `Hannibal`,
`Caesar`). A copy of the game folder that is simply dropped into a Wine
prefix has none of these keys, and the games then fail in confusing ways:

| value | type | meaning |
|---|---|---|
| `Install` | DWORD | the kind of installation. **Hannibal and Caesar accept only 0 or 1**: any other value makes them stop at start-up on an error box that, under Wine, never appears on screen (the process just sits there). Alexander works with 2. |
| `Path` | string | the game's data folder, e.g. `C:\GOG\Caesar\data` |
| `CDDataPath` | string | the same folder without the drive, e.g. `GOG\Caesar\data` |
| `CDTrackOffset` | DWORD | 0 |
| `Palette` | string | the palette file in the data folder: `GAME.PAL` (Alexander), `HANNIBAL.PAL`, `CAESAR.PAL`. **Without it the screen stays black**: the game builds an all-black palette and maps every picture to the 20 Windows system colours. |

32-bit programs on a 64-bit Wine prefix may look under `Wow6432Node`, so
set the same values under `HKEY_LOCAL_MACHINE\SOFTWARE\Wow6432Node\I-Magic\<Game>`
too. Caesar also reads some of them from `HKEY_CURRENT_USER`; setting them
there as well does no harm.

Optional, under `HKEY_CURRENT_USER\SOFTWARE\I-Magic\<Game>\Preferences`:
`Show Intro Video` (DWORD 0 skips the intro).

## Step by step (Wine)

1. Make a prefix and copy the installed game into it (leave your original
   installation alone):

   ```sh
   export WINEPREFIX=$HOME/gboh-wine
   wineboot -i
   mkdir -p "$WINEPREFIX/drive_c/GOG"
   cp -R "/path/to/GOG Games/Caesar" "$WINEPREFIX/drive_c/GOG/Caesar"
   ```

2. Write a `.reg` file for the game (here Caesar; for the others change
   the key name, the folder and the palette, and use `Install`=2 for
   Alexander if 1 doesn't work):

   ```
   REGEDIT4

   [HKEY_LOCAL_MACHINE\Software\I-Magic\Caesar]
   "Install"=dword:00000001
   "Path"="C:\\GOG\\Caesar\\data"
   "CDDataPath"="GOG\\Caesar\\data"
   "CDTrackOffset"=dword:00000000
   "Palette"="CAESAR.PAL"

   [HKEY_LOCAL_MACHINE\Software\Wow6432Node\I-Magic\Caesar]
   "Install"=dword:00000001
   "Path"="C:\\GOG\\Caesar\\data"
   "CDDataPath"="GOG\\Caesar\\data"
   "CDTrackOffset"=dword:00000000
   "Palette"="CAESAR.PAL"

   [HKEY_CURRENT_USER\Software\I-Magic\Caesar]
   "Install"=dword:00000001
   "Path"="C:\\GOG\\Caesar\\data"
   "CDDataPath"="GOG\\Caesar\\data"
   "CDTrackOffset"=dword:00000000
   "Palette"="CAESAR.PAL"

   [HKEY_CURRENT_USER\Software\I-Magic\Caesar\Preferences]
   "Show Intro Video"=dword:00000000
   ```

   and import it: `wine regedit caesar.reg`.

3. Run the game from its folder:

   ```sh
   cd "$WINEPREFIX/drive_c/GOG/Caesar"
   WINEDLLOVERRIDES=winecoreaudio.drv=d wine caesar.exe
   ```

   (`alex.exe`, `HANNIBAL.EXE` for the others.)

## Known problems under Wine

- **Crash when a battle starts (macOS):** Wine's CoreAudio driver fails an
  assertion. `WINEDLLOVERRIDES=winecoreaudio.drv=d` turns Wine's sound off,
  which avoids it (the games then run silent).
- **WinG:** the games ship `WING32.DLL`, a 16-bit thunk that can't load in
  a 64-bit prefix. Wine's own WinG is used automatically and works once the
  palette loads.
- **Colours:** on a true-colour desktop the animated water doesn't animate
  (the games cycle palette entries, which only shows in 256-colour modes).
- **Help:** the GOG builds replace every help call with a box pointing to
  the manual (the PDF in the game folder).
- **A hung start-up with no window** almost always means a missing or
  out-of-range registry value (see `Install` and `Palette` above). The
  error box is there but invisible; `winedbg` attached to the process shows
  the main thread waiting in a dialog.
