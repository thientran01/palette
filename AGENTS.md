# Palette

Always-on-top mini music player for Windows (Tauri v2 + React). It controls whatever is playing through GSMTC. Windows-only, so Rust won't compile elsewhere. Module doc comments and `docs/specs/` explain the reasoning behind each design.

Before opening a PR, run what CI runs: `npm run build`, `npm test`, and `cargo test` / `fmt --check` / `clippy -D warnings` in `src-tauri`. To check UI in a browser, run `npm run dev` (flags are in `src/lib/backend.ts`; see `.cursor/skills/verify-pulse`).

## Rules the code won't enforce

- Never resize the main window. WebView2 shakes. Mode changes are CSS inside a fixed window, and `tauri.conf.json` must match `MODE_SIZES.expanded`.
- Anything that pops out of the widget must be added to the hit rect, or clicks fall through to the desktop.
- Show/hide the main window only through `apply_visibility`.
- `src/lib/posClock.ts` is the only owner of playback position.
- Never trust a command's return value (SMTC or Spotify). Re-read state to verify.
- Never `PUT play uris` into a live Spotify context, because it kills the context.
- The Last.fm key is personal and must never be a constant. The repo is public.
- Write config through `settings.rs`. Never overwrite `settings.json` wholesale.
- A new window label must be added to `src-tauri/capabilities/default.json`, or that window gets no IPC and nothing reports it.
- Player quirks are in `docs/smtc-support-matrix.md`. Apple Music can't seek, and its session disappears when playback stops.

## Design

Chrome is neutral. Album-art color is an accent only and never colors text or chrome. No glow. Nothing moves ambiently except the waveform bars and the resting pulse. Use the `EASE`/`DUR` tokens.

Already tried and rejected, so don't re-propose them: idle-driven behaviors, snapping to corners, waveform animation on track change, and a badge for the controlled app.

## Releases

Bump the version in `tauri.conf.json`, `Cargo.toml`, and `package.json`, update `CHANGELOG.md`, merge, then push a `vX.Y.Z` tag. Never commit to `main`.
