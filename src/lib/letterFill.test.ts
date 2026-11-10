import { describe, expect, it } from "vitest";
import { ATTACK, ATTACK_MS, FILL_LEAD_MS, letterFill, letterSegments, rowLetterSpans } from "./letterFill";

const w = (t: number, end: number, text: string, extra = {}) => ({ t, end, text, ...extra });

describe("letters", () => {
  it("splits graphemes and leaves spaces as text", () => {
    expect(letterSegments("kiss, ").map(s => s.text)).toEqual(["k", "i", "s", "s", ",", " "]);
    expect(letterSegments("한국 ").filter(s => s.letter).map(s => s.text)).toEqual(["한", "국"]);
    expect(letterSegments("é").length).toBe(1);
  });
});

describe("letter spans follow one smooth cursor per row", () => {
  it("reaches every word exactly on its onset, in order, end to end", () => {
    const words = [w(1000, 1180, "one "), w(1200, 1400, "more "), w(1420, 2600, "kiss, "), w(3000, 3300, "wine")];
    const spans = rowLetterSpans(words);
    words.forEach((word, i) => expect(spans[i][0].on).toBe(word.t));
    for (const word of spans) for (let k = 1; k < word.length; k++) expect(word[k].on).toBe(word[k - 1].off);
    for (let i = 1; i < spans.length; i++) expect(spans[i - 1][spans[i - 1].length - 1].off).toBeLessThanOrEqual(words[i].t + 1);
  });
  it("ends a word before a rest where its sound ends", () => {
    const [word] = rowLetterSpans([w(1000, 1400, "word"), w(3000, 3300, "next")]);
    expect(word[3].off).toBeCloseTo(1400, -1);
  });
  it("gives a held letter the whole hold", () => {
    const [[o]] = rowLetterSpans([w(1000, 3000, "오")]);
    expect(o.off - o.on).toBeCloseTo(2000, -1);
  });
  it("gives a backing reply its own timing", () => {
    const [, back] = rowLetterSpans([w(2000, 5000, "lead "), w(1000, 1400, "(oh)", { timing: "phrase" })]);
    expect(back[0].on).toBe(1000);
    expect(back[back.length - 1].off).toBeLessThanOrEqual(1400);
  });
});

describe("a letter fills with an attack and a drag", () => {
  const span = { on: 1000, off: 2000 };
  it("starts just before it is sung, without a jump", () => {
    expect(letterFill(span, 1000 - FILL_LEAD_MS)).toBe(0);
    expect(letterFill(span, 1000 - FILL_LEAD_MS + 10)).toBeLessThan(0.02);
  });
  it("is half up right after it is sung and full only as the singing leaves", () => {
    expect(letterFill(span, 1000 + ATTACK_MS)).toBeCloseTo(ATTACK, 1);
    expect(letterFill(span, 1000 + ATTACK_MS)).toBeGreaterThanOrEqual(ATTACK);
    expect(letterFill(span, 1500)).toBeCloseTo(ATTACK + (1 - ATTACK) / 2, 6);
    expect(letterFill(span, 1990)).toBeLessThan(1);
    expect(letterFill(span, 2000)).toBe(1);
  });
  it("is simply lit on a quick letter", () => {
    expect(letterFill({ on: 1000, off: 1040 }, 1000 + ATTACK_MS)).toBe(1);
  });
});
