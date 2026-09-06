import { describe, expect, it, vi } from "vitest";
import { attachWords, parseLrc } from "./lrc";
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
    expect(upcoming.spans[0].values.get("--wipe")).toBe("20.0%");
    h.anchor(1000, false);
    expect(upcoming.style.values.has("--word-bright")).toBe(false);
    expect(upcoming.style.values.has("--word-peak")).toBe(false);
    expect(upcoming.style.values.has("--lyric-scale")).toBe(false);
    expect(upcoming.style.values.has("--lyric-blur")).toBe(false);
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
    expect(current.style.values.has("--word-peak")).toBe(false);
    expect(current.style.values.has("--lyric-scale")).toBe(false);
    expect(current.style.values.has("--lyric-blur")).toBe(false);
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
  expect(spans[0].values.get("--wipe")).toBe("100.0%");
  expect(spans[1].values.get("--wipe")).toBe("0.0%");
  expect(parseFloat(spans[2].values.get("--wipe")!)).toBeGreaterThan(0);
  h.anchor(148190,false);
  expect(spans.every(s=>s.values.get("--wipe")==="0.0%")).toBe(true);
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
  expect(css.values.has("--word-bright")).toBe(false);
  stop();
});

it("releases the singing bloom and clears its color on seek and disposal", () => {
  const h=harness(1010);const current=row(1,1000,1500);
  const stop=driveWordRows([current],0,0,h.clock,h.frames);
  expect(current.spans[0].values.get("--word-bloom")).toContain("var(--word-halo, transparent)");
  expect(current.style.values.has("--word-halo")).toBe(true);
  h.anchor(1100,false);
  const paused=current.spans[0].values.get("--word-bloom");
  expect(h.pending.size).toBe(0);
  h.frame(1200);
  expect(current.spans[0].values.get("--word-bloom")).toBe(paused);
  h.anchor(1260,true);
  expect(current.spans[0].values.get("--word-bloom")).toBe("none");
  h.anchor(1010,true);
  h.anchor(500,false);
  expect(current.style.values.has("--word-halo")).toBe(false);
  h.anchor(1010,true);
  stop();
  expect(current.style.values.has("--word-halo")).toBe(false);
});


it("keeps Orchestra backing moving after the lead and resets the same spans on seek", () => {
  const lines=attachWords(parseLrc('[00:10.86]Hah (Slow slow)\n[00:15.07]Ad astra',177000),[
    {t:10881,end:11081,text:'Hah ',line_t:10860},
    {t:11101,end:11262,text:'(Slow ',line_t:10860},
    {t:11302,end:11582,text:'slow)',line_t:10860},
  ]);
  const words=lines.find(l=>l.t===10860)!.words!;
  const spans=words.map(()=>style());const h=harness(13800);
  const stop=driveWordRows([{index:0,words,spans,style:style()}],0,220,h.clock,h.frames);
  expect(spans[0].values.get('--wipe')).toBe('100.0%');
  const progress=Number.parseFloat(spans[2].values.get('--wipe')!);
  expect(progress).toBeGreaterThan(0);expect(progress).toBeLessThan(100);
  h.anchor(10640,false);
  expect(spans.every(s=>s.values.get('--wipe')==='0.0%')).toBe(true);
  expect(h.pending.size).toBe(0);
  h.anchor(14850,true);
  expect(spans.every(s=>s.values.get('--wipe')==='100.0%')).toBe(true);
  stop();expect(h.pending.size).toBe(0);
});
