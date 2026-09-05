---
name: verify-pulse
description: "Drive Pulse's Vite browser mock — the widget, transport, queue, lyrics, and search palette — to prove user-facing behavior. Use when verifying Pulse UI, a mode/transport/queue/lyrics/palette change, or when a Pulse PR needs a scripted proof."
---

# Verify Pulse

Pulse is a Tauri v2 always-on-top music widget. The **verification surface is the Vite browser mock** (`npm run dev`): `src/lib/backend.ts` self-advances a now-playing session so an agent can drive the real React UI without GSMTC, Spotify OAuth, or the user's desktop widget.

The installed app and `npm run tauri dev` are a **different surface**. They are single-instance, share the user's real media session and app-data (`settings.json`, `history.jsonl`, `upnext.json`, `spotify_tokens.json`), and must not be driven by this skill.

Read `features/README.md` before a run. Drive the matching feature file. A proof that only hits one convenient entry point is incomplete when that file lists others.

## Launch

From the Pulse repo root, after `npm install`:

```
node .cursor/skills/verify-pulse/scripts/launch.mjs
```

Optional: `node .cursor/skills/verify-pulse/scripts/launch.mjs --port 1423`

Ready when the script prints `ready http://localhost:1422/` and exits 0. It starts the same Vite as `npm run dev` (`node node_modules/vite/bin/vite.js` with `PORT` set — not `cmd.exe`, which drops the log handle on Windows), waits until `GET /` returns this repo's Vite index (`id="root"` and `/src/main.tsx`), and writes `.cursor/skills/verify-pulse/.run/state.json`.

Default verification port is **1422**. Vite and `tauri dev` both claim **1420** (`vite.config.ts`, `strictPort: true`). Launch refuses 1420 and refuses to start a second instance if `.run/state.json` still names a live pid.

Mock routes (same origin, same process):

| URL | What the user sees |
| --- | --- |
| `/` | Spotify-like mock: playing **Savior** / **THE BOYZ**, seekable, Spotify connected, up-next **Instagram** + **WHERE YOU AT?**, ~40 Earlier rows |
| `/?am` | Apple Music profile: 1s-floored position, `can_seek=false` |
| `/?nothing` | Resting widget: **Nothing playing** |
| `/?spotify=off` | Queue/palette gate: connect-from-tray copy |
| `/?queue=empty` | Spotify connected, no playback |
| `/?lyrics=none` | Lyrics miss |
| `/?lyrics=words` | In-line word karaoke (local times fixture) |
| `/?lyrics=<ms>` | Delayed lyric fetch |
| `/?jump=partial` | Play-now returns `partial` |
| `/?similar=<status>` | More-like-this toast (`no_data`, `no_key`, `offline`, …) |
| `/?window=palette` | Search palette window |
| `/?window=focus` | Focus-mode room (native `focusOpen` is a no-op in the mock) |
| `/?lab` | Icon morph lab (dev only) |
| `/?fs` + `/?presence` | Presence overlay (`fs=YES concealed=YES`) |

Baseline fixture after every navigate to `/` (the mock persists `pulse.mode` and `pulse.expandedView` on this origin):

1. `localStorage.setItem("pulse.mode", "card")`
2. `localStorage.removeItem("pulse.expandedView")`
3. Reload

Do not launch `npm run tauri dev` for verification. Do not attach to a server you did not start.

## Doctor

Run this first, and again whenever anything looks off:

```
node .cursor/skills/verify-pulse/scripts/doctor.mjs
```

Pass means all of: `.run/state.json` exists, `pid` is alive, `port` is not 1420, `GET $url` is 200, the body is this repo's Vite index, and `package` matches `package.json` (`0.7.1` at skill creation). Stdout is `ok` plus `url=`, `pid=`, `port=`, `package=`, `title=`, `html=pulse-vite`.

Fail (`unhealthy: …`, exit 1) means stop driving. Cleanup and launch if the instance is yours; if you cannot name the pid from `.run/state.json`, do not kill by process name and do not hijack 1420.

## Drive

Harness: **cursor-ide-browser** against the doctor URL, or **chrome-session.mjs** (system Chrome/Edge via CDP) when the Cursor tab cannot hover. There is no Playwright suite and no Pulse CLI.

- Prefer accessible names. Real labels live in the feature files and in `src/App.tsx`, `src/Transport.tsx`, `src/Queue.tsx`, `src/Palette.tsx`, `src/LyricsPanel.tsx`, `src/Focus.tsx`.
- Mode cluster and the pill/card queue toggle are `pointer-events: none` until the widget is hot (`data-hot` from mousemove) or a descendant is `:focus-visible`. Hover the bottom-right widget, or run `node .cursor/skills/verify-pulse/scripts/chrome-session.mjs reveal` (dispatches `mousemove` on the widget root). cursor-ide-browser's `Tab` is a DOM key event and does **not** move real focus; a click on `Expand to lyrics` while cold fails with `pointer-events: none`. A mouse click on the progress slider also does **not** reveal chrome (`:focus-visible` stays off).
- Card and expanded **transport is always visible**. Pill play/pause sits in the hover scrim.
- Mode glide is 200ms (`DUR[3]`, `EASE.inOut`). Wait for the destination copy (lyrics region, **Nothing playing**, transport row), not a 50ms sleep.
- Default mock track ring: Savior → Instagram → WHERE YOU AT?. `window.__mockNext()` (browser console, mock only) skips without a Next button — used for the pill announcement.
- Native-only paths (report `verified-unreachable` with the prerequisite, do not fake them in the mock): tray, global hotkeys, dock drag/snap, courtesy conceal of the native window, Spotify OAuth, process-loopback audio, single-instance summons. `/?window=focus` and `/?fs` are the mock stand-ins for focus and presence UI, not those native actions.

