import { describe, expect, it } from "vitest";
import { driveWordRows, type WordRow } from "./wordWipeDriver";
import { whileShown } from "./windowShown";

function fakeStyle() {
  const values = new Map<string, string>();
  return { values,
    setProperty: (k: string, v: string | null) => { values.set(k, v ?? ""); },
    removeProperty: (k: string) => { const old = values.get(k) ?? ""; values.delete(k); return old; },
  };
}

describe("whileShown pauses a frame loop in a hidden window", () => {
  it("stops requesting frames on hide, catches up in one write on show", () => {
    let pos = 1000;
    const clockSubs = new Set<() => void>();
    const clock = { now: () => pos, isPlaying: () => true,
      subscribe: (cb: () => void) => { clockSubs.add(cb); return () => { clockSubs.delete(cb); }; } };
    let shown = true;
    const shownSubs = new Set<() => void>();
    const setShown = (v: boolean) => { shown = v; shownSubs.forEach(cb => cb()); };
    const gated = whileShown(clock, () => shown,
      cb => { shownSubs.add(cb); return () => { shownSubs.delete(cb); }; });

    let seq = 0;
    const pending = new Map<number, () => void>();
    const frames = { request: (cb: () => void) => { pending.set(++seq, cb); return seq; },
      cancel: (id: number) => { pending.delete(id); } };
    const letters = [Array.from({ length: 4 }, fakeStyle)];
    const row = { index: 0, words: [{ t: 1000, end: 1400, text: "word" }], style: fakeStyle(), letters };
    const stop = driveWordRows([row as unknown as WordRow], 0, 0, gated, frames);
    expect(pending.size).toBe(1);

    setShown(false);
    expect(pending.size).toBe(0); // no rAF behind a hidden window
    pos = 2300; // the song moves on while nobody watches
    expect(letters[0].every(s => s.values.get("--lit") !== "1.000")).toBe(true);

    setShown(true);
    expect(letters[0].every(s => s.values.get("--lit") === "1.000")).toBe(true);
    expect(pending.size).toBe(1);

    stop();
    expect(clockSubs.size).toBe(0);
    expect(shownSubs.size).toBe(0);
  });
});
