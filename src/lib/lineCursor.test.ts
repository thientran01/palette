import { describe, expect, it } from "vitest";
import { lineCursor, MIN_LETTER_MS } from "./lineCursor";

const w = (t: number, end: number, text: string, extra = {}) => ({ t, end, text, ...extra });

describe("the row cursor keeps every onset exact", () => {
  const words = [w(1000, 1180, "one "), w(1200, 1400, "more "), w(1420, 2600, "kiss, "), w(3000, 3300, "wine")];
  const widths = [40, 50, 45, 42];
  const c = lineCursor(words, widths);
  it("sits at each word's left edge when the word is sung", () => {
    expect(c.offsets).toEqual([0, 40, 90, 135]);
    for (const [i, word] of words.entries()) expect(c.at(word.t)).toBeCloseTo(c.offsets[i], 6);
  });
  it("is dark before the first onset and full after the last word ends", () => {
    expect(c.at(999)).toBe(-Infinity);
    expect(c.at(5000)).toBe(177);
    expect(c.end).toBeGreaterThanOrEqual(3300);
  });
  it("never runs backward and never jumps between frames", () => {
    let prev = c.at(1000);
    for (let t = 1000; t <= 3400; t += 1000 / 60) {
      const x = c.at(t);
      expect(x).toBeGreaterThanOrEqual(prev - 1e-9);
      // ~33px/frame would be the old 90ms flash across a 50px word.
      expect(x - prev).toBeLessThan(8);
      prev = x;
    }
  });
  it("finishes a word before a silence where its sound ends, then waits", () => {
    expect(c.at(2600)).toBeCloseTo(135, 6);
    expect(c.at(2800)).toBeCloseTo(135, 6);
  });
});

describe("short words and syllables", () => {
  it("crosses a 20ms word at the letter pace instead of snapping", () => {
    const c = lineCursor([w(1000, 1020, "I "), w(2000, 2400, "know")], [12, 40]);
    expect(c.at(1010)).toBeLessThan(6);
    expect(c.at(1000 + MIN_LETTER_MS)).toBeCloseTo(12, 6);
  });
  it("runs straight through a phrase of syllables without stopping at each", () => {
    const syl = [0, 1, 2, 3, 4].map(i => w(1000 + i * 110, 1000 + i * 110 + 80, "가"));
    const c = lineCursor(syl, [17, 17, 17, 17, 17]);
    // In the 30ms gaps between syllables the cursor keeps moving.
    for (const s of syl.slice(0, -1)) expect(c.at(s.end + 15)).toBeGreaterThan(c.at(s.end) + 0.5);
  });
});

describe("held notes follow the voice's letters", () => {
  it("slows on the held letter instead of crossing the word evenly", () => {
    // "kiss,": k-i quick, the i held for ~1.3s, then s-s.
    const kiss = w(1000, 2700, "kiss,", { points: [
      { t: 1000, fraction: 0 }, { t: 1020, fraction: 0.2 }, { t: 1240, fraction: 0.2 },
      { t: 1260, fraction: 0.4 }, { t: 2600, fraction: 0.4 }, { t: 2620, fraction: 0.8 },
      { t: 2680, fraction: 0.8 }, { t: 2700, fraction: 1 },
    ] });
    const c = lineCursor([kiss, w(3500, 3700, "wine")], [50, 40]);
    // Letters fill until the next letter starts (lrc.ts sampleFill).
    expect(c.at(1240)).toBeCloseTo(10, 6);
    expect(c.at(2600)).toBeCloseTo(20, 6);
    // The last 60% isn't a 20ms snap: it gets the letter pace.
    expect(c.at(2620)).toBeLessThan(25);
    expect(c.at(2600 + 3 * MIN_LETTER_MS)).toBeCloseTo(50, 6);
  });
});

describe("other voices keep their own fill", () => {
  it("leaves phrase-timed and out-of-order words out of the cursor", () => {
    const words = [w(2000, 5000, "lead "), w(1000, 3000, "(oh)"), w(1000, 4000, "(ah)", { timing: "phrase" })];
    const c = lineCursor(words, [30, 30, 30]);
    expect(c.owns).toEqual([true, false, false]);
    expect(c.at(1500)).toBe(-Infinity);
    expect(c.at(6000)).toBe(30);
  });
});
