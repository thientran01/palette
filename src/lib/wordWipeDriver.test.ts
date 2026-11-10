import { describe, expect, it, vi } from "vitest";
import { attachWords, parseLrc, type LyricWord } from "./lrc";
import { letterCount } from "./letterRise";
import { driveWordRows, settleRows, type WordRow } from "./wordWipeDriver";

function style() {
  const values = new Map<string, string>();
  return { values,
    setProperty: vi.fn((key: string, value: string | null) => { values.set(key, value ?? ""); }),
    removeProperty: vi.fn((key: string) => { const old = values.get(key) ?? ""; values.delete(key); return old; }),
  };
}
type Fake = ReturnType<typeof style>;
function rowOf(index: number, words: LyricWord[]) {
  const letters = words.map(w => Array.from({ length: letterCount(w.text) }, style));
  return { index, words, style: style(), letters } satisfies WordRow & { letters: Fake[][] };
}
function row(index: number, t: number, end = t + 1000) {
  return rowOf(index, [{ t, end, text: "word" }]);
}
const lit = (s: Fake) => Number(s.values.get("--lit") ?? 0);
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

describe("data-singing marks only the rows being sung", () => {
  it("sets it while a row is active and clears it after, and on stop", () => {
    const h = harness(2200);
    const attrs = new Map<string, boolean>();
    const el = { toggleAttribute: vi.fn((name: string, on?: boolean) => { attrs.set(name, !!on); return !!on; }) };
    const live = { ...row(0, 2000, 2400), el };
    const stop = driveWordRows([live], -1, 0, h.clock, h.frames);
    expect(attrs.get("data-singing")).toBe(true);
    h.anchor(9000, false);
    expect(attrs.get("data-singing")).toBe(false);
    h.anchor(2200, false);
    expect(attrs.get("data-singing")).toBe(true);
    stop();
    expect(attrs.get("data-singing")).toBe(false);
  });
});

describe("letters rise from their own onsets", () => {
  it("lights the letters of a word in order across its sung span", () => {
    // A word before a rest spreads its letters across its whole sound.
    const h = harness(1000); const r = rowOf(0, [{ t: 1000, end: 1400, text: "한국어요" }]);
    const stop = driveWordRows([r], 0, 0, h.clock, h.frames);
    h.frame(1150);
    const [a, b, c, d] = r.letters[0].map(lit);
    expect(a).toBeGreaterThan(b); expect(b).toBeGreaterThan(c);
    expect(c).toBeGreaterThan(0); expect(d).toBe(0);
    h.frame(2300);
    expect(r.letters[0].every(s => lit(s) === 1)).toBe(true);
    stop();
  });
  it("overlaps quick syllables into one wave instead of a strobe", () => {
    const words = [0, 1, 2, 3].map(i => ({ t: 1000 + i * 100, end: 1000 + i * 100 + 80, text: "가" }));
    const h = harness(1000); const r = rowOf(0, words);
    const stop = driveWordRows([r], 0, 0, h.clock, h.frames);
    h.frame(1180);
    const levels = r.letters.map(l => lit(l[0]));
    // Neighbouring syllables are mid-rise at once, each a little behind.
    expect(levels.filter(v => v > 0 && v < 1).length).toBeGreaterThanOrEqual(2);
    expect(levels[0]).toBeGreaterThan(levels[1]);
    expect(levels[1]).toBeGreaterThan(levels[2]);
    expect(levels[2]).toBeGreaterThan(levels[3]);
    stop();
  });
  it("applies the word lead", () => {
    const h = harness(800); const r = row(0, 1000, 2000);
    const stop = driveWordRows([r], 0, 220, h.clock, h.frames);
    expect(lit(r.letters[0][0])).toBeGreaterThan(0);
    stop();
  });
  it("waits for a held syllable's next sound before rising it", () => {
    // "hall·way": the aligner heard "w" at 1900, after a long "hall".
    const words = [{ t: 1000, end: 2000, text: "hallway", points: [
      { t: 1000, fraction: 0 }, { t: 1020, fraction: 1 / 7 }, { t: 1020, fraction: 1 / 7 },
      { t: 1040, fraction: 2 / 7 }, { t: 1300, fraction: 2 / 7 }, { t: 1320, fraction: 3 / 7 },
      { t: 1400, fraction: 3 / 7 }, { t: 1420, fraction: 4 / 7 }, { t: 1900, fraction: 4 / 7 },
      { t: 1920, fraction: 5 / 7 }, { t: 1920, fraction: 5 / 7 }, { t: 1940, fraction: 6 / 7 },
      { t: 1980, fraction: 6 / 7 }, { t: 2000, fraction: 1 },
    ] }];
    const h = harness(1000); const r = rowOf(0, words);
    const stop = driveWordRows([r], 0, 0, h.clock, h.frames);
    h.frame(1800);
    const l = r.letters[0].map(lit);
    expect(l.slice(0, 4)).toEqual([1, 1, 1, 1]);
    expect(l.slice(4)).toEqual([0, 0, 0]);
    h.frame(2100);
    expect(r.letters[0].every(s => lit(s) === 1)).toBe(true);
    stop();
  });
});

