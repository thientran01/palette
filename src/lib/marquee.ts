/*
 * Timing for the marquee (src/Marquee.tsx): text too long for its line pans
 * to its end, rests, and pans back, instead of ending in an ellipsis
 * (Thien, 2026-11-09).
 *
 * One cycle, as one infinite WAAPI animation on `transform` (compositor-only,
 * so a panning title costs no main-thread frames):
 *
 *   rest at start → pan out → rest at end → pan back
 *
 * The start rest is the longer one and comes first, so a new song's title
 * sits still long enough to read its beginning (and for the track-change
 * slide to finish) before anything moves. Speed is in em/s, so a 40px Focus
 * title and a 12px pill artist pan at the same pace in characters. The pan
 * eases in and out on a sine curve: its peak speed is only ~1.6x the
 * average, where EASE.inOut (cubic) peaks at 3x and reads as a lurch over a
 * multi-second pan.
 */

export const MARQUEE = {
  /** Width of each edge fade, in em. Also how far the left fade sits
   * outside the column, so it must stay under the gap to the left
   * neighbor (8px pill, 10px expanded, 12px card at 12-15px text). */
  fadeEm: 0.5,
  /** Average pan speed, in em per second (~26px/s at 12px). */
  speedEmPerS: 2.2,
  /** Floor for one pan, so a few pixels of overflow glide rather than twitch. */
  minPanMs: 1200,
  restStartMs: 3000,
  restEndMs: 1600,
  /** Sine in-out. */
  ease: [0.37, 0, 0.63, 1],
} as const;

/** Overflow under this many px isn't worth moving for (subpixel rounding). */
export const MARQUEE_SLACK_PX = 1;

/** A column narrower than this many fades can't show a word, so panning
 * through it would be letters flickering past a slot (the pill's artist
 * when a long title takes the whole row); it keeps the ellipsis. */
export const MARQUEE_MIN_COLUMN_FADES = 4;

/** How far the text must travel so its end lands just inside the right
 * fade, or 0 when it fits (or the column is too narrow to read through).
 * `textPx` is the text's full width, `columnPx` the width it has to show
 * in, `fadePx` one edge fade. */
export function marqueeTravel(textPx: number, columnPx: number, fadePx: number): number {
  if (!(textPx > columnPx + MARQUEE_SLACK_PX)) return 0;
  if (columnPx < MARQUEE_MIN_COLUMN_FADES * fadePx) return 0;
  return Math.round((textPx - columnPx + fadePx) * 10) / 10;
}

export interface MarqueeCycle {
  panMs: number;
  totalMs: number;
  keyframes: Keyframe[];
}

/** One full cycle for `travelPx` of travel at a font size of `fontPx`. */
export function marqueeCycle(travelPx: number, fontPx: number): MarqueeCycle {
  const pxPerMs = (MARQUEE.speedEmPerS * fontPx) / 1000;
  const panMs = Math.max(MARQUEE.minPanMs, Math.round(travelPx / pxPerMs));
  const { restStartMs: r, restEndMs: e } = MARQUEE;
  const totalMs = r + panMs + e + panMs;
  const ease = `cubic-bezier(${MARQUEE.ease.join(", ")})`;
  const home = "translateX(0px)";
  const away = `translateX(${-travelPx}px)`;
  return {
    panMs,
    totalMs,
    keyframes: [
      { offset: 0, transform: home },
      { offset: r / totalMs, transform: home, easing: ease },
      { offset: (r + panMs) / totalMs, transform: away },
      { offset: (r + panMs + e) / totalMs, transform: away, easing: ease },
      { offset: 1, transform: home },
    ],
  };
}
