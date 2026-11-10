import { describe, expect, it } from "vitest";
import { letterLit, letterSegments, letterSnap, riseCurve, RISE_EARLY_MS, RISE_MS, rowLetterOnsets, SNAP_EARLY_MS, SNAP_MS, SYLLABLE_LAG_MS } from "./letterRise";

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

/** Checkpoints the way acoustic.rs spelling_points writes them: each
 * letter's CTC span as a (start, its offset) and (end, next offset) pair. */
function spelled(text: string, heard: number[], end: number) {
  const width = text.trimEnd().length;
  const at = [...text].map((c, i) => [c, i] as const).filter(([c]) => /[a-z']/i.test(c)).map(([, i]) => i);
  const points = at.flatMap((offset, k) => [
    { t: heard[k], fraction: k === 0 ? 0 : offset / width },
    { t: k + 1 === at.length ? end : heard[k] + 20, fraction: k + 1 === at.length ? 1 : at[k + 1] / width },
  ]);
  return { t: heard[0], end, text, points };
}

describe("Latin words rise a syllable at a time", () => {
  it("starts each syllable when the aligner heard it", () => {
    // "away," sung "a" (held) then "way": w heard at +240, y at the end.
    const [o] = rowLetterOnsets([spelled("away, ", [1000, 1240, 1300, 1500], 1520), w(2000, 2200, "go")]);
    expect(o[0]).toBe(1000);
    expect(o[1]).toBe(1240);
    // "ay" rides with "w" (one sound), not on the aligner's late "y".
    expect(o[3] - o[1]).toBeLessThanOrEqual(2 * SYLLABLE_LAG_MS);
    // The comma rises with the letter before it.
    expect(o[4]).toBe(o[3]);
  });
  it("rises a one-syllable word as one sound", () => {
    const [o] = rowLetterOnsets([w(1000, 1300, "strength")]);
    expect(o[o.length - 1] - o[0]).toBeLessThanOrEqual(7 * SYLLABLE_LAG_MS);
  });
  it("looks the same on every repeat, whatever gap follows", () => {
    const tight = rowLetterOnsets([w(1000, 1400, "baby "), w(1420, 1600, "go")])[0].map(t => t - 1000);
    const gapped = rowLetterOnsets([w(5000, 5400, "baby "), w(7000, 7200, "go")])[0].map(t => t - 5000);
    expect(tight).toEqual(gapped);
    // No checkpoints: "ba·by" splits the word's own sound evenly.
    expect(tight[2]).toBe(200);
  });
  it("keeps every word's letters in order", () => {
    for (const text of ["pretending,", "beautiful", "'Cause", "you're", "Hmm"]) {
      const [o] = rowLetterOnsets([w(1000, 1900, text)]);
      for (let i = 1; i < o.length; i++) expect(o[i]).toBeGreaterThanOrEqual(o[i - 1]);
      expect(o[o.length - 1]).toBeLessThanOrEqual(1900);
    }
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

describe("a syllable snaps up", () => {
  it("moves fast first and lands sooner than a letter's rise", () => {
    expect(letterSnap(1000, 1000 - SNAP_EARLY_MS)).toBe(0);
    expect(letterSnap(1000, 1000)).toBeGreaterThan(letterLit(1000, 1000));
    expect(letterSnap(1000, 1015)).toBeGreaterThan(0.5);
    expect(letterSnap(1000, 1000 - SNAP_EARLY_MS + SNAP_MS)).toBe(1);
    expect(SNAP_MS - SNAP_EARLY_MS).toBeLessThan(RISE_MS - RISE_EARLY_MS);
  });
  it("is used for Latin words only", () => {
    expect(riseCurve(w(0, 100, "love"))).toBe(letterSnap);
    expect(riseCurve(w(0, 100, "사랑"))).toBe(letterLit);
    expect(riseCurve(w(0, 100, "(oh)", { timing: "phrase" }))).toBe(letterLit);
  });
});
