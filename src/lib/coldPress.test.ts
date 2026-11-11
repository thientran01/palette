import { describe, expect, it } from "vitest";
import { routeColdPress, type ColdPressDeps } from "./coldPress";

/** A fake widget: `expand` is hit-testable only once revealed, the way the
 * corner cluster is pointer-events none until data-hot. */
function fakeWidget(opts: { controlUnderPress?: boolean } = {}) {
  const { controlUnderPress = true } = opts;
  const log: string[] = [];
  let revealed = false;
  let release: ((at: { x: number; y: number } | null) => void) | null = null;
  const deps: ColdPressDeps<string> = {
    reveal: () => {
      revealed = true;
      log.push("reveal");
    },
    controlAt: (x) => (revealed && controlUnderPress && x < 10 ? "expand" : null),
    awaitRelease: (cb) => {
      release = cb;
    },
    activate: (c) => log.push(`click ${c}`),
  };
  return {
    deps,
    log,
    release: (at: { x: number; y: number } | null) => release?.(at),
    waiting: () => release !== null,
  };
}

describe("a press before any hover", () => {
  it("reveals, skips the drag, and clicks the control on release", () => {
    const w = fakeWidget();
    expect(routeColdPress({ x: 5, y: 5 }, false, w.deps)).toBe(true);
    expect(w.log).toEqual(["reveal"]);
    w.release({ x: 6, y: 5 });
    expect(w.log).toEqual(["reveal", "click expand"]);
  });

  it("doesn't click when the release lands off the control", () => {
    const w = fakeWidget();
    expect(routeColdPress({ x: 5, y: 5 }, false, w.deps)).toBe(true);
    w.release({ x: 50, y: 5 });
    expect(w.log).toEqual(["reveal"]);
  });

  it("doesn't click when the release is never seen", () => {
    const w = fakeWidget();
    routeColdPress({ x: 5, y: 5 }, false, w.deps);
    w.release(null);
    expect(w.log).toEqual(["reveal"]);
  });

  it("stays a drag on bare shell", () => {
    const w = fakeWidget({ controlUnderPress: false });
    expect(routeColdPress({ x: 5, y: 5 }, false, w.deps)).toBe(false);
    expect(w.waiting()).toBe(false);
  });
});

describe("a press after hover", () => {
  it("is left alone: the hit test was already right", () => {
    const w = fakeWidget();
    expect(routeColdPress({ x: 5, y: 5 }, true, w.deps)).toBe(false);
    expect(w.log).toEqual([]);
    expect(w.waiting()).toBe(false);
  });
});
