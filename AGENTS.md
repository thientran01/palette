# AGENTS.md

Palette (package name `pulse`) is a **Windows-only** Tauri v2 desktop app. Project overview, architecture, hotkeys, and gotchas live in `CLAUDE.md` and `README.md`; the SMTC player-support facts live in `docs/smtc-support-matrix.md`. Read those first — this file only adds Cursor Cloud specifics.

## Cursor Cloud specific instructions

Cloud Agent VMs are **Linux**. The full app (Rust/Tauri backend) is Windows-only: it depends on GSMTC (`Windows.Media.Control`), WASAPI process-loopback, Win32/DWM APIs, and WebView2, so `npm run tauri dev`, `npm run tauri build`, and everything under `src-tauri/` (`cargo build/test/fmt/clippy`) **cannot run here** and only build/run on Windows with the Rust MSVC toolchain + VS Build Tools (per `.github/workflows/ci.yml`, which runs on `windows-latest`).

What runs on this Linux VM is the **frontend + tooling only** (all cross-platform):

- Install: `npm ci` (Node 22; matches CI).
- Test: `npm test` (Vitest — currently the `src/lib/posClock.test.ts` clock kernel net).
- Build / type-check gate: `npm run build` (`tsc` type-check + `vite build`).
- Run the UI: `npm run dev` (Vite dev server on **port 1420**, strict — the port is fixed in `vite.config.ts`; don't expect it to fall back to another port).

There is **no ESLint/Prettier and no `lint` script**; `tsc` (via `npm run build`) is the only static gate on the frontend. Rust `fmt`/`clippy` gates exist but are Windows-CI-only.

**Testing the UI without the Tauri backend:** the frontend has built-in browser mocks selected by URL query param (routed in `src/main.tsx` / `src/lib/backend.ts`), so real Windows media sessions are not needed:

- `/?am` — replays Apple Music's playback profile (a track playing, with waveform); the best default for exercising now-playing UI.
- `/?nothing` — the no-session resting state.
- `/?spotify=off` — forces the Spotify queue gate. `/?jump=partial` — the jump-failure caption.
- `/?lab` — the morphing-icon sequencer (dev only). `?window=search|focus|prefs` — render the other webviews standalone.

The optional power tier (Spotify Web API, Last.fm) needs external credentials and is not required to run or test the core UI on Linux.
