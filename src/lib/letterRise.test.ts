import { describe, expect, it } from "vitest";
import { letterLit, letterSegments, RISE_EARLY_MS, RISE_MS, rowLetterOnsets } from "./letterRise";

const w = (t: number, end: number, text: string, extra = {}) => ({ t, end, text, ...extra });

describe("letters", () => {
  it("splits graphemes and leaves spaces as text", () => {
    expect(letterSegments("kiss, ").map(s => s.text)).toEqual(["k", "i", "s", "s", ",", " "]);
    expect(letterSegments("한국 ").filter(s => s.letter).map(s => s.text)).toEqual(["한", "국"]);
    expect(letterSegments("é").length).toBe(1);
  });
});

describe("letter onsets follow one smooth cursor per row", () => {
  it("starts every word exactly on its onset", () => {
    const words = [w(1000, 1180, "one "), w(1200, 1400, "more "), w(1420, 2600, "kiss, "), w(3000, 3300, "wine")];
    const onsets = rowLetterOnsets(words);
    words.forEach((word, i) => expect(onsets[i][0]).toBe(word.t));
    const flat = onsets.flat();
    for (let i = 1; i < flat.length; i++) expect(flat[i]).toBeGreaterThanOrEqual(flat[i - 1]);
  });
  it("spreads a short word's letters over its sound instead of a 90ms burst", () => {
    const [word] = rowLetterOnsets([w(1000, 1400, "word"), w(3000, 3300, "next")]);
    expect(word[3]).toBeGreaterThan(1200);
  });
  it("keeps the pace even across a word boundary inside a phrase", () => {
    const [a, b] = rowLetterOnsets([w(1000, 1180, "one "), w(1200, 1400, "more"), w(3000, 3300, "x")]);
    const gaps = [...a, ...b].slice(1).map((t, i) => t - [...a, ...b][i]);
    // No letter waits much longer than its neighbours (the old fill packed
    // "one" into 90ms, then stood still for 110ms).
    for (let i = 1; i < gaps.length; i++) expect(gaps[i] / gaps[i - 1]).toBeLessThan(2.2);
  });
  it("holds the letter the aligner heard held, the same reading as the fill", () => {
    const kiss = w(1000, 2700, "kiss,", { points: [
      { t: 1000, fraction: 0 }, { t: 1020, fraction: 0.2 }, { t: 1240, fraction: 0.2 },
      { t: 1260, fraction: 0.4 }, { t: 2600, fraction: 0.4 }, { t: 2620, fraction: 0.8 },
      { t: 2680, fraction: 0.8 }, { t: 2700, fraction: 1 },
    ] });
    const [o] = rowLetterOnsets([kiss, w(3500, 3700, "wine")]);
    expect(o[1]).toBeCloseTo(1240, -1);
    expect(o[2]).toBeCloseTo(2600, -1);
  });
  it("gives a backing reply its own timing", () => {
    const [, back] = rowLetterOnsets([w(2000, 5000, "lead "), w(1000, 1400, "(oh)", { timing: "phrase" })]);
    expect(back[0]).toBe(1000);
    expect(back[back.length - 1]).toBeLessThan(1400);
  });
});

describe("each rise eases in and out", () => {
  const start = 1000 - RISE_EARLY_MS;
  it("starts gently a little before the onset and lands after RISE_MS", () => {
    expect(letterLit(1000, 1010, start)).toBe(0);
    expect(letterLit(1000, 1010, start + 10)).toBeLessThan(0.01);
    expect(letterLit(1000, 1010, 1000)).toBeGreaterThan(0.1);
    expect(letterLit(1000, 1010, start + RISE_MS / 2)).toBeCloseTo(0.5, 6);
    expect(letterLit(1000, 1010, start + RISE_MS)).toBe(1);
  });
  it("rises as slowly as a letter is held", () => {
    expect(letterLit(1000, 2000, 1500)).toBeLessThan(1);
  });
});
