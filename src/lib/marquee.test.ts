import { describe, expect, it } from "vitest";
import { MARQUEE, marqueeClock, marqueeKeyframes, marqueeTravel } from "./marquee";

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

const at = (frames: Keyframe[], i: number) => frames[i].offset as number;

describe("marqueeClock", () => {
  it("is null when nothing pans", () => {
    expect(marqueeClock([{ travelPx: 0, fontPx: 15 }])).toBeNull();
  });

  it("pans at the same pace in characters at any font size", () => {
    const big = marqueeClock([{ travelPx: 400, fontPx: 40 }])!;
    const small = marqueeClock([{ travelPx: 120, fontPx: 12 }])!;
    expect(big.panMs).toBe(small.panMs);
  });

  it("never pans faster than the floor, however small the overflow", () => {
    expect(marqueeClock([{ travelPx: 3, fontPx: 12 }])!.panMs).toBe(MARQUEE.minPanMs);
  });

  it("takes the group's speed from its largest panning font", () => {
    const c = marqueeClock([
      { travelPx: 60, fontPx: 15 },
      { travelPx: 100, fontPx: 12 },
      { travelPx: 0, fontPx: 40 },
    ])!;
    expect(c.pxPerMs).toBeCloseTo((MARQUEE.speedEmPerS * 15) / 1000);
    expect(c.panMs).toBe(Math.round(100 / c.pxPerMs));
  });
});

describe("marqueeKeyframes", () => {
  it("rests at start, pans, rests at end, pans back", () => {
    const clock = marqueeClock([{ travelPx: 80, fontPx: 12 }])!;
    const f = marqueeKeyframes(80, clock);
    expect(f.map((k) => k.transform)).toEqual([
      "translateX(0px)",
      "translateX(0px)",
      "translateX(-80px)",
      "translateX(-80px)",
      "translateX(0px)",
    ]);
    expect(at(f, 1)).toBeCloseTo(MARQUEE.restStartMs / clock.totalMs);
    const offsets = f.map((k) => k.offset as number);
    expect([...offsets].sort((a, b) => a - b)).toEqual(offsets);
  });

  it("is linear: no per-keyframe easing", () => {
    const clock = marqueeClock([{ travelPx: 80, fontPx: 12 }])!;
    expect(marqueeKeyframes(80, clock).every((k) => k.easing === undefined)).toBe(true);
  });

  it("moves a title and its artist together: same start, same speed, home together", () => {
    const lines = [
      { travelPx: 150, fontPx: 15 },
      { travelPx: 50, fontPx: 12 },
    ];
    const clock = marqueeClock(lines)!;
    const long = marqueeKeyframes(150, clock);
    const short = marqueeKeyframes(50, clock);
    // Both leave the start rest at the same moment...
    expect(at(short, 1)).toBe(at(long, 1));
    // ...at the same speed (px per unit of offset) on the way out...
    const speed = (f: Keyframe[], d: number) => d / (at(f, 2) - at(f, 1));
    expect(speed(short, 50)).toBeCloseTo(speed(long, 150));
    // ...and on the way back, landing home together.
    expect(speed(short, 50)).toBeCloseTo(50 / (1 - at(short, 3)));
    expect(at(short, 4)).toBe(at(long, 4));
  });
});
