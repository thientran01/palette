import { useRef } from "react";
import { useReducedMotion } from "motion/react";
import { cssEase, DUR, EASE } from "../lib/tokens";

const IN_OUT = cssEase(EASE.inOut);

/** Seek press feedback: the glyph is a rotation arrow, so each press spins it
 * one FULL revolution in the seek direction (260ms; a ±12° kick read as a
 * nervous twitch, not a seek; 220ms spun too fast, per live feedback).
 * EASE.inOut, NOT out: with out's front-loaded velocity the near-circular
 * glyph jumped 45°+ per frame at launch and stroboscopically read as
 * spinning BACKWARD (live feedback — the rendered direction was measured
 * correct). inOut launches slow enough for the eye to lock direction, hides
 * peak speed mid-flight, and arrives legibly. A revolution ends at rest
 * orientation, so there is no settle phase; the rotation is snapped back
 * to 0 invisibly afterwards. Mashing accumulates turns mid-flight —
 * repeated seeks read as the dial whirring. The glyph carries no numeral:
 * the spin plus the visible time jump are the feedback; the amount lives in
 * the tooltip/aria-label.
 *
 * WAAPI on the CSS `rotate` property, not Motion's `rotate`, so the spin runs
 * on the compositor (Motion 12 ticks it on the main thread). The computed
 * `rotate` reads back unwrapped degrees mid-flight (450deg, not a matrix), so
 * a mash can extend the spin from exactly where the arrow is. No fill: a
 * finished spin is a whole number of turns, so dropping back to none is the
 * same frame. */
export function useSeekTick(dir: -1 | 1) {
  const scope = useRef<HTMLSpanElement>(null);
  const reduced = useReducedMotion() ?? false;
  const flight = useRef<Animation | null>(null);
  // Where the running spin will come to rest, in degrees (0 when idle).
  const target = useRef(0);
  const tick = () => {
    const el = scope.current;
    if (reduced || !el) return;
    // Read BEFORE cancelling; "none" at rest parses to NaN → 0.
    const from = parseFloat(getComputedStyle(el).rotate) || 0;
    // Mashing accumulates turns mid-flight; a fresh press starts from 0.
    target.current = (flight.current ? target.current : 0) + dir * 360;
    flight.current?.cancel();
    const a = el.animate([{ rotate: `${from}deg` }, { rotate: `${target.current}deg` }], {
      duration: DUR[5],
      easing: IN_OUT,
    });
    flight.current = a;
    a.onfinish = () => {
      if (flight.current !== a) return; // a newer press extended the spin
      flight.current = null;
      target.current = 0;
    };
  };
  return { scope, tick };
}
