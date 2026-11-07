import { describe, expect, it } from "vitest";
import { resampleRGBA } from "./crispArt";

function solid(w: number, h: number, rgba: [number, number, number, number]): Uint8ClampedArray {
  const a = new Uint8ClampedArray(w * h * 4);
  for (let i = 0; i < w * h; i++) a.set(rgba, i * 4);
  return a;
}

describe("resampleRGBA", () => {
  it("keeps a flat color flat (weights normalize)", () => {
    const out = resampleRGBA(solid(300, 300, [40, 120, 200, 255]), 300, 300, 52, 52);
    expect(out.length).toBe(52 * 52 * 4);
    for (let i = 0; i < out.length; i += 4) {
      expect([out[i], out[i + 1], out[i + 2], out[i + 3]]).toEqual([40, 120, 200, 255]);
    }
  });

  it("keeps a hard edge in place and sharp", () => {
    // Left half black, right half white, 300 → 50 (6x shrink).
    const w = 300;
    const src = new Uint8ClampedArray(w * 4 * 4);
    for (let y = 0; y < 4; y++)
      for (let x = 0; x < w; x++) src.set(x < 150 ? [0, 0, 0, 255] : [255, 255, 255, 255], (y * w + x) * 4);
    const out = resampleRGBA(src, w, 4, 50, 4);
    const row = Array.from({ length: 50 }, (_, x) => out[x * 4]);
    // Far from the edge: exact. At the edge: a transition of a couple of pixels.
    expect(row[10]).toBe(0);
    expect(row[40]).toBe(255);
    expect(row[23]).toBeLessThan(20);
    expect(row[26]).toBeGreaterThan(235);
  });

  it("doesn't bleed dark fringes from transparent pixels", () => {
    const w = 120;
    const src = new Uint8ClampedArray(w * w * 4);
    for (let i = 0; i < w * w; i++) src.set(i % w < 60 ? [0, 0, 0, 0] : [255, 0, 0, 255], i * 4);
    const out = resampleRGBA(src, w, w, 20, 20);
    for (let x = 0; x < 20; x++) {
      const p = (10 * 20 + x) * 4;
      if (out[p + 3] > 8) expect(out[p]).toBeGreaterThan(240);
    }
  });
});
