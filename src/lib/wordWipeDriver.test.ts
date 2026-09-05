import { describe, expect, it, vi } from "vitest";
import { driveWordRows, type WordRow } from "./wordWipeDriver";

function style() {
  const values = new Map<string, string>();
  return { values,
    setProperty: vi.fn((key: string, value: string | null) => { values.set(key, value ?? ""); }),
    removeProperty: vi.fn((key: string) => { const old = values.get(key) ?? ""; values.delete(key); return old; }),
  };
}
function row(index: number, t: number, end = t + 1000) {
  const css = style(); const span = style();
  return { index, words: [{ t, end, text: "word" }], style: css, spans: [span] } satisfies WordRow;
}
function harness(pos = 0, playing = true) {
  let seq = 0;
  const pending = new Map<number, () => void>();
  const subs = new Set<() => void>();
  return {
    clock: { now: () => pos, isPlaying: () => playing,
      subscribe: (cb: () => void) => { subs.add(cb); return () => { subs.delete(cb); }; } },
    frames: { request: (cb: () => void) => { const id = ++seq; pending.set(id, cb); return id; },
      cancel: (id: number) => { pending.delete(id); } },
    anchor: (next: number, running = playing) => { pos = next; playing = running; subs.forEach(cb => cb()); },
    frame: (next: number) => { pos = next; const jobs = [...pending.values()]; pending.clear(); jobs.forEach(cb => cb()); },
    pending, subs,
  };
}

describe("word rows follow onsets independently of the line marker", () => {
  it("paints an upcoming word before its line becomes current", () => {
    const h = harness(2800); const upcoming = row(1, 3000);
    const stop = driveWordRows([upcoming], 0, 210, h.clock, h.frames);
    expect(upcoming.style.values.get("--word-bright")).toBe("rgb(var(--fg))");
    expect(parseFloat(upcoming.spans[0].values.get("--wipe")!)).toBeGreaterThan(0);
    expect(h.pending.size).toBe(1);
    stop();
  });
  it("keeps a sung tail and the next row active concurrently", () => {
    const h = harness(2200); const old = row(0, 1000, 3000); const next = row(1, 2000, 4000);
    const stop = driveWordRows([old, next], 1, 0, h.clock, h.frames);
    expect(old.style.values.has("--word-bright")).toBe(true);
    expect(next.style.values.has("--word-bright")).toBe(true);
    h.frame(3000);
    expect(old.style.values.has("--word-bright")).toBe(false);
    expect(next.style.values.has("--word-bright")).toBe(true);
    stop();
  });
  it("clears future highlights on a backward seek and recomputes their wipe", () => {
    const h = harness(3200); const upcoming = row(1, 3000);
    const stop = driveWordRows([upcoming], 0, 0, h.clock, h.frames);
    expect(upcoming.spans[0].values.get("--wipe")).toBe("100.0%");
    h.anchor(1000, false);
    expect(upcoming.style.values.has("--word-bright")).toBe(false);
    expect(h.pending.size).toBe(0);
    h.anchor(3020, true);
    const wipe = parseFloat(upcoming.spans[0].values.get("--wipe")!);
    expect(wipe).toBeGreaterThan(0); expect(wipe).toBeLessThan(100);
    stop();
  });
  it("runs one loop, cancels on pause, and releases subscriptions on cleanup", () => {
    const h = harness(1500); const current = row(0, 1000);
    const stop = driveWordRows([current], 0, 0, h.clock, h.frames);
    h.anchor(1550); h.anchor(1600);
    expect(h.pending.size).toBe(1);
    h.anchor(1600, false);
    expect(h.pending.size).toBe(0);
    h.anchor(1700, true);
    expect(h.pending.size).toBe(1);
    stop();
    expect(h.pending.size).toBe(0); expect(h.subs.size).toBe(0);
    expect(current.style.values.has("--word-bright")).toBe(false);
  });
  it("does not write unchanged spans or animate empty panels", () => {
    const h = harness(1500); const current = row(0, 1000);
    const stop = driveWordRows([current], 0, 0, h.clock, h.frames);
    current.spans[0].setProperty.mockClear();
    h.frame(1550); h.frame(1600);
    expect(current.spans[0].setProperty).not.toHaveBeenCalled();
    stop();
    driveWordRows([], -1, 0, h.clock, h.frames)();
    expect(h.pending.size).toBe(0); expect(h.subs.size).toBe(0);
  });
});
