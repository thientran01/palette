import { describe, expect, it } from "vitest";
import { heldBend, UNVOICED } from "./voiceMotion";

/** A held note: 1.4s, steady pitch for 300ms, then ±70-cent vibrato at
 * 5.5Hz. */
function held(t = 1000) {
  const n = 140;
  const c = Array.from({ length: n }, (_, k) => k < 30 ? -600 : Math.round(-600 + 70 * Math.sin(2 * Math.PI * 5.5 * (k - 30) / 100)));
  return { t, end: t + n * 10, text: "kiss, ", voice: { c, l: new Array(n).fill(80) } };
}

describe("held notes ride the pitch", () => {
  it("sways with vibrato around where the note started", () => {
    const w = held();
    const ys = Array.from({ length: 60 }, (_, k) => heldBend(w, 3000, 1500 + k * 10));
    const span = Math.max(...ys) - Math.min(...ys);
    expect(span).toBeGreaterThan(1.5);
    expect(span).toBeLessThan(3.5);
    expect([1100, 1200, 1250].every(t => heldBend(w, 3000, t) === 0)).toBe(true);
  });
  it("runs later letters a little behind, as a wave", () => {
    const w = held();
    expect(heldBend(w, 3000, 1700, 0)).not.toBeCloseTo(heldBend(w, 3000, 1700, 3), 2);
  });
  it("settles after the note ends and ignores short syllables", () => {
    const w = held();
    expect(heldBend(w, 3000, 2400 + 120 + 260)).toBe(0);
    expect(heldBend({ t: 1000, end: 1200, text: "가", voice: { c: [0, 600, 0], l: [90, 90, 90] } }, undefined, 1100)).toBe(0);
    expect(heldBend({ t: 1000, end: 2400, text: "oh" }, undefined, 1800)).toBe(0);
  });
  it("holds the last pitch through unvoiced frames instead of dropping", () => {
    const w = held();
    const c = w.voice.c.map((v, k) => k >= 100 && k < 110 ? UNVOICED : v);
    const a = heldBend({ ...w, voice: { ...w.voice, c } }, 3000, 1995);
    const b = heldBend({ ...w, voice: { ...w.voice, c } }, 3000, 2050);
    expect(Math.abs(a - b)).toBeLessThan(0.5);
  });
});
