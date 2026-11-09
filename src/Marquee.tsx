/*
 * One line of text that pans to show all of itself when it's too long,
 * instead of ending in an ellipsis (Thien, 2026-11-09). Used for the
 * now-playing title and artist in the pill, card, expanded header, album
 * view and Focus. Lists (queue, search) keep their ellipsis: a column of
 * rows all panning at once would be noise.
 *
 * - Moves only when the text actually overflows, measured on every render
 *   (stragglers update a title in place), on resize, and once fonts load.
 * - Timing is in lib/marquee.ts: a long rest at the start, a slow eased pan,
 *   a shorter rest at the end, and back. One infinite WAAPI animation on
 *   `transform`, so it runs on the compositor.
 * - Edge fades are a static mask. The left fade sits in a padding strip that
 *   hangs 0.5em OUTSIDE the column (negative margin, so layout is unchanged),
 *   which keeps the first letter crisp at rest; the right fade is the last
 *   0.5em of the column, and the pan stops with the text's end just clear of
 *   it. Nothing has to animate the mask, so the fades cost nothing per frame.
 *   Ancestors must not clip (no overflow-hidden), or the left strip is cut.
 * - Pauses while its window is hidden (lib/windowShown.ts: a hidden Tauri
 *   webview still ticks animations at full rate).
 * - Reduced motion keeps the old ellipsis.
 *
 * This is the one exception to "nothing moves ambiently": Thien asked for it
 * over truncation, and it moves only text that would otherwise be cut off.
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useReducedMotion } from "motion/react";
import { MARQUEE, marqueeCycle, marqueeTravel } from "./lib/marquee";
import { isWindowShown, onWindowShown } from "./lib/windowShown";

export function Marquee({ children, className = "" }: { children: ReactNode; className?: string }) {
  const outer = useRef<HTMLSpanElement>(null);
  const inner = useRef<HTMLSpanElement>(null);
  const reduced = useReducedMotion() === true;
  // [travel px, font px]; travel 0 = the text fits (or motion is reduced).
  const [fit, setFit] = useState<[number, number]>([0, 0]);
  const [travel, fontPx] = fit;

  const measure = useCallback(() => {
    const o = outer.current;
    const i = inner.current;
    if (!o || !i) return;
    const cs = getComputedStyle(o);
    const font = parseFloat(cs.fontSize) || 12;
    // clientWidth includes the left strip while panning; take it back off
    // so the column reads the same in both states (no flip-flop).
    const column = o.clientWidth - (parseFloat(cs.paddingLeft) || 0);
    const next = reduced ? 0 : marqueeTravel(i.getBoundingClientRect().width, column, MARQUEE.fadeEm * font);
    setFit((prev) => (Math.abs(prev[0] - next) < 0.5 && prev[1] === font ? prev : [next, font]));
  }, [reduced]);

  // Every render: the text can change in place without a remount.
  useLayoutEffect(measure);

  useEffect(() => {
    const o = outer.current;
    if (!o) return;
    const ro = new ResizeObserver(() => measure());
    ro.observe(o);
    let live = true;
    void document.fonts?.ready.then(() => live && measure());
    return () => {
      live = false;
      ro.disconnect();
    };
  }, [measure]);

  useEffect(() => {
    const el = inner.current;
    if (!el || travel <= 0) return;
    const cycle = marqueeCycle(travel, fontPx);
    const anim = el.animate(cycle.keyframes, { duration: cycle.totalMs, iterations: Infinity });
    const sync = () => (isWindowShown() ? anim.play() : anim.pause());
    sync();
    const off = onWindowShown(sync);
    return () => {
      off();
      anim.cancel();
    };
  }, [travel, fontPx]);

  const panning = travel > 0;
  return (
    <span
      ref={outer}
      data-pan={panning ? "" : undefined}
      className={`marquee block min-w-0 overflow-hidden whitespace-nowrap ${panning ? "text-left" : "text-ellipsis"} ${className}`}
    >
      <span ref={inner} className={panning ? "inline-block" : undefined}>
        {children}
      </span>
    </span>
  );
}