describe("rows follow onsets independently of the line marker", () => {
  it("sings an upcoming row before its line becomes current", () => {
    const h = harness(2900); const upcoming = row(1, 3000);
    const stop = driveWordRows([upcoming], 0, 300, h.clock, h.frames);
    expect(upcoming.style.values.get("--word-bright")).toBe("rgb(var(--fg))");
    expect(lit(upcoming.letters[0][0])).toBeGreaterThan(0);
    expect(h.pending.size).toBe(1);
    stop();
  });
  it("keeps a sung tail and the next row active concurrently, then lets go", () => {
    const h = harness(2200); const old = row(0, 1000, 3000); const next = row(1, 2000, 4000);
    const stop = driveWordRows([old, next], 1, 0, h.clock, h.frames);
    expect(old.style.values.has("--word-bright")).toBe(true);
    expect(next.style.values.has("--word-bright")).toBe(true);
    h.frame(3300);
    expect(old.style.values.has("--word-bright")).toBe(true);
    h.frame(3500);
    expect(old.style.values.has("--word-bright")).toBe(false);
    expect(old.letters[0].some(s => s.values.has("--lit"))).toBe(false);
    stop();
  });
  it("clears future letters on a backward seek and recomputes them", () => {
    const h = harness(3200); const upcoming = row(1, 3000);
    const stop = driveWordRows([upcoming], 0, 0, h.clock, h.frames);
    const before = lit(upcoming.letters[0][0]);
    expect(before).toBeGreaterThan(0);
    h.anchor(1000, false);
    expect(upcoming.style.values.has("--word-bright")).toBe(false);
    expect(upcoming.letters[0].some(s => s.values.has("--lit"))).toBe(false);
    expect(h.pending.size).toBe(0);
    h.anchor(3020, true);
    expect(lit(upcoming.letters[0][0])).toBeGreaterThan(0);
    expect(lit(upcoming.letters[0][0])).toBeLessThan(before);
    stop();
  });
  it("runs one loop, cancels on pause, and releases everything on cleanup", () => {
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
    for (const prop of ["--word-bright", "--lyric-scale", "--lyric-opacity", "--lyric-filter"]) {
      expect(current.style.values.has(prop)).toBe(false);
    }
    expect(current.letters[0].some(s => s.values.has("--lit"))).toBe(false);
  });
  it("does not rewrite unchanged letters or animate empty panels", () => {
    const h = harness(2500); const current = row(0, 1000);
    const stop = driveWordRows([current], 0, 0, h.clock, h.frames);
    current.letters[0].forEach(s => s.setProperty.mockClear());
    h.frame(2550); h.frame(2600);
    current.letters[0].forEach(s => expect(s.setProperty).not.toHaveBeenCalled());
    stop();
    driveWordRows([], -1, 0, h.clock, h.frames)();
    expect(h.pending.size).toBe(0); expect(h.subs.size).toBe(0);
  });
});

