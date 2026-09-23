# Palette

Always-on-top mini music player for Windows. It reads and controls whatever is playing (Apple Music, Spotify, browsers) through the Windows system media API (GSMTC), with synced lyrics, local word karaoke, album-art accents, and an audio-reactive separator. Personal project by Thien (designer). User-facing overview: `README.md`. Release history: `CHANGELOG.md`.

This file lists the invariants that aren't obvious from the code. Design history and the reasoning behind each decision live in PR descriptions, `docs/specs/`, and module doc comments. Read those before reopening a settled decision.

## Stack

- Tauri v2 (Rust): one frameless, transparent, always-on-top main window plus Search, Focus, and Preferences webviews.
- React 19, TypeScript, Vite 7, Tailwind v4 (CSS-first config), `motion`.
- Windows-only: GSMTC, WASAPI, Win32. The Rust side doesn't compile on other platforms, so CI runs on `windows-latest`.

## Commands

```sh
npm run dev            # browser mock of the frontend (no Tauri)
npm run tauri dev      # the real app (Rust MSVC toolchain + VS Build Tools)
npm run build          # tsc + vite build (must run before cargo: tauri embeds dist/)
npm test               # vitest
cargo test   --manifest-path src-tauri/Cargo.toml --all-targets
cargo fmt    --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
python -m unittest discover -s scripts -p "test_karaoke_*.py"
```

CI (`.github/workflows/ci.yml`) runs all of the above on every PR. fmt and clippy are hard gates.

Browser mock flags (`src/lib/backend.ts`, `src/main.tsx`): `?am` replays Apple Music's messy emit profile (1s-floored positions, `can_seek=false`), `?nothing` forces the no-session state, `?spotify=off` shows the queue gate, `?jump=partial` shows the jump-failure caption, `?lab` opens the icon sequencer, and `?window=search|focus|prefs` renders another window. `.cursor/skills/verify-pulse` drives the mock to verify UI behavior.

## Map

`src-tauri/src/`:

| Module | Owns |
|---|---|
| `lib.rs` | Setup, tray, global hotkeys (`HK_*` defaults + rebind), `VisIntent` / `apply_visibility` |
| `media.rs` | GSMTC watcher, transport/seek, art cache. Emits raw `(position_ms, position_at_ms)` pairs, diff-suppressed |
| `dock.rs` | Free placement with per-axis edge snapping, hit-rect click-through, topmost re-assert |
| `presence.rs` | Fullscreen sensing and the courtesy conceal |
| `lyrics.rs` | LRCLIB fetch with a disk cache. Prefers original-script synced lyrics over romanized ones |
| `karaoke.rs`, `karaoke/research.rs` | Word-karaoke capture, cache, and alignment orchestration. Opt-in research recordings |
| `align.rs`, `acoustic.rs`, `acoustic_worker.rs` | Forced alignment of known lyric tokens onto captured PCM. The optional local MMS model falls back to a fixed-prior aligner |
| `entry_timing.rs`, `vocal_preview.rs`, `preview_lifetime.rs` | Dev-only vocal-separation trial (never enabled in release builds) |
| `sync_library.rs` | Saved-sync library: reversible delete, generation-guarded jobs |
| `audio.rs`, `loopback.rs` | Process-scoped WASAPI loopback, FFT to band energies, device-wide fallback |
| `history.rs` | Append-only `history.jsonl` of Apple Music and Spotify listens |
| `spotify.rs`, `spotify_jump.rs` | Web API (PKCE OAuth), play-now jump, and the evidence that verifies it |
| `upnext.rs` | Palette-managed up-next list, fed to Spotify one track at a time |
| `lastfm.rs`, `similar.rs` | "More like this" and Search's discovery picks |
| `search.rs`, `focus.rs`, `prefs.rs` | Secondary windows |
| `settings.rs` | `settings.json` read-modify-write plus `write_atomic` |

`src/`: `App.tsx` (the widget: pill ↔ card ↔ expanded), `Focus.tsx`, `Search.tsx`, `Prefs.tsx`, `Queue.tsx`, `LyricsPanel.tsx`, `Waveform.tsx` (the living separator), `icons/` (morphing icons), `lib/` (`posClock.ts`, `lrc.ts`, `wordWipeDriver.ts`, `tokens.ts`, `sizes.ts`, `trackDir.ts`, `palette.ts`).

`docs/`: `smtc-support-matrix.md` (what each player honors over GSMTC), `presence-signal-matrix.md` (what Windows reports per fullscreen scenario), `specs/` (feature specs), `handoffs/` + `research/` + `performance/` (karaoke evidence), `states/` (UI state contracts).

## Invariants

### Windows and placement

