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
 * The pan is LINEAR: it's text being read, so it moves at one steady reading
 * pace (Emil Kowalski's easing rule: constant motion like a marquee is
 * linear). The first cut eased in and out, which ran the middle of each pan
 * ~1.6x faster than its average and made it hard to read (Thien,
 * 2026-11-09). Speed is in em/s, so Focus's 40px title and the pill's 12px
 * artist pan at the same pace in characters.
 *
 * Lines that belong together (a title and its artist) share one clock: same
 * speed, same start, same rest, and they arrive home together. The shorter
 * line finishes its pan out first and waits; on the way back it leaves later
 * so both land at the start at the same moment. Panning at different speeds
 * and times read as broken (Thien, 2026-11-09).
 */

export const MARQUEE = {
  /** Width of each edge fade, in em. Also how far the left fade sits
   * outside the column, so it must stay under the gap to the left
   * neighbor (8px pill, 10px expanded, 12px card at 12-15px text). */
  fadeEm: 0.5,
  /** Pan speed, in em per second: ~20px/s for the 15px titles, a steady
   * reading pace. Was 2.2 average with a 1.6x eased peak. */
  speedEmPerS: 1.3,
  /** Floor for one pan, so a few pixels of overflow glide rather than twitch. */
  minPanMs: 1200,
  /** Long enough to read the start (and let a track-change slide finish). */
  restStartMs: 3000,
  restEndMs: 2000,
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

/** One line's measurement: how far it travels, at what font size. */
export interface MarqueeLine {
  travelPx: number;
  fontPx: number;
}

/** The clock a group of lines shares. */
export interface MarqueeClock {
  /** Pan speed for every line in the group. */
  pxPerMs: number;
  /** The longest line's pan; the cycle is built around it. */
  panMs: number;
  totalMs: number;
}

/** The shared clock for the lines that pan, or null when none does. Speed
 * comes from the largest panning font, so a title and its smaller artist
 * line move together at the title's pace. */
export function marqueeClock(lines: Iterable<MarqueeLine>): MarqueeClock | null {
  let travel = 0;
  let font = 0;
  for (const l of lines) {
    if (l.travelPx <= 0) continue;
    travel = Math.max(travel, l.travelPx);
    font = Math.max(font, l.fontPx);
  }
  if (travel <= 0) return null;
  const pxPerMs = (MARQUEE.speedEmPerS * font) / 1000;
  const panMs = Math.max(MARQUEE.minPanMs, Math.round(travel / pxPerMs));
  return { pxPerMs, panMs, totalMs: MARQUEE.restStartMs + panMs + MARQUEE.restEndMs + panMs };
}

/** One line's keyframes on the group clock (linear between them). */
export function marqueeKeyframes(travelPx: number, clock: MarqueeClock): Keyframe[] {
  const { restStartMs: r, restEndMs: e } = MARQUEE;
  const P = clock.panMs;
  const T = clock.totalMs;
  const p = Math.min(P, Math.max(MARQUEE.minPanMs, Math.round(travelPx / clock.pxPerMs)));
  const home = "translateX(0px)";
  const away = `translateX(${-travelPx}px)`;
  return [
    { offset: 0, transform: home },
    { offset: r / T, transform: home },
    { offset: (r + p) / T, transform: away },
    // Leave late by the difference, so every line lands home together.
    { offset: (r + P + e + (P - p)) / T, transform: away },
    { offset: 1, transform: home },
  ];
}
