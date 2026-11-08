import { useRef } from "react";
import { useReducedMotion } from "motion/react";
import { cssEase, DUR, EASE } from "../lib/tokens";

const IN_OUT = cssEase(EASE.inOut);

/** Skip press feedback: the glyph travels out the side it points at while a
 * pixel-identical ghost follows it in from the other side — one continuous
 * pass-through inside the button's mask, the track itself flicking past.
 * A directional cousin of the seek spin: full committed travel per press
 * (the ±12° seek kick taught that a partial gesture reads as a nervous
 * twitch, not the verb). EASE.inOut for the same reason as the spin — the
 * launch is slow enough for the eye to lock direction. DUR[3], not the
 * spin's DUR[5]: the extra room exists because a revolution sweeps ~4× the
 * distance of this 16px pass — at equal duration the flick would crawl. The end frame is the
 * ghost centered, pixel-identical to rest, so the strip snaps back to 0
 * invisibly (the spin's N·360 normalize). Mashing re-launches from wherever
 * the glyph visually is: rewinding the strip one width is frame-identical
 * (the ghosts are exact copies one width away), so repeated presses read as
 * flicking through tracks, never a pop.
 *
 * The scope element must be a strip of THREE copies of the glyph — ghosts
 * parked at ±width — inside an overflow-hidden box of exactly `width`, or
 * the wrap frames stop being identical and the mash pops.
 *
 * Driven through WAAPI on the CSS `translate` property, not Motion's `x`:
 * Motion 12 runs `x` on the main thread, and a skip press is the moment the
 * whole widget re-renders for the new track, so a main-thread flick stutters
 * right as it plays. A native animation runs on the compositor, and the
 * computed `translate` still reports where the glyph is mid-flight. No fill:
 * when the pass ends the strip simply drops back to 0, the ghost-identical
 * frame. */
export function useSkipFlick(dir: -1 | 1, width: number) {
  const scope = useRef<HTMLSpanElement>(null);
  const reduced = useReducedMotion() ?? false;
  const flight = useRef<Animation | null>(null);
  const tick = () => {
    const el = scope.current;
    if (reduced || !el) return;
    // Read BEFORE cancelling: the computed value is the in-flight position.
    // "none" (at rest) parses to NaN → 0.
    const cur = parseFloat(getComputedStyle(el).translate) || 0;
    flight.current?.cancel();
    // Past center → rewind one width (frame-identical) so the pass runs
    // full-length; before center (a mash caught the re-entry) → continue
    // from where the glyph is, sweeping it through center and out.
    const from = dir * cur > 0.5 ? cur - dir * width : cur;
    const a = el.animate([{ translate: `${from}px 0` }, { translate: `${dir * width}px 0` }], {
      duration: DUR[3],
      easing: IN_OUT,
    });
    flight.current = a;
    a.onfinish = () => {
      if (flight.current === a) flight.current = null;
    };
  };
  return { scope, tick };
}
