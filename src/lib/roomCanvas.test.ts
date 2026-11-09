import { describe, expect, it } from "vitest";
import { bezierEase, blendCapsule, ROOM_FRAME_MS, roomCenters } from "./roomCanvas";
import { EASE } from "./tokens";

describe("roomCenters", () => {
  it("lays bars out like the DOM row (justify-between)", () => {
    const c = roomCenters(19, 780, 12);
    expect(c).toHaveLength(19);
    expect(c[0]).toBe(6);
    expect(c[18]).toBeCloseTo(774);
    expect(c[9]).toBeCloseTo(390);
  });
});

describe("bezierEase", () => {
  const ease = bezierEase(EASE.out);
  it("pins the ends", () => {
    expect(ease(0)).toBe(0);
    expect(ease(1)).toBe(1);
    expect(ease(-1)).toBe(0);
    expect(ease(2)).toBe(1);
  });
  it("is monotonic and front-loaded like EASE.out", () => {
    let prev = 0;
    for (let t = 0.05; t <= 1; t += 0.05) {
      const v = ease(t);
      expect(v).toBeGreaterThanOrEqual(prev);
      prev = v;
    }
    expect(ease(0.5)).toBeGreaterThan(0.85);
  });
  it("is the identity for a linear curve", () => {
    const lin = bezierEase([0, 0, 1, 1]);
    for (const t of [0.1, 0.33, 0.8]) expect(lin(t)).toBeCloseTo(t, 4);
  });
});

describe("blendCapsule", () => {
  const dot = { cx: 3, cy: 60, w: 6, h: 6, a: 0 };
  const bar = { cx: 6, cy: 60, w: 12, h: 40, a: 1 };
  it("starts at the DOM dot and ends at the live bar", () => {
    expect(blendCapsule(dot, bar, 0, 0)).toEqual(dot);
    expect(blendCapsule(dot, bar, 1, 1)).toEqual(bar);
  });
  it("runs opacity on its own progress", () => {
    const mid = blendCapsule(dot, bar, 0.5, 1);
    expect(mid.h).toBe(23);
    expect(mid.a).toBe(1);
  });
});

describe("ROOM_FRAME_MS", () => {
  // How many display frames pass between drawn frames at a refresh rate.
  const stride = (hz: number) => Math.ceil(ROOM_FRAME_MS / (1000 / hz) - 1e-9);
  it("paces evenly at about 60fps on common refresh rates", () => {
    expect(stride(60)).toBe(1);
    expect(stride(120)).toBe(2);
    expect(stride(144)).toBe(2);
    expect(stride(165)).toBe(3);
    expect(stride(240)).toBe(4);
  });
});
