import { describe, expect, it, vi } from "vitest";
import { attachWords, parseLrc } from "./lrc";
import { driveWordRows, WIPE_FEATHER_PX, type WordRow } from "./wordWipeDriver";

function style() {
  const values = new Map<string, string>();
  return { values,
    setProperty: vi.fn((key: string, value: string | null) => { values.set(key, value ?? ""); }),
    removeProperty: vi.fn((key: string) => { const old = values.get(key) ?? ""; values.delete(key); return old; }),
  };
}
/** A span's lit share from its --wipe (px): 0 dark, 1 fully lit. Test rows
 * carry no measured widths, so the driver estimates 10px per letter. */
function lit(span: { values: Map<string, string> }, letters = 4): number {
  const edge = WIPE_FEATHER_PX + 1;
  return (parseFloat(span.values.get("--wipe")!) + edge) / (10 * letters + 2 * edge);
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

describe("data-wiping marks only the rows being painted", () => {
  it("sets it while a row is active and clears it after, and on stop", () => {
    const h = harness(2200);
    const attrs = new Map<string, boolean>();
    const el = { toggleAttribute: vi.fn((name: string, on?: boolean) => { attrs.set(name, !!on); return !!on; }) };
    const live = { ...row(0, 2000, 2400), el };
    const stop = driveWordRows([live], -1, 0, h.clock, h.frames);
    expect(attrs.get("data-wiping")).toBe(true);
    h.anchor(9000, false);
    expect(attrs.get("data-wiping")).toBe(false);
    h.anchor(2200, false);
    expect(attrs.get("data-wiping")).toBe(true);
    stop();
    expect(attrs.get("data-wiping")).toBe(false);
  });
});

describe("word rows follow onsets independently of the line marker", () => {
  it("paints an upcoming word before its line becomes current", () => {
    const h = harness(2800); const upcoming = row(1, 3000);
    const stop = driveWordRows([upcoming], 0, 300, h.clock, h.frames);
    expect(upcoming.style.values.get("--word-bright")).toBe("rgb(var(--fg))");
    expect(lit(upcoming.spans[0])).toBeGreaterThan(0);
    expect(h.pending.size).toBe(1);
    stop();
  });
  it("keeps a sung tail and the next row active concurrently", () => {
    const h = harness(2200); const old = row(0, 1000, 3000); const next = row(1, 2000, 4000);
    const stop = driveWordRows([old, next], 1, 0, h.clock, h.frames);
    expect(old.style.values.has("--word-bright")).toBe(true);
    expect(next.style.values.has("--word-bright")).toBe(true);
    h.frame(3000);
    expect(old.style.values.has("--word-bright")).toBe(true);
    h.frame(3400);
    expect(old.style.values.has("--word-bright")).toBe(false);
    expect(next.style.values.has("--word-bright")).toBe(true);
    stop();
  });
  it("clears future highlights on a backward seek and recomputes their wipe", () => {
    const h = harness(3200); const upcoming = row(1, 3000);
    const stop = driveWordRows([upcoming], 0, 0, h.clock, h.frames);
    const before = lit(upcoming.spans[0]);
    expect(before).toBeGreaterThan(0); expect(before).toBeLessThan(1);
    h.anchor(1000, false);
    expect(upcoming.style.values.has("--word-bright")).toBe(false);
    expect(upcoming.style.values.has("--word-peak")).toBe(false);
    expect(upcoming.style.values.has("--lyric-scale")).toBe(false);
    expect(upcoming.style.values.has("--lyric-opacity")).toBe(false);
    expect(upcoming.style.values.has("--lyric-filter")).toBe(false);
    expect(h.pending.size).toBe(0);
    h.anchor(3020, true);
    const wipe = lit(upcoming.spans[0]);
    expect(wipe).toBeGreaterThan(0); expect(wipe).toBeLessThan(before);
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
    expect(current.style.values.has("--word-peak")).toBe(false);
    expect(current.style.values.has("--lyric-scale")).toBe(false);
    expect(current.style.values.has("--lyric-opacity")).toBe(false);
    expect(current.style.values.has("--lyric-filter")).toBe(false);
  });
  it("does not write unchanged spans or animate empty panels", () => {
    const h = harness(2500); const current = row(0, 1000);
    const stop = driveWordRows([current], 0, 0, h.clock, h.frames);
    current.spans[0].setProperty.mockClear();
    h.frame(2550); h.frame(2600);
    expect(current.spans[0].setProperty).not.toHaveBeenCalled();
    stop();
    driveWordRows([], -1, 0, h.clock, h.frames)();
    expect(h.pending.size).toBe(0); expect(h.subs.size).toBe(0);
  });
});

// Backing vocals are printed last but sung concurrently. A single cursor
// used to fill future lead words or skip the entire parenthesized tail.
it("advances lead and backing spans independently without filling future lead words", () => {
  const lines = attachWords(parseLrc("[02:28.19]lead future (oh, oh-oh, oh-oh)\n[02:33.67]next", 197533), [
    {t:148200,end:149000,text:"lead ",line_t:148190},
    {t:152000,end:153000,text:"future ",line_t:148190},
    {t:153628,end:153729,text:"(oh, ",line_t:148190},
    {t:153749,end:153829,text:"oh-oh, ",line_t:148190},
    {t:153849,end:154150,text:"oh-oh)",line_t:148190},
  ]);
  const words=lines.find(l => l.t === 148190)!.words!;
  const spans=words.map(() => style());
  const h=harness(149000);
  const stop=driveWordRows([{index:0,words,spans,style:style()}],0,0,h.clock,h.frames);
  const letters=words.map(w=>[...w.text].length);
  expect(lit(spans[0],letters[0])).toBeCloseTo(1,6);
  expect(lit(spans[1],letters[1])).toBeCloseTo(0,6);
  expect(lit(spans[2],letters[2])).toBeGreaterThan(0);
  h.anchor(148190,false);
  expect(spans.every((s,i)=>Math.abs(lit(s,letters[i]))<1e-9)).toBe(true);
  expect(h.pending.size).toBe(0);
  stop();
});

it("keeps row bounds across both voices regardless of printed order", () => {
  const h=harness(1000);const css=style();const spans=[style(),style()];
  const words=[{t:2000,end:5000,text:"lead"},{t:1000,end:3000,text:"(oh)"}];
  const stop=driveWordRows([{index:1,style:css,spans,words}],0,0,h.clock,h.frames);
  expect(css.values.has("--word-bright")).toBe(true);
  h.frame(4000);
  expect(css.values.has("--word-bright")).toBe(true);
  h.frame(5000);
  expect(css.values.has("--word-bright")).toBe(true);
  h.frame(5400);
  expect(css.values.has("--word-bright")).toBe(false);
  stop();
});

// One soft light moves through the row: no white sheen on the filling word
// and no halo when it completes (they flickered at syllable rate).
it("writes only the wipe: no sheen or completion halo", () => {
  const h=harness(1010);const current=row(1,1000,1500);
  const stop=driveWordRows([current],0,0,h.clock,h.frames);
  h.frame(1300);h.frame(1500);h.frame(1600);
  const written=new Set([...current.spans[0].setProperty.mock.calls.map(c=>c[0]),...current.style.setProperty.mock.calls.map(c=>c[0])]);
  expect([...written].filter(k=>/sheen|bloom|halo|peak/.test(k))).toEqual([]);
  expect(lit(current.spans[0])).toBeCloseTo(1,6);
  stop();
});

it("slows on a held letter and applies the existing lead offset", () => {
  const current=row(0,1000,2000);
  const detailed={...current,words:[{t:1000,end:2000,text:"hallway",points:[
    {t:1000,fraction:0},{t:1200,fraction:.5},{t:1900,fraction:.5},{t:2000,fraction:1},
  ]}]};
  const h=harness(1100);
  const stop=driveWordRows([detailed],0,220,h.clock,h.frames);
  // 220ms lead: the clock at 1680 is the song at 1900, where the held
  // first half ends; it took the whole hold, not 200ms.
  h.frame(1230);
  expect(lit(current.spans[0],7)).toBeLessThan(.4);
  h.frame(1680);
  expect(lit(current.spans[0],7)).toBeCloseTo(.5,6);
  // The second half gets the letter pace (55ms a letter), not 100ms.
  h.frame(1780);
  expect(lit(current.spans[0],7)).toBeLessThan(1);
  h.frame(1880);
  expect(lit(current.spans[0],7)).toBeCloseTo(1,6);
  stop();
});

it("does not rewrite identical CSS values as fractional progress changes", () => {
  const h=harness(1100);const r=row(0,1000,3000);
  const stop=driveWordRows([r],0,0,h.clock,h.frames);
  r.spans[0].setProperty.mockClear();
  h.frame(1100.001);
  expect(r.spans[0].setProperty).not.toHaveBeenCalled();
  h.frame(1300);
  expect(r.spans[0].setProperty).toHaveBeenCalledWith("--wipe",expect.stringMatching(/^-?\d+\.\dpx$/));
  stop();
});
