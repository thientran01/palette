/*
 * Is THIS window on screen? Rust tells each window (lib.rs tell_shown).
 *
 * The page can't tell on its own: Tauri's hide() hides the HWND, but wry only
 * calls WebView2's SetIsVisible at creation, so a hidden window keeps
 * `document.hidden === false`, `visibilityState === "visible"`, and full-rate
 * requestAnimationFrame. Measured on Thien's PC (2026-11-09): ~167 fps of rAF
 * in the widget hidden behind focus. Frame loops (the karaoke driver, the
 * progress bar) pause on this instead.
 *
 * Fails open: until Rust says otherwise, and always in a plain browser, the
 * window counts as shown. A window wrongly believed hidden would freeze its
 * karaoke; one wrongly believed shown only spends frames.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";

// types.ts's IN_TAURI, guarded: this module also loads under node tests,
// where `window` doesn't exist.
const IN_TAURI = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let shown = true;
const subs = new Set<() => void>();

function set(next: boolean): void {
  if (next === shown) return;
  shown = next;
  // Infinite CSS breathers (index.css) pause on this; one-shot animations
  // are left alone so nothing finishes late after a show.
  document.documentElement.toggleAttribute("data-window-hidden", !next);
  for (const cb of [...subs]) cb();
}

if (IN_TAURI) {
  // Listen first, then seed: a change after the seed's read always arrives
  // as an event, so a show can't slip between the two and leave the page
  // believing it's hidden. An event that lands before the seed wins, since
  // the seed may have read the old state.
  let heard = false;
  void listen<boolean>("window-shown", (e) => {
    heard = true;
    set(e.payload !== false);
  })
    .then(() => invoke<boolean>("window_shown"))
    .then((v) => {
      if (!heard) set(v !== false);
    })
    .catch(() => {});
}

export function isWindowShown(): boolean {
  return shown;
}

export function onWindowShown(cb: () => void): () => void {
  subs.add(cb);
  return () => {
    subs.delete(cb);
  };
}

export function useWindowShown(): boolean {
  return useSyncExternalStore(onWindowShown, isWindowShown);
}

/** A play clock that also reads "paused" while the window is hidden, and
 * notifies on show/hide, so a driver built on it (wordWipeDriver) stops its
 * rAF loop behind a hidden window and catches up in one write on show. The
 * driver isn't disposed on hide: its letters keep their state, and nothing
 * flashes when the window comes back. */
export interface PlayClock {
  now(): number;
  isPlaying(): boolean;
  subscribe(cb: () => void): () => void;
}

export function whileShown(
  clock: PlayClock,
  shownNow: () => boolean = isWindowShown,
  onShownChange: (cb: () => void) => () => void = onWindowShown,
): PlayClock {
  return {
    now: () => clock.now(),
    isPlaying: () => clock.isPlaying() && shownNow(),
    subscribe(cb) {
      const offClock = clock.subscribe(cb);
      const offShown = onShownChange(cb);
      return () => {
        offClock();
        offShown();
      };
    },
  };
}
