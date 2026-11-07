import { useRef } from "react";
import { useReducedMotion } from "motion/react";
import { cssEase, DUR, EASE } from "../lib/tokens";

const OUT = cssEase(EASE.out);
const IN_OUT = cssEase(EASE.inOut);

/** Mode-bracket press feedback: the glyph pulses in the direction of its
 * verb — expand's corner brackets burst apart, contract's pull together —
 * then settles back while the shell does the real glide. Scale on the glyph
 * wrapper IS the bracket separation (the two strokes are symmetric about
 * center), so no extra geometry poses. Fast attack (EASE.out to the peak),
 * settled return (EASE.inOut) — the press registers instantly, the recovery
 * is calm. Rides inside the button's own 90ms 0.95 press scale; for expand
 * the two oppose deliberately: the chrome dips while the verb grows.
 *
 * WAAPI on the CSS `scale` property, not Motion's `scale`: the press lands
 * just as the mode change re-renders the widget, and Motion 12 would tick
 * the pulse on that same busy main thread. Per-keyframe easing gives the
 * two segments their own curves. */
export function useBracketPulse(verb: "expand" | "contract") {
  const scope = useRef<HTMLSpanElement>(null);
  const reduced = useReducedMotion() ?? false;
  const peak = verb === "expand" ? 1.2 : 0.8;
  const pulse = () => {
    const el = scope.current;
    if (reduced || !el) return;
    el.getAnimations().forEach((a) => a.cancel());
    el.animate(
      [
        { scale: 1, easing: OUT },
        { scale: peak, offset: 0.4, easing: IN_OUT },
        { scale: 1 },
      ],
      { duration: DUR[3] },
    );
  };
  return { scope, pulse };
}
