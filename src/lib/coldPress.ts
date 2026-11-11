/*
 * A press that arrives before any hover ("cold"), on a control that only
 * takes pointer events once hovered.
 *
 * The widget's corner controls (the mode ladder, the queue seat, the pill's
 * scrim) sit at opacity 0 and pointer-events none until the root's data-hot,
 * which mousemove arms. Normally the cursor's travel arms it long before any
 * press. But when the press is the FIRST pointer event the webview sees (the
 * cursor arrived while the window was still click-through, or Windows hid
 * the pointer after typing and swallowed the moves), the hit test runs while
 * the control is still pointer-events none. The press lands on the shell
 * underneath, the root starts a native window drag, the drag loop eats the
 * mouseup, and the click never fires. The control fades in a frame later and
 * the next click works: "focus needs two clicks" (seen twice while recording
 * the promo, 2026-11-10).
 *
 * The fix: on a cold press that isn't already on a control, reveal first
 * (synchronously), then hit-test again. If a control is there now, the press
 * was aimed at it: skip the drag and replay the click on release, if the
 * release lands on the same control. Anything else stays a drag.
 */

export interface ColdPressDeps<T> {
  /** Commit the hover reveal synchronously, so the next hit test sees the
   * controls as hit-testable. */
  reveal(): void;
  /** The control under a point, or null. */
  controlAt(x: number, y: number): T | null;
  /** Wait for the press's release. Calls back with the release point, or
   * with null when the release can't be seen (it left the window, or another
   * press came first). */
  awaitRelease(cb: (at: { x: number; y: number } | null) => void): void;
  /** Act on the control (a button's click). */
  activate(control: T): void;
}

/** True when the press belongs to a control (don't start a drag). */
export function routeColdPress<T>(
  at: { x: number; y: number },
  hot: boolean,
  deps: ColdPressDeps<T>,
): boolean {
  // Already hovered: the hit test was already right, and a press on bare
  // shell is a drag.
  if (hot) return false;
  deps.reveal();
  const control = deps.controlAt(at.x, at.y);
  if (control === null) return false;
  deps.awaitRelease((up) => {
    if (up !== null && deps.controlAt(up.x, up.y) === control) deps.activate(control);
  });
  return true;
}

/** The DOM wiring App uses. `reveal` is App's flushSync(setHot(true)). */
export function domColdPressDeps(reveal: () => void): ColdPressDeps<HTMLElement> {
  return {
    reveal,
    controlAt(x, y) {
      const el = document.elementFromPoint(x, y);
      const control = el?.closest<HTMLElement>('button, [role="slider"]') ?? null;
      if (control === null) return null;
      if (control instanceof HTMLButtonElement && control.disabled) return null;
      if (control.getAttribute("aria-disabled") === "true") return null;
      return control;
    },
    awaitRelease(cb) {
      const done = (at: { x: number; y: number } | null) => {
        window.removeEventListener("mouseup", onUp, true);
        window.removeEventListener("mousedown", onDown, true);
        cb(at);
      };
      const onUp = (e: MouseEvent) => {
        if (e.button === 0) done({ x: e.clientX, y: e.clientY });
      };
      // A release outside the window never arrives here; the next press
      // retires the wait, so a stale listener can't replay a later click.
      const onDown = () => done(null);
      window.addEventListener("mouseup", onUp, true);
      window.addEventListener("mousedown", onDown, true);
    },
    activate(control) {
      // A slider has no click to replay (a scrub needs the drag itself), so
      // a cold press on one just doesn't move the window.
      if (control instanceof HTMLButtonElement) control.click();
    },
  };
}
