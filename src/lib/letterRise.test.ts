import { describe, expect, it } from "vitest";
import { letterLit, letterOnsets, letterSegments, RISE_MS } from "./letterRise";

describe("letters", () => {
  it("splits graphemes and leaves spaces as text", () => {
    expect(letterSegments("kiss, ").map(s => s.text)).toEqual(["k", "i", "s", "s", ",", " "]);
    expect(letterSegments("한국 ").filter(s => s.letter).map(s => s.text)).toEqual(["한", "국"]);
    expect(letterSegments("é").length).toBe(1);
  });
  it("spreads letters across the sung span, or follows checkpoints", () => {
    expect(letterOnsets({ t: 1000, end: 2000, text: "한국" })).toEqual([1000, 1500]);
    expect(letterOnsets({ t: 1000, end: 2000, text: "hallway", points: [
      { t: 1000, fraction: 0 }, { t: 1200, fraction: 4 / 7 }, { t: 1600, fraction: 4 / 7 }, { t: 2000, fraction: 1 },
    ] })).toEqual([1000, 1200, 1200, 1200, 1200, 2000, 2000]);
  });
  it("keeps onsets exact and rises for at least RISE_MS", () => {
    expect(letterLit(1000, 1010, 1000 - 31)).toBe(0);
    expect(letterLit(1000, 1010, 1000)).toBeGreaterThan(0);
    expect(letterLit(1000, 1010, 1000 + RISE_MS / 2)).toBeLessThan(1);
    expect(letterLit(1000, 1010, 1000 + RISE_MS)).toBe(1);
    // A held letter rises as slowly as it's held.
    expect(letterLit(1000, 2000, 1500)).toBeLessThan(1);
  });
});
