import { useRef } from "react";
import { useAnimate, useReducedMotion } from "motion/react";
import { DUR, EASE } from "../lib/tokens";

const IN_OUT = [...EASE.inOut] as [number, number, number, number];

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
    await animate(scope.current, { rotate: turns.current }, { duration: DUR[5] / 1000, ease: IN_OUT });
    if (id !== seq.current) return; // a newer press extended the spin
    // N·360 ≡ 0 — normalize instantly (visually identical) so the value
    // can't grow without bound across a session.
    turns.current = 0;
    await animate(scope.current, { rotate: 0 }, { duration: 0 });
  };

  return { scope, tick };
}
