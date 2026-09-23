# Palette

An always-on-top mini music player for Windows. Apple Music's own miniplayer minimizes every time you click away, so I built one that doesn't.

Palette controls any player that talks to the Windows system media API (GSMTC). That includes Apple Music, Spotify, and browsers. It shows synced lyrics with word-level karaoke, picks up accent colors from the album art, and has a small visualizer that moves with the song.

## Features

- **Works with any player.** Now-playing info and transport controls for Apple Music, Spotify, and browsers over GSMTC, with no per-app setup.
- **Three sizes.** A pill that expands to a card, then to a full lyrics view, then to a fullscreen focus mode.
- **Synced lyrics** from [LRCLIB](https://lrclib.net). Click a line to seek, with a countdown through instrumental breaks.
- **Word karaoke.** After you listen to a song once, Palette aligns its lyrics to the audio on your machine and highlights each word as it's sung. Saved syncs are listed in the lyric view, where you can refresh or delete them.
- **Album-art accents.** Progress fills and the visualizer take their color from the cover. The rest of the UI stays neutral.
- **A visualizer that follows the song.** It listens to the playing app only, so Discord or game audio doesn't move it.
- **±10s seek** where the player supports it (see [player support](#player-support)).
- **Free placement.** Drag it anywhere. Drop it near a screen edge and it lines up with that edge.
- **Search** (`Ctrl+Alt+S`) to play, queue, or dig up tracks from your history.
- **Up-next queue, play history, and "more like this"** discovery (Spotify, optional).
- **Hide on fullscreen.** Palette gets out of the way of games and fullscreen video, then comes back where it was.
- **Rebindable global hotkeys** and self-updating installs.

## Install

Download `Palette_x.y.z_x64-setup.exe` from the [latest release](https://github.com/thientran01/palette/releases/latest) and run it. It installs per-user and doesn't need admin. Palette runs from the system tray, and the widget stays on top wherever you put it.

The installer is unsigned, so the first run shows a Windows SmartScreen **"Windows protected your PC"** dialog. Click **More info**, then **Run anyway**. You only see it once.

## Hotkeys

| Shortcut | Action |
|---|---|
| `Ctrl+Alt+K` | Play / pause |
| `Ctrl+Alt+←` / `Ctrl+Alt+→` | Seek −10s / +10s |
| `Ctrl+Alt+N` / `Ctrl+Alt+P` | Next / previous track |
| `Ctrl+Alt+M` | Show / hide the widget |
| `Ctrl+Alt+S` | Search |
| `Ctrl+Alt+[` / `Ctrl+Alt+]` | Nudge karaoke words earlier / later |

You can rebind all of these in **Preferences → Hotkeys**. Transport commands go to whichever media session Windows treats as current, usually the app that played most recently.

## Tray menu

Show / Hide · Reset position · Preferences… · Shortcuts / Help · Start at login · Hide on fullscreen · Connect Spotify · Check for updates · Open logs · Quit Palette

## Player support

Everything below was measured, not assumed. The full results are in [docs/smtc-support-matrix.md](docs/smtc-support-matrix.md).

- **Spotify** supports seek over GSMTC. A ±10s jump lands in about 50ms.
- **Apple Music** ignores every programmatic seek method I found, so its seek buttons are turned off and the progress bar is display-only. Position, lyrics, and karaoke still work.
- **Apple Music lossless** can take the output device in WASAPI exclusive mode. Nothing on the system can capture that audio, so the visualizer stays flat and karaoke can't learn the song. To fix it, open Windows Sound settings, go to your output device's Advanced tab, and untick "Allow apps to take exclusive control of this device". Playback is no longer bit-perfect after that (finding 12 in the matrix).

## Spotify and Last.fm (optional)

The core player needs no accounts: now-playing, transport, seek, lyrics, karaoke, and the visualizer all run off GSMTC.

Connecting Spotify from the tray adds Search's play and queue actions, the managed up-next queue, play-now from history, and "more like this". Two caveats:

- The Spotify app is in Development Mode, so only accounts I've allow-listed can connect.
- "More like this" also needs your own [Last.fm API key](https://www.last.fm/api/account/create), entered in **Preferences → Connectors**.

## Privacy

Palette has no analytics, no telemetry, and no account.

**Stored on your machine** in `%APPDATA%\com.thien.pulse`:

- preferences
- play history (`history.jsonl`, Apple Music and Spotify listens only)
- lyrics, thumbnail, and karaoke word-timing caches
- Spotify tokens, only if you connect. They're plain JSON protected by your Windows profile permissions, not OS encryption.
- research recordings, only if you start one from the lyric sync panel. Palette keeps the latest five.

Logs (`pulse.log`) and the WebView2 cache live in `%LOCALAPPDATA%\com.thien.pulse`. Release logs record when the widget hid for fullscreen, but not which app caused it.

Karaoke alignment runs locally. Song audio is held in memory while you listen, and only the word timings are saved.

**Sent over the network:** lyric lookups to [LRCLIB](https://lrclib.net), plus Spotify Web API and Last.fm requests if you opted into those. Nothing else.

**Clearing data:** **Preferences → Data → Clear play history** deletes history and its thumbnails. Uninstalling always deletes Spotify tokens. Everything else survives a reinstall unless you tick "Delete the application data" in the uninstaller.

## Build from source

You need Node 20+, Rust with the MSVC toolchain, and the Visual Studio Build Tools. Palette is Windows-only.

```sh
npm install
npm run tauri dev     # run the app
npm test              # frontend tests
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build   # installer → src-tauri/target/release/bundle/nsis/
```

`npm run tauri build` compiles and then fails at the bundling step unless `TAURI_SIGNING_PRIVATE_KEY` is set, because the config creates signed updater artifacts. You can fix that two ways:

- Generate your own keypair with `npx tauri signer generate` and export the private key as `TAURI_SIGNING_PRIVATE_KEY`.
- Set `createUpdaterArtifacts` to `false` in `src-tauri/tauri.conf.json` for a personal build without self-update.

Word karaoke works without extra setup using a timing-based aligner. The more accurate local acoustic model is a separate developer install (`scripts/install_karaoke_model.py`), and model weights aren't included in the repo or the installer.

## Stack

Tauri v2 (Rust), React 19, TypeScript, Vite, and Tailwind v4. WASAPI loopback and an FFT drive the visualizer, LRCLIB supplies lyrics, and a local forced aligner times the words.

## License

[MIT](LICENSE)