// Backing vocals are printed last but sung concurrently: each letter keeps
// its own onset, so a backing reply rises with the lead without lighting
// future lead words.
it("lights backing letters on their own time without lighting future lead words", () => {
  const lines = attachWords(parseLrc("[02:28.19]lead future (oh, oh-oh, oh-oh)\n[02:33.67]next", 197533), [
    {t:148200,end:149000,text:"lead ",line_t:148190},
    {t:152000,end:153000,text:"future ",line_t:148190},
    {t:153628,end:153729,text:"(oh, ",line_t:148190},
    {t:153749,end:153829,text:"oh-oh, ",line_t:148190},
    {t:153849,end:154150,text:"oh-oh)",line_t:148190},
  ]);
  const words = lines.find(l => l.t === 148190)!.words!;
  const r = rowOf(0, words);
  const h = harness(149300);
  const stop = driveWordRows([r], 0, 0, h.clock, h.frames);
  expect(r.letters[0].every(s => lit(s) === 1)).toBe(true);
  expect(r.letters[1].every(s => lit(s) === 0)).toBe(true);
  expect(r.letters[2].some(s => lit(s) > 0)).toBe(true);
  h.anchor(148100, false);
  expect(r.letters.flat().every(s => lit(s) === 0)).toBe(true);
  expect(h.pending.size).toBe(0);
  stop();
});

it("keeps row bounds across both voices regardless of printed order", () => {
  const h = harness(1000);
  const r = rowOf(1, [{ t: 2000, end: 5000, text: "lead" }, { t: 1000, end: 3000, text: "(oh)" }]);
  const stop = driveWordRows([r], 0, 0, h.clock, h.frames);
  expect(r.style.values.has("--word-bright")).toBe(true);
  h.frame(5000);
  expect(r.style.values.has("--word-bright")).toBe(true);
  h.frame(5500);
  expect(r.style.values.has("--word-bright")).toBe(false);
  stop();
});

it("writes no sheen, halo or wipe: only the letters' rise", () => {
  const h = harness(1010); const r = row(1, 1000, 1500);
  const stop = driveWordRows([r], 0, 0, h.clock, h.frames);
  h.frame(1300); h.frame(1500); h.frame(1600);
  const written = new Set([...r.letters.flat().flatMap(s => s.setProperty.mock.calls.map(c => c[0])),
    ...r.style.setProperty.mock.calls.map(c => c[0])]);
  expect([...written].filter(k => /sheen|bloom|halo|peak|wipe/.test(k))).toEqual([]);
  stop();
});

describe("held notes just rise", () => {
  it("never moves a risen letter, even with a voice track", () => {
    const n = 140;
    const c = Array.from({ length: n }, (_, k) => Math.round(-600 + 70 * Math.sin(2 * Math.PI * 5.5 * k / 100)));
    const held = { t: 1000, end: 1000 + n * 10, text: "오", voice: { c, l: new Array(n).fill(80) } };
    const h = harness(1000); const r = rowOf(0, [held]);
    const stop = driveWordRows([r], 0, 0, h.clock, h.frames);
    for (let t = 1500; t < 2100; t += 50) { h.frame(t); expect(r.letters[0][0].values.has("--bend")).toBe(false); }
    stop();
  });
});

