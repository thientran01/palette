import { describe, expect, it } from "vitest";
import { BANDS_STALE_MS, BarRange, RANGE_MIN_SPAN, bandsStale } from "./waveformFrame";

describe("bandsStale", () => {
  it("keeps a live ~30Hz stream fresh, even with a few late packets", () => {
    expect(bandsStale(1033, 1000)).toBe(false);
    expect(bandsStale(1000 + 4 * 33, 1000)).toBe(false);
  });

  it("keeps a payload that landed after the frame started fresh", () => {
    // rAF's timestamp is the frame start, so it can precede the payload.
    expect(bandsStale(1000, 1004)).toBe(false);
  });

  it("reads a stream that stopped as silence", () => {
    expect(bandsStale(1000 + BANDS_STALE_MS + 1, 1000)).toBe(true);
  });
});

describe("BarRange", () => {
  const run = (r: BarRange, values: number[]) => values.map((e) => r.step(e, 33));

  it("stretches a bin that only wobbles mid-height over the bar's full range", () => {
    const r = new BarRange();
    // A real-song bin: sits near 0.35 and the beat nudges it to ~0.6.
    const wobble = Array.from({ length: 300 }, (_, i) => (i % 15 < 3 ? 0.6 : 0.35));
    const out = run(r, wobble).slice(-60);
    expect(Math.max(...out) - Math.min(...out)).toBeGreaterThan(0.6);
  });

  it("keeps a held, nearly flat bin from blowing its noise up to full height", () => {
    const r = new BarRange();
    const held = Array.from({ length: 300 }, (_, i) => 0.5 + (i % 2 ? 0.01 : -0.01));
    const out = run(r, held).slice(-60);
    expect(Math.max(...out) - Math.min(...out)).toBeLessThan((0.02 / RANGE_MIN_SPAN) * 1.01);
  });

  it("still draws a quiet section shorter than a loud one", () => {
    const loud = new BarRange();
    const quiet = new BarRange();
    const beat = (hi: number, lo: number) => Array.from({ length: 300 }, (_, i) => (i % 15 < 3 ? hi : lo));
    const peakLoud = Math.max(...run(loud, beat(0.9, 0.5)).slice(-60));
    const peakQuiet = Math.max(...run(quiet, beat(0.3, 0.1)).slice(-60));
    expect(peakQuiet).toBeLessThan(peakLoud);
  });

  it("stays at the floor through silence", () => {
    const r = new BarRange();
    run(r, Array.from({ length: 100 }, () => 0.7));
    const out = run(r, Array.from({ length: 200 }, () => 0));
    expect(out[out.length - 1]).toBe(0);
  });
});
