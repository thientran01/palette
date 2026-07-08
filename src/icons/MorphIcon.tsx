import { useEffect, useRef } from "react";
import { motion, useReducedMotion } from "motion/react";
import { DUR, EASE } from "../lib/tokens";
import { ICONS, STROKE_WIDTH, type MorphName } from "./geometry";

const OUT = [...EASE.out] as [number, number, number, number];

export function MorphIcon({
  name,
  size,
  dur = DUR[3],
  ease = EASE.inOut,
  className,
}: {
  name: MorphName;
  size: number;
  dur?: number;
  ease?: readonly [number, number, number, number];
  className?: string;
}) {
  const reduced = useReducedMotion() ?? false;
  const prevName = useRef<MorphName>(name);
  useEffect(() => {
    prevName.current = name;
  }, [name]);

  const from = ICONS[prevName.current];
  const to = ICONS[name];
  const seconds = reduced ? 0 : dur / 1000;

  return (
    <svg
      viewBox="0 0 16 16"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={STROKE_WIDTH}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      className={className}
    >
      {to.map((stroke, i) => {
        const appearing = from[i].o === 0 && stroke.o === 1;
        const vanishing = from[i].o === 1 && stroke.o === 0;
        return (
          <motion.path
            key={i}
            initial={{ d: from[i].d, opacity: from[i].o }}
            animate={{ d: stroke.d, opacity: stroke.o }}
            transition={{
              d: { duration: seconds, ease: [...ease] as [number, number, number, number] },
              // Appearing strokes fade in over the FIRST 90ms — their park
              // point sits on live geometry, so the early fade reads as the
              // joint budding. Vanishing strokes fade out over the LAST 90ms
              // so they stay readable while shrinking toward their park.
              // Parked-on-both-ends strokes hold 0 with no tween (no flash).
              opacity: appearing
                ? { duration: reduced ? 0 : DUR[1] / 1000, ease: OUT }
                : vanishing
                  ? {
                      duration: reduced ? 0 : DUR[1] / 1000,
                      delay: reduced ? 0 : Math.max(dur - DUR[1], 0) / 1000,
                      ease: OUT,
                    }
                  : { duration: 0 },
            }}
          />
        );
      })}
    </svg>
  );
}