Stable names (default `/`, card, playing):

- Transport: `Previous track`, `Back 10 seconds`, `Pause` / `Play`, `Forward 10 seconds`, `Next track`
- Progress: `Track position` (slider when seekable; progressbar on the pill hairline and on `/?am`)
- Mode: `Expand to card` / `Expand to lyrics` / `Expand to focus`, `Collapse to pill` / `Collapse to card`
- Queue: `Open queue` / `Close queue`, lists `Up next` and `Earlier`, rows `{title} — {artist}`, `Remove from queue`, `Play now`, `Add to queue`, `More like Savior`
- Lyrics: region `Lyrics`; lines `Seek to Mock lyric line N — la la la (Savior)`; `Now — back to the current line`; view toggle `Show album cover` / `Show lyrics` / `No synced lyrics`
- Palette: combobox `Search Spotify`, listbox `From your history` or `Search results`, `Play {title} now`, `Queue {title}`
- Focus: `Show queue`, `Leave focus (Esc)`
- AM seek-disabled: `Seeking not supported by Apple Music`

## Evidence

Write proof under `.cursor/skills/verify-pulse/artifacts/<feature-id>/`. Cleanup never deletes this directory.

Standards:

- Exercise the real widget/palette/focus UI. Do not call `commands.*` from the console except `__mockNext()` where a feature file names it (no Next button on the pill).
- Capture the action and the resulting state. A single pretty screenshot of the last frame is not a proof.
- Every artifact set includes an ARIA snapshot and a screenshot that show Pulse identity: mock track **Savior** / **THE BOYZ**, resting **Nothing playing**, or palette placeholder **Play something…**. The document title is `Tauri + React + Typescript` — do not use it as identity.
- Record the feature ID and the entry point used (URL + control name).
- Side effects in the mock stay in-memory (up-next, history append on skip). Confirm them on a second surface in the same page (Up next list, Earlier, now-playing title). Do not treat `app-data` files as in play — those belong to Tauri.
- `/?am`, `/?jump=partial`, `/?similar=`, `/?lyrics=` are production-boundary mocks already in `backend.ts`. They are the allowed stand-in for GSMTC/Spotify/LRCLIB. Still observe what the UI does (disabled seek, toast copy), not the query-string name.
- Unreachable native entry points are `verified-unreachable` with the attempted route and the unmet prerequisite. Do not mark them verified via a mock URL.

## Cleanup

```
node .cursor/skills/verify-pulse/scripts/cleanup.mjs
```

Kills **only** the Vite pid in `.run/state.json` and the Chrome pid in `.run/chrome.json` (Windows `taskkill /T`) and deletes `.run/`. It does not kill by process name. It does not delete `artifacts/`. After a failed iteration, run cleanup before the next launch so 1422/9333 are not stranded.

## Helpers

All of these run from the Pulse repo root:

```
node .cursor/skills/verify-pulse/scripts/launch.mjs
node .cursor/skills/verify-pulse/scripts/launch.mjs --port 1423
node .cursor/skills/verify-pulse/scripts/doctor.mjs
node .cursor/skills/verify-pulse/scripts/chrome-session.mjs start
node .cursor/skills/verify-pulse/scripts/chrome-session.mjs reset
node .cursor/skills/verify-pulse/scripts/chrome-session.mjs reveal
node .cursor/skills/verify-pulse/scripts/chrome-session.mjs click Expand to lyrics
node .cursor/skills/verify-pulse/scripts/chrome-session.mjs snapshot .cursor/skills/verify-pulse/artifacts/mode-ladder/expanded.aria.txt
node .cursor/skills/verify-pulse/scripts/chrome-session.mjs screenshot .cursor/skills/verify-pulse/artifacts/mode-ladder/expanded.png
node .cursor/skills/verify-pulse/scripts/chrome-session.mjs goto http://localhost:1422/?nothing
node .cursor/skills/verify-pulse/scripts/chrome-session.mjs stop
node .cursor/skills/verify-pulse/scripts/cleanup.mjs
```

`scripts/lib.mjs` and `scripts/cdp.mjs` are shared — do not invoke them directly. `chrome-session.mjs start` requires a live launch pid and a Chrome or Edge exe (`VERIFY_PULSE_CHROME` overrides). `cleanup.mjs` stops both Vite and the chrome session.
