import { describe, expect, it } from "vitest";
import { FALLBACK_LINE_MS, lineSpan, panOffset } from "./cardLinePan";

describe("lineSpan", () => {
  it("uses the sung words when the line has them", () => {
    const line = { t: 1000, text: "a b", words: [{ t: 1200, text: "a ", end: 1500 }, { t: 1600, text: "b", end: 2400 }] };
    expect(lineSpan(line, { t: 5000, text: "next" })).toEqual({ start: 1200, end: 2400 });
  });
  it("falls back to the next line's stamp, then a fixed length", () => {
    expect(lineSpan({ t: 1000, text: "a" }, { t: 3000, text: "b" })).toEqual({ start: 1000, end: 3000 });
    expect(lineSpan({ t: 1000, text: "a" }, undefined)).toEqual({ start: 1000, end: 1000 + FALLBACK_LINE_MS });
  });
});

describe("panOffset", () => {
  const span = { start: 0, end: 1000 };
  it("never moves a line that fits", () => {
    expect(panOffset(0, span, 500)).toBe(0);
    expect(panOffset(-20, span, 500)).toBe(0);
  });
  it("holds the start, slides, then rests on the end", () => {
    expect(panOffset(100, span, -50)).toBe(0);
    expect(panOffset(100, span, 200)).toBe(0);
    expect(panOffset(100, span, 500)).toBe(50);
    expect(panOffset(100, span, 800)).toBe(100);
    expect(panOffset(100, span, 2000)).toBe(100);
  });
});
