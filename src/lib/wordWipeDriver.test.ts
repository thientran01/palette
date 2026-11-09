import { describe, expect, it, vi } from "vitest";
import { attachWords, parseLrc, type LyricWord } from "./lrc";
import { letterCount } from "./letterRise";
import { driveWordRows, type WordRow } from "./wordWipeDriver";

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
    h.frame(1290);
    const levels = r.letters.map(l => lit(l[0]));
    // Several syllables are mid-rise at once, each a little behind.
    expect(levels.filter(v => v > 0 && v < 1).length).toBeGreaterThanOrEqual(3);
    for (let i = 1; i < levels.length; i++) expect(levels[i]).toBeLessThan(levels[i - 1]);
    stop();
  });
  it("applies the word lead", () => {
    const h = harness(800); const r = row(0, 1000, 2000);
    const stop = driveWordRows([r], 0, 220, h.clock, h.frames);
    expect(lit(r.letters[0][0])).toBeGreaterThan(0);
    stop();
  });
  it("paces a held letter by the aligner's checkpoints", () => {
    const words = [{ t: 1000, end: 2000, text: "hallway", points: [
      { t: 1000, fraction: 0 }, { t: 1050, fraction: 2 / 7 }, { t: 1900, fraction: 2 / 7 },
      { t: 1950, fraction: 6 / 7 }, { t: 2000, fraction: 1 },
    ] }];
    const h = harness(1000); const r = rowOf(0, words);
    const stop = driveWordRows([r], 0, 0, h.clock, h.frames);
    h.frame(1800);
    const l = r.letters[0].map(lit);
    // The checkpoint's plateau is held (lrc.ts sampleFill's reading): "a"
    // is partway through its long rise, later letters wait for 1900.
    expect(l[0]).toBe(1);
    expect(l[1]).toBeGreaterThan(0.1); expect(l[1]).toBeLessThan(1);
    expect(l[2]).toBe(0);
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

describe("a held note's letters ride the singer's pitch", () => {
  const n = 140;
  const c = Array.from({ length: n }, (_, k) => k < 30 ? -600 : Math.round(-600 + 70 * Math.sin(2 * Math.PI * 5.5 * (k - 30) / 100)));
  const held = { t: 1000, end: 1000 + n * 10, text: "oh", voice: { c, l: new Array(n).fill(80) } };
  it("sways risen letters with vibrato and settles after", () => {
    const h = harness(1000); const r = rowOf(0, [held]);
    const stop = driveWordRows([r], 0, 0, h.clock, h.frames);
    const bends: number[] = [];
    for (let t = 1500; t < 2100; t += 10) { h.frame(t); bends.push(parseFloat(r.letters[0][0].values.get("--bend") ?? "0")); }
    expect(Math.max(...bends) - Math.min(...bends)).toBeGreaterThan(1.4);
    h.frame(2400 + 120 + 300);
    expect(r.letters[0][0].values.has("--bend")).toBe(false);
    stop();
  });
  it("never bends a short syllable, even with a voice track", () => {
    const h = harness(1000);
    const r = rowOf(0, [{ t: 1000, end: 1100, text: "가", voice: { c: [-600, 600, -600, 600, -600], l: [90, 90, 90, 90, 90] } }]);
    const stop = driveWordRows([r], 0, 0, h.clock, h.frames);
    for (const t of [1020, 1040, 1060]) { h.frame(t); expect(r.letters[0][0].values.has("--bend")).toBe(false); }
    stop();
  });
});
