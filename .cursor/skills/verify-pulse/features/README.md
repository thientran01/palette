# Pulse verification map

This directory is the maintained source for verifying the user-facing behavior of Pulse's Vite browser mock. Read the index before driving the app, then use the matching feature file as the recipe.

## Baseline preconditions

- Launch the mock with `node .cursor/skills/verify-pulse/scripts/launch.mjs` (port 1422 unless `--port` was passed).
- Run `node .cursor/skills/verify-pulse/scripts/doctor.mjs` and require `ok`, `html=pulse-vite`, and a `url` on localhost that is not port 1420.
- Never drive an instance that was not started by this verification run. Never drive `npm run tauri dev` or an installed Pulse.
- After navigating to `/`, set `localStorage.pulse.mode` to `card`, remove `pulse.expandedView`, and reload so leftover origin storage cannot start you in pill or expanded.
- Default `/` seed: playing **Savior** / **THE BOYZ**, seekable, Spotify connected, Up next **Instagram** and **WHERE YOU AT?**, Earlier populated.

## Driving conventions

- Start every recipe from the baseline state unless its preconditions say otherwise.
- Prefer ARIA roles and accessible names over CSS selectors or coordinates.
- Reveal hover-only chrome (mode cluster, pill/card queue toggle, pill play/pause) by hovering the bottom-right widget or running `chrome-session.mjs reveal`. Do not assume Tab or a click on the progress slider reveals chrome.
- Treat every command as literal. Keep quoted names and query flags unchanged.
- Run browser actions through cursor-ide-browser against the doctor URL, or through `chrome-session.mjs` (`start`, `reveal`, `click`, `snapshot`, `screenshot`, `goto`, `stop`).
- Restore the baseline (mode card, queue closed, default `/`) after a mutation. Do not remove proof artifacts during cleanup.

## Proof and skip reporting

- Capture the user action and the resulting state, not only the final screen.
- UI proof includes an ARIA snapshot and a screenshot with Pulse identity visible (Savior / THE BOYZ, Nothing playing, or Play something…).
- Mutation proof includes a second look at the same page (now-playing title, Up next, Earlier toast).
- Record the feature ID and entry point used with every artifact.
- Report an unreachable path with the attempted command and the unmet precondition.
- Do not report a skipped entry point as verified through a different path.
- Native tray / hotkey / dock / conceal / OAuth paths are `verified-unreachable` from this mock.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the user-visible behavior. It then uses exactly four H2 sections in this order.

1. `Sub-features` lists short IDs with one line for each behavior.
2. `How to get to it (user POV)` lists every user entry point.
3. `Driving it with the browser mock` starts with `Preconditions:` and uses labeled bullets that pair each user action with an exact command and observable result.
4. `Gotchas` lists traps that can waste or invalidate a verification run.

Keep implementation details out of the map. Name only user paths, stable handles, required state, commands, and observable proof.

## Features

- [Mode ladder](./mode-ladder.md) covers pill, card, expanded, the resting empty state, and the focus-room mock.
- [Transport](./transport.md) covers play/pause, ±10s seek, skip, and the Apple Music no-seek profile.
- [Queue and history](./queue-history.md) covers the popover and expanded surface, Up next, Earlier, and play-now.
- [Lyrics](./lyrics.md) covers the expanded karaoke view, click-to-seek, the art fallback, and a lyrics miss.
- [Search palette](./search-palette.md) covers history resurfacing, type-to-search, play, and queue.
