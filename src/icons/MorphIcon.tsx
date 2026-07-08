import { useEffect, useRef, useState } from "react";
import { motion, useReducedMotion } from "motion/react";
import { DUR, EASE } from "../lib/tokens";
import { ICONS, STROKE_WIDTH, type MorphName } from "./geometry";

/** Last glyph rendered into each named slot. Module-level because App keys
 * the whole mode subtree by mode — a freshly mounted mode button has no
 * in-tree memory of the glyph the outgoing button showed, so it reads its
 * FROM state here and morphs instead of popping. */
const lastGlyph = new Map<string, MorphName>();

const OUT = [...EASE.out] as [number, number, number, number];

export function MorphIcon({
  name,
  size,
  slot,
  dur = DUR[3],
  ease = EASE.inOut,
  className,
}: {
  name: MorphName;
  size: number;
  /** Continuity slot (pairs with the button's layoutId, e.g. "mode-primary"):
   * the glyph morphs FROM whatever this slot last showed, across remounts. */
  slot?: string;
  dur?: number;
  ease?: readonly [number, number, number, number];
  className?: string;
}) {
  const reduced = useReducedMotion() ?? false;
  const [mountName] = useState<MorphName>(() => (slot ? (lastGlyph.get(slot) ?? name) : name));
  const prevName = useRef<MorphName>(mountName);
  useEffect(() => {
    if (slot) lastGlyph.set(slot, name);
    prevName.current = name;
  }, [slot, name]);

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
