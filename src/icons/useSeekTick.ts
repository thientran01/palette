import { useRef } from "react";
import { useAnimate, useReducedMotion } from "motion/react";
import { DUR, EASE } from "../lib/tokens";

const OUT = [...EASE.out] as [number, number, number, number];

/** Seek press feedback: the glyph is a rotation arrow, so each press spins it
 * one FULL revolution in the seek direction (220ms EASE.out — flick-a-dial;
 * a ±12° kick read as a nervous twitch, not a seek). A revolution ends at
 * rest orientation, so there is no settle phase; the rotation is snapped
 * back to 0 invisibly afterwards. Mashing accumulates turns mid-flight —
 * repeated seeks read as the dial whirring. The glyph carries no numeral:
 * the spin plus the visible time jump are the feedback; the amount lives in
 * the tooltip/aria-label. */
export function useSeekTick(dir: -1 | 1) {
  const [scope, animate] = useAnimate();
  const reduced = useReducedMotion() ?? false;
  const turns = useRef(0);
  const seq = useRef(0);

  const tick = async () => {
    if (reduced || !scope.current) return;
    const id = ++seq.current;
    turns.current += dir * 360;
    await animate(scope.current, { rotate: turns.current }, { duration: DUR[4] / 1000, ease: OUT });
    if (id !== seq.current) return; // a newer press extended the spin
    // N·360 ≡ 0 — normalize instantly (visually identical) so the value
    // can't grow without bound across a session.
    turns.current = 0;
    await animate(scope.current, { rotate: 0 }, { duration: 0 });
  };

  return { scope, tick };
}
