/*
 * Motion tokens — JS mirror of the @theme values in index.css, for `motion`
 * (framer-motion) transitions and any inline style that can't read CSS vars.
 * Keep in sync with index.css; there is deliberately no third copy.
 */
export const EASE = {
  out: [0.16, 1, 0.3, 1],
  inOut: [0.65, 0, 0.35, 1],
} as const;

/** Durations in ms — everything under 300ms. 5 exists for the seek spin:
 * a full revolution needs more room than a state morph. */
export const DUR = { 1: 90, 2: 140, 3: 200, 4: 220, 5: 260 } as const;

/** An EASE curve as a CSS easing string, for WAAPI (`element.animate`). */
export const cssEase = (e: readonly number[]): string => `cubic-bezier(${e.join(", ")})`;

/** A slide offset as a full `transform` string, for Motion variants. Not
 * Motion's `x`: in motion 12 only opacity, transform, filter and clip-path
 * are handed to WAAPI (motion-dom's acceleratedValues), so `x` ticks on the
 * main thread's rAF loop. A skip is the busiest moment the widget has (the
 * new track's render, the lyrics fetch and the art decode all land in the
 * same 90ms), and a main-thread slide drops frames exactly then. */
export const slideX = (dx: number): string => `translateX(${dx}px)`;

/** Hand a settled slide back with no transform at all (what `x: 0` left
 * behind), so a resting panel isn't left as its own stacking context. Use
 * as a target's `transitionEnd`. */
export const SLIDE_REST = { transform: "none" } as const;
