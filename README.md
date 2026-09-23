# Palette

An always-on-top mini music player for Windows. Apple Music's miniplayer minimizes every time you click away, so I built one that doesn't.

It controls whatever is playing (Apple Music, Spotify, browsers) through the Windows media API, with synced lyrics, word-by-word karaoke, album-art accent colors, and a small visualizer that moves with the song.

## Install

Download the setup `.exe` from the [latest release](https://github.com/thientran01/palette/releases/latest). It installs per-user and updates itself. The installer is unsigned, so on first run click **More info → Run anyway** on the SmartScreen prompt.

## Hotkeys

| Shortcut | Action |
|---|---|
| `Ctrl+Alt+K` | Play / pause |
| `Ctrl+Alt+←` / `→` | Seek −10s / +10s |
| `Ctrl+Alt+N` / `P` | Next / previous |
| `Ctrl+Alt+M` | Show / hide |
| `Ctrl+Alt+S` | Search |
| `Ctrl+Alt+[` / `]` | Nudge karaoke words earlier / later |

Rebind them in **Preferences → Hotkeys**.

## Good to know

- Apple Music can't be seeked by other apps, so seek is off for it. Spotify seeks fine. Details: [docs/smtc-support-matrix.md](docs/smtc-support-matrix.md).
- If Apple Music lossless takes exclusive control of your audio device, the visualizer and karaoke go quiet. Untick "Allow apps to take exclusive control" in your output device's Windows sound settings.
- Search's play/queue, the up-next queue, and "more like this" need Spotify (connect from the tray). "More like this" also needs a [Last.fm API key](https://www.last.fm/api/account/create). The Spotify app is in development mode, so only allow-listed accounts can connect.

## Privacy

No analytics, no telemetry, no account. Settings, play history, and lyric/karaoke caches stay in `%APPDATA%\com.thien.pulse`. Karaoke timing is computed on your machine, and song audio is never saved or uploaded. The only network requests go to LRCLIB for lyrics, plus Spotify and Last.fm if you connect them. Clear history in **Preferences → Data**.

## Build from source

Requires Node 20+, Rust (MSVC), and the VS Build Tools.

```sh
npm install
npm run tauri dev
```

`npm run tauri build` needs a `TAURI_SIGNING_PRIVATE_KEY`. Generate one with `npx tauri signer generate`, or set `createUpdaterArtifacts` to `false` in `src-tauri/tauri.conf.json`.

## License

[MIT](LICENSE)
