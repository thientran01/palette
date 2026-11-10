import { describe, expect, it } from "vitest";
import { letterLit, letterSegments, RISE_EARLY_MS, RISE_MS, rowLetterOnsets, LATIN_LETTER_MS } from "./letterRise";

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
  it("spreads a word's letters over its sound instead of a 90ms burst", () => {
    const [word] = rowLetterOnsets([w(1000, 1400, "ことばを"), w(3000, 3300, "次")]);
    expect(word[3]).toBeGreaterThan(1200);
  });
  it("keeps the pace even across a word boundary inside a phrase", () => {
    const [a, b] = rowLetterOnsets([w(1000, 1180, "하나 "), w(1200, 1400, "더해서"), w(3000, 3300, "끝")]);
    const gaps = [...a, ...b].slice(1).map((t, i) => t - [...a, ...b][i]);
    // No letter waits much longer than its neighbours (the old fill packed
    // a word into 90ms, then stood still).
    for (let i = 1; i < gaps.length; i++) expect(gaps[i] / gaps[i - 1]).toBeLessThan(2.2);
  });
  it("gives a backing reply its own timing", () => {
    const [, back] = rowLetterOnsets([w(2000, 5000, "lead "), w(1000, 1400, "(oh)", { timing: "phrase" })]);
    expect(back[0]).toBe(1000);
    expect(back[back.length - 1]).toBeLessThan(1400);
  });
});

describe("Latin words rise evenly over their own sound", () => {
  it("spaces letters evenly and lets punctuation ride along", () => {
    const [o] = rowLetterOnsets([w(1000, 1200, "away, "), w(2000, 2200, "go")]);
    expect(o.slice(0, 4)).toEqual([1000, 1050, 1100, 1150]);
    expect(o[4]).toBe(o[3]);
  });
  it("finishes a held word early in its hold", () => {
    const [o] = rowLetterOnsets([w(1000, 3000, "love")]);
    expect(o[3] - o[0]).toBeLessThanOrEqual(3 * LATIN_LETTER_MS);
  });
  it("looks the same on every repeat, whatever gap or checkpoints", () => {
    const tight = rowLetterOnsets([w(1000, 1400, "baby "), w(1420, 1600, "go")])[0].map(t => t - 1000);
    const gapped = rowLetterOnsets([w(5000, 5400, "baby ", { points: [
      { t: 5000, fraction: 0 }, { t: 5300, fraction: 0.5 }, { t: 5400, fraction: 1 },
    ] }), w(7000, 7200, "go")])[0].map(t => t - 5000);
    expect(tight).toEqual(gapped);
  });
  it("starts a leading apostrophe with the word", () => {
    const [o] = rowLetterOnsets([w(1000, 1600, "'Cause")]);
    expect(o[0]).toBe(1000);
    expect(o[1]).toBe(1000);
  });
});

describe("each rise is quick and eases in and out", () => {
  const start = 1000 - RISE_EARLY_MS;
  it("starts gently just before the onset and lands soon after it", () => {
    expect(letterLit(1000, start)).toBe(0);
    expect(letterLit(1000, start + 10)).toBeLessThan(0.01);
    expect(letterLit(1000, 1000)).toBeGreaterThan(0.2);
    expect(letterLit(1000, start + RISE_MS / 2)).toBeCloseTo(0.5, 6);
    expect(letterLit(1000, start + RISE_MS)).toBe(1);
    expect(start + RISE_MS - 1000).toBeLessThanOrEqual(150);
  });
});
