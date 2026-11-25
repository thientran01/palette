/*
 * The mode ladder's box constants — split out of App.tsx when main.tsx (the
 * entry) started importing WINDOW_MAX for the crash-fallback hit-rect widen:
 * a non-component export on App.tsx demotes the app's most-edited module
 * from a Fast Refresh boundary (mixed exports), turning every App edit into
 * a full page reload up through the entry. Values only, never behavior.
 */

export type Mode = "pill" | "card" | "expanded";

/** Each mode's FOOTPRINT (shell + gutter), in logical px. The native window
 * itself never resizes: it lives at WINDOW_MAX from birth (tauri.conf.json)
 * and every mode change is the shell's 200ms EASE.inOut CSS glide inside it,
 * clip-revealing the incoming mode's already-final ModeContent plane while
 * the content layers crossfade. (Resizing a WebView2 window at all costs one
 * wrong frame: per-frame animation shakes, a snap blinks — measured live
 * 2026-07-09/10.) These sizes still drive the shell/plane boxes and the
 * click-through hit rect (dock.rs). */
export const MODE_SIZES: Record<Mode, [number, number]> = {
  pill: [300, 48],
  // The mini lyrics view (Thien, 2026-11-25, design "E"): a 78px cover
  // inset 8px, titles + the current lyric line beside it, the pill's accent
  // hairline as progress, controls on hover. 108 = SHELL_CHROME 14 + the
  // 94px content row (cover 78 + 2 x 8 inset). Beside it: 6 + title 20 +
  // meta 16, then the lyric slot's 44px down to the cover's bottom line,
  // which fits a two-row line.
  card: [380, 108],
  expanded: [380, 440], // lyrics home; big-art fallback gets breathing room
};

/** The window's permanent size = the largest mode. Keep in sync with
 * tauri.conf.json width/height (the window must be BORN at this size —
 * matching them means the launch dock is pure positioning, no resize, no
 * first-frame artifact). Consumers: App.tsx everywhere, main.tsx's
 * crash-fallback widen — never duplicate the numbers. */
export const WINDOW_MAX: [number, number] = MODE_SIZES.expanded;
