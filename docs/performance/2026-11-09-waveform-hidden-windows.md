# Waveform cost — measured 2026-11-09

## Finding

The waveform was being fed and animated in windows nobody could see.

- **Expanded mode** pre-creates the focus window hidden (warm on intent, focus.rs). That window mounts the 19-bar room Waveform. It subscribed to `audio-bands` and got all ~30 payloads a second for as long as the widget sat in expanded.
- **Focus mode** hides the widget (`focus_open` in VisIntent). The hidden widget kept getting every payload too.

Each payload is a script eval on the UI thread plus a WebView2 IPC into that window's renderer, so expanded and focus modes paid for two windows. Tauri's `hide()` hides the HWND but leaves the WebView2 controller marked visible (wry calls `SetIsVisible` only at creation). The page is never told it is hidden, so its rAF loop and per-bar style writes most likely kept running too.

The DSP is not the cost. FFT 2048 + 3 bands + 16 bins is 4.6 µs per tick on the cloud container, which is 0.014% of one core at 30 Hz.

## Change

- `audio.rs` `BandFanout`: live payloads go only to windows the user can see. That means main when `VisIntent::effective_visible()`, and focus when `focus_open`. Both are atomics, so there's no main-thread hop. The capture's terminating zero still goes to every window, so a widget re-shown while paused is already at rest. A window that comes into view gets the latest payload on the next owner tick, so a latched-silent stream still settles it to the dot.
- `focus.rs`: `audio::prime` hands the warm window the latest payload right before `focus-shown`. The room remounts on that event and seeds its phase from it, so it still arrives with the bars riding, as before.
- `Waveform.tsx`: a payload older than 250 ms (`BANDS_STALE_MS`, matching `SILENCE_AFTER_MS`) reads as silence. Without that, a hidden window stepped toward its frozen last payload forever and never reached the idle stop.

Visible windows are untouched: same payloads, same cadence, same frame code.

## Numbers

Vite browser mock, headless Chromium, 60 Hz, median of 3 × 8 s. "Task" is renderer main-thread time per second of wall clock (CDP `TaskDuration`). The floor is the same page with reduced motion, so no Waveform work at all. The run-to-run noise is about ±5 ms/s.

What a hidden window costs:

| Hidden window | Before: still fed (renders like a visible page) | After: not fed | Floor |
|---|---:|---:|---:|
| Warm focus room while in expanded (19 bars) | 65.9 ms/s, 120 rAF/s, 562 bar writes/s | 32.1 ms/s, 61 rAF/s, 0 writes* | 34.5 ms/s, 60 rAF/s |
| Widget hidden behind focus, expanded art (9 bars) | 44.4 ms/s, 120 rAF/s, 266 writes/s | 29.7 ms/s, 61 rAF/s | 30.6 ms/s, 60 rAF/s |

\*The 3–6 writes/s left in the table come from the decay tail crossing the start of the measurement window.

So a hidden window now costs what a page without a waveform costs. Before, it cost the same as the visible one: about 31 ms/s of main thread for the room and about 14 ms/s for the expanded hero. The IPC side isn't measurable in a browser, but it is simple arithmetic: expanded and focus modes now eval 30 payloads a second instead of 60.

The old frontend without the stale rule, given no payloads: rAF stays at 120–180/s (the loop never idles). That's why the rule is needed alongside the Rust gate.

## Not done, and why

- Skipping unchanged transform writes was tried and measured as no change. Chromium already drops an inline-style write whose value is unchanged (no mutation record, no style invalidation).
- Pacing the bars off payload arrival instead of the rAF clock would cut rAF callbacks from vsync rate to 30/s. But it couples frame timing to IPC jitter (occasional 50 ms frames), which changes how the motion looks. Not done.
- The visible cost (about 14–31 ms/s, mostly style recalc plus compositor update for each bar's transform) is what the design itself costs. Only a canvas rewrite would change it, and that would change how the capsules are rasterized.

## Reproduction

`node scripts/perf/waveform-cost.cjs <label> [seconds]` against the verify-pulse mock (port 1422). `window.__mockUnfed(ms)` in the mock stops band payloads, which is what a hidden window now receives.
