import { describe, expect, it } from "vitest";
import { MARQUEE, marqueeCycle, marqueeTravel } from "./marquee";

describe("marqueeTravel", () => {
  it("stays still when the text fits, or overflows by subpixel rounding", () => {
    expect(marqueeTravel(100, 120, 6)).toBe(0);
    expect(marqueeTravel(120, 120, 6)).toBe(0);
    expect(marqueeTravel(120.8, 120, 6)).toBe(0);
  });

  it("travels the overflow plus one fade, so the end lands clear of the right fade", () => {
    expect(marqueeTravel(200, 120, 6)).toBe(86);
  });

  it("keeps the ellipsis in a column too narrow to read through", () => {
    expect(marqueeTravel(200, 0, 6)).toBe(0);
    expect(marqueeTravel(200, 20, 6)).toBe(0);
  });
});

describe("marqueeCycle", () => {
  it("rests at start, pans, rests at end, pans back", () => {
    const c = marqueeCycle(80, 12);
    const t = c.keyframes.map((k) => k.transform);
    expect(t).toEqual([
      "translateX(0px)",
      "translateX(0px)",
      "translateX(-80px)",
      "translateX(-80px)",
      "translateX(0px)",
    ]);
    expect(c.totalMs).toBe(MARQUEE.restStartMs + MARQUEE.restEndMs + 2 * c.panMs);
    expect(c.keyframes[1].offset).toBeCloseTo(MARQUEE.restStartMs / c.totalMs);
    const offsets = c.keyframes.map((k) => k.offset as number);
    expect([...offsets].sort((a, b) => a - b)).toEqual(offsets);
  });

  it("pans at the same pace in characters at any font size", () => {
    expect(marqueeCycle(400, 40).panMs).toBe(marqueeCycle(120, 12).panMs);
  });

  it("never pans faster than the floor, however small the overflow", () => {
    expect(marqueeCycle(3, 12).panMs).toBe(MARQUEE.minPanMs);
  });
});
