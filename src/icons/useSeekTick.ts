import { useRef } from "react";
import { useAnimate, useReducedMotion } from "motion/react";
import { DUR, EASE } from "../lib/tokens";

const OUT = [...EASE.out] as [number, number, number, number];

/** Seek press feedback: a ±12° rotational kick with a 140ms settle back to 0.
 * The glyph carries no numeral — the tick plus the visible time jump are the
 * feedback; the amount lives in the tooltip/aria-label. Mashing retriggers
 * from the current angle, clamped at ±18°, so it reads as the dial straining
 * rather than accumulating. Two eased tweens, no spring. */
export function useSeekTick(dir: -1 | 1) {
  const [scope, animate] = useAnimate();
  const reduced = useReducedMotion() ?? false;
  const angle = useRef(0);
  const seq = useRef(0);

  const tick = async () => {
    if (reduced || !scope.current) return;
    const id = ++seq.current;
    angle.current = Math.max(-18, Math.min(18, angle.current + dir * 12));
    await animate(scope.current, { rotate: angle.current }, { duration: DUR[1] / 1000, ease: OUT });
    if (id !== seq.current) return; // a newer press owns the settle
    angle.current = 0;
    await animate(scope.current, { rotate: 0 }, { duration: DUR[2] / 1000, ease: OUT });
  };

  return { scope, tick };
}
