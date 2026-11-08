import { describe, expect, it } from "vitest";
import { REST, UNVOICED, voicePose } from "./voiceMotion";

/** A held note: 1.4s, steady pitch for 300ms, then ±70-cent vibrato at
 * 5.5Hz, loudness 80. */
function held(t = 1000) {
  const n = 140;
  const c = Array.from({ length: n }, (_, k) => k < 30 ? -600 : Math.round(-600 + 70 * Math.sin(2 * Math.PI * 5.5 * (k - 30) / 100)));
  return { t, end: t + n * 10, text: "kiss, ", voice: { c, l: new Array(n).fill(80) } };
}

describe("every word lifts while sounding, then settles", () => {
  const w = { t: 1000, end: 1100, text: "가" };
  it("rests before, rises at the onset and settles after its release", () => {
    expect(voicePose(w, 1110, 900)).toEqual(REST);
    expect(voicePose(w, 1110, 1100).y).toBeCloseTo(1, 1);
    expect(voicePose(w, 1110, 1110 + 130).y).toBeLessThan(0.6);
    expect(voicePose(w, 1110, 1110 + 260)).toEqual(REST);
  });
  it("never follows pitch or swells on a short syllable", () => {
    const c = [-600, 0, -1200, 0, -1200, 0, -1200, 0, -1200, 0];
    const p = voicePose({ ...w, voice: { c, l: new Array(10).fill(100) } }, 1110, 1050);
    expect(p.s).toBe(1);
    expect(p.y).toBeLessThanOrEqual(1);
  });
});

describe("a held note follows the singer", () => {
  it("sways with vibrato around where the note started", () => {
    const w = held();
    const ys = Array.from({ length: 60 }, (_, k) => voicePose(w, 3000, 1500 + k * 10).y);
    const span = Math.max(...ys) - Math.min(...ys);
    expect(span).toBeGreaterThan(1.5);
    expect(span).toBeLessThan(3.5);
    // Steady part: no sway, just the loudness lift.
    const steady = [1150, 1200, 1250].map(t => voicePose(w, 3000, t).y);
    expect(Math.max(...steady) - Math.min(...steady)).toBeLessThan(0.05);
  });
  it("swells slightly across the hold and returns to rest after", () => {
    const w = held();
    expect(voicePose(w, 3000, 1100).s).toBeLessThan(voicePose(w, 3000, 2300).s);
    expect(voicePose(w, 3000, 2300).s).toBeLessThan(1.04);
    expect(voicePose(w, 3000, 2400 + 120 + 260)).toEqual(REST);
  });
  it("holds the last pitch through unvoiced frames instead of dropping", () => {
    const w = held();
    const c = w.voice.c.map((v, k) => k >= 100 && k < 110 ? UNVOICED : v);
    const a = voicePose({ ...w, voice: { ...w.voice, c } }, 3000, 1995).y;
    const b = voicePose({ ...w, voice: { ...w.voice, c } }, 3000, 2050).y;
    expect(Math.abs(a - b)).toBeLessThan(0.5);
  });
});