- The main window never resizes after launch. It's born at `WINDOW_MAX` (`tauri.conf.json` must match `MODE_SIZES.expanded` in `src/lib/sizes.ts`), and mode changes are CSS glides inside it. Resizing the native window for animation makes WebView2 shake or blink.
- The oversized window is click-through outside the frontend-reported hit rect (`set_hit_size`). Anything that pops out of the widget (the queue popover, overlays) has to be added to the hit rect, otherwise clicks go through to the desktop.
- Placement is free. On release, each axis snaps to an edge line within 24px, otherwise it stays where it was dropped. There is no corner magnet and no fullscreen seat, and Palette never moves itself in response to another app. The docked corner is the anchor for seat, popover direction, and growth. It's persisted (`dockCorner`) because it can't be re-derived from the window rect.
- Every show/hide of the main window goes through `apply_visibility`, never a raw `hide()`/`show()`. Search owns its own show/hide. Focus is `focus_open` in `VisIntent`.
- Presence has one consumer: the conceal. Presence never moves, resizes, or recolors anything, and manual input always wins.
- Every window label must be listed in `src-tauri/capabilities/default.json`. A missing label means no IPC for that window, and nothing reports the failure. Window-state denylists search, focus, and prefs.

### Playback and data

- `src/lib/posClock.ts` is the only place that owns the displayed position. Rust never projects a UI position. Per-player timing constants (`JITTER_BAND_MS`, `VOCAL_LEAD_MS`) mirror `docs/smtc-support-matrix.md`, so update both together.
- Never trust a command's return value (SMTC or Spotify). Verify by re-reading state.
- Spotify: never `PUT play uris` into a live context, because that kills it. The only exception is starting from no playback. Tokens are deleted only on proof the session is dead (a 400-class token answer, or a fresh token that still gets 401). Transport errors, 5xx, and 403 never delete them.
- The Last.fm API key is personal (`settings.json` `lastfm_api_key`), never a constant, and never logged. The repo is public.
- Config writes go through `settings.rs` (`set_value` merges per key, `write_atomic` for every config file). Never overwrite `settings.json` wholesale.
- Blocking HTTP runs on `spawn_blocking`, never on the media loop thread. Alignment inference never touches the media thread either.
- `norm_key` exclusion is shared by `similar.rs` and `Search.tsx`. Keep them in lockstep.
- History logs Apple Music and Spotify only. Browsers and YouTube are excluded because GSMTC mislabels video as Music.

### Karaoke

- Word timings attach to lyric rows by their LRC line stamp, never by word time.
- Caches are bound to the exact source lyrics and alignment recipe. Captures with seeks or audio gaps are rejected, not cached.
- No model weights or song audio are bundled or uploaded. The vocal-separation trial is dev-only.
- Don't retune the 160ms word lead or the 90ms wipe attack from synthetic tests. Only listener feedback (the `Ctrl+Alt+[`/`]` nudges) moves them.

### Design

- Chrome is neutral (semantic tokens in `index.css`/`lib/tokens.ts`). The album-art palette is an accent layer only: progress fills, separator capsules, the current-lyric marker, and brief queue feedback. Accent never colors text or chrome surfaces (extracted accents only guarantee 3:1 contrast).
- No glow anywhere. The shell shadow is neutral black.
- The only ambient motion is the separator bars and the no-session resting pulse. Icons morph only in response to input. Motion uses the `EASE`/`DUR` tokens.
- Each view has one living separator.
- Track changes: the outgoing view exits fast with a directional slide (90/90ms, `lib/trackDir.ts`), keyed on the settled slide epoch rather than the raw track key. Chrome, transport, and the waveform don't slide. Lyric arrival choreography is only for content the user waited on.

### Settled: don't re-propose

Each of these was tried and removed after live use:

- Idle- or attention-driven behaviors (AFK grow, "working quiet")
- The corner magnet and the fullscreen seat
- Track-change waveform choreography
- Resizing the native window for animation
- A persistent controlled-app badge

## Player gotchas

Details are in `docs/smtc-support-matrix.md`.

- Apple Music has no working programmatic seek (SMTC seek returns `true` and does nothing). Seek buttons are capability-gated, and the hotkey still fires. Position reports at 1s granularity.
- Apple Music packs `"<artist> — <album>"` into Artist and deregisters its session when playback stops. A missing session is a normal state.
- GSMTC updates title/artist before the thumbnail. `media.rs` re-reads art for about 10s after a track change and bumps the `art_id` revision when the bytes change.
- Apple Music's thumbnail ContentType is a comma-separated list (take the first entry). `ReadAsync` can return partial data.
- Commands go to Windows' "current" session, which follows whichever app played most recently.

## Releases

Bump `version` in `tauri.conf.json`, `Cargo.toml`, `package.json` (and the lockfiles), add a `CHANGELOG.md` entry, merge, then `git tag vX.Y.Z && git push origin vX.Y.Z`. `release.yml` builds, signs, and publishes the GitHub Release with `latest.json`, and installed apps self-update at launch. A local `npm run tauri build` needs `TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/pulse.key)"` (the `_PATH` variant doesn't work). Losing that key orphans every install.

## Workflow

- Work on a branch and open a PR. Never commit to `main`.
- The repo lives off OneDrive (`C:\Users\Thien\Projects\pulse`) because Vite misbehaves under OneDrive sync.