describe("a line change hands the running driver the new line", () => {
  // The panel used to rebuild the driver on every line change; now it calls
  // setCurrentLine. Both must leave every row and letter exactly the same.
  const snapshot = (rows: ReturnType<typeof row>[]) => rows.map(r => ({
    row: [...r.style.values],
    letters: r.letters.map(w => w.map(s => [...s.values])),
  }));
  it("writes the same styles as a rebuild at every frame", () => {
    const make = () => [row(0, 1000, 1600), row(1, 2000, 2600), row(2, 2400, 3500), row(3, 6000, 6400)];
    const a = make(); const b = make();
    const ha = harness(900); const hb = harness(900);
    let stopA = driveWordRows(a, 0, 0, ha.clock, ha.frames);
    const driverB = driveWordRows(b, 0, 0, hb.clock, hb.frames);
    // Line marker as the panel moves it, including a jump back (a seek).
    const lineAt = (t: number) => t < 2000 ? 0 : t < 2400 ? 1 : t < 6000 ? 2 : 3;
    let line = 0;
    for (const t of [...Array.from({ length: 400 }, (_, i) => 900 + i * 16), 1500, ...Array.from({ length: 60 }, (_, i) => 1500 + i * 16)]) {
      if (t === 1500) { ha.anchor(t); hb.anchor(t); } else { ha.frame(t); hb.frame(t); }
      const next = lineAt(t);
      if (next !== line) {
        line = next;
        stopA();
        stopA = driveWordRows(a, line, 0, ha.clock, ha.frames);
        driverB.setCurrentLine(line);
      }
      expect(snapshot(b)).toEqual(snapshot(a));
    }
    stopA(); driverB();
  });
  it("keeps the current row sung while it is current, even past its words", () => {
    const h = harness(5000); const r = row(0, 1000, 1200); const el = { toggleAttribute: vi.fn() };
    const driver = driveWordRows([{ ...r, el }], -1, 0, h.clock, h.frames);
    expect(r.style.values.has("--word-bright")).toBe(false);
    driver.setCurrentLine(0);
    expect(r.style.values.get("--word-bright")).toBe("rgb(var(--fg))");
    expect(el.toggleAttribute).toHaveBeenLastCalledWith("data-singing", true);
    driver.setCurrentLine(1);
    expect(r.style.values.has("--word-bright")).toBe(false);
    driver();
  });
});

describe("a row's letters move only while it sings or settles", () => {
  it("marks a row settling when it stops singing, and on stop", () => {
    const h = harness(1000);
    const attrs = new Map<string, boolean>();
    const el = { toggleAttribute: vi.fn((name: string, on?: boolean) => { attrs.set(name, !!on); return !!on; }) };
    const stop = driveWordRows([{ ...row(0, 1000, 1200), el }], -1, 0, h.clock, h.frames);
    expect(attrs.get("data-singing")).toBe(true);
    expect(attrs.has("data-settling")).toBe(false);
    h.frame(5000);
    expect(attrs.get("data-singing")).toBe(false);
    expect(attrs.get("data-settling")).toBe(true);
    h.anchor(1100, false);
    attrs.delete("data-settling");
    stop();
    expect(attrs.get("data-settling")).toBe(true);
  });
  it("lifts data-settling when the row's drop has eased out", () => {
    const listeners = new Map<string, (e: Event) => void>();
    const root = {
      addEventListener: (type: string, cb: (e: Event) => void) => { listeners.set(type, cb); },
      removeEventListener: (type: string) => { listeners.delete(type); },
    } as unknown as HTMLElement;
    let settling = true;
    let running: { transitionProperty: string; playState: string }[] = [];
    const rowEl = { removeAttribute: vi.fn(() => { settling = false; }) };
    const ink = { closest: () => (settling ? rowEl : null), getAnimations: () => running };
    const fire = (type: string, propertyName: string) =>
      listeners.get(type)?.({ propertyName, target: ink } as unknown as Event);
    const off = settleRows(root);
    fire("transitionend", "transform");
    expect(settling).toBe(true);
    // The row sang again mid-ease: a fresh drop is still running.
    running = [{ transitionProperty: "--drop", playState: "running" }];
    fire("transitioncancel", "--drop");
    expect(settling).toBe(true);
    running = [];
    fire("transitionend", "--drop");
    expect(settling).toBe(false);
    off();
    expect(listeners.size).toBe(0);
  });
});
