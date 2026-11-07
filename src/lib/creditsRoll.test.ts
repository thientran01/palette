import { describe, expect, it } from "vitest";
import type { LyricLine } from "./lrc";
import { ROLL_DWELL_MS, planRoll, rollOffsetAt, singingEndMs } from "./creditsRoll";

describe("singingEndMs", () => {
  it("uses the trailing outro break's start", () => {
    const lines: LyricLine[] = [
      { t: 1000, text: "a" },
      { t: 5000, text: "b" },
      { t: 9000, text: "", end: 20000 },
    ];
    expect(singingEndMs(lines)).toBe(9000);
  });

  it("falls back to the last word, or a few seconds into the last line", () => {
    const words = [{ t: 5000, text: "b", end: 11000, line_t: 5000 }];
    expect(singingEndMs([{ t: 5000, text: "b", words }])).toBe(11000);
    expect(singingEndMs([{ t: 5000, text: "b" }])).toBe(9000);
    expect(singingEndMs([{ t: 0, text: "", end: 4000 }])).toBeNull();
  });
});

describe("rollOffsetAt", () => {
  it("parks, then rolls to finish before the track ends, then stops", () => {
    // 600px of credits, 20s of outro after the dwell and tail.
    const plan = planRoll(10_000, 10_000 + ROLL_DWELL_MS + 20_000 + 2000, 100, 700);
    expect(plan.speed).toBeCloseTo(0.03);
    expect(rollOffsetAt(plan, 10_500)).toBe(100);
    expect(rollOffsetAt(plan, plan.start + 10_000)).toBeCloseTo(400);
    expect(rollOffsetAt(plan, plan.start + 60_000)).toBe(700);
  });

  it("keeps a readable pace on short and long outros", () => {
    const short = planRoll(0, 6000, 0, 2000);
    expect(short.speed).toBeCloseTo(0.04);
    const long = planRoll(0, 600_000, 0, 100);
    expect(long.speed).toBeCloseTo(0.014);
  });
});
