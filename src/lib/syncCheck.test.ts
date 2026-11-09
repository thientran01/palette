import { describe, expect, it } from "vitest";
import type { SavedSync, SyncCheck } from "./backend";
import { checkSummary, onTimePercent, savedWhen, sortSyncs } from "./syncCheck";

const check = (on_time: number, extra: Partial<SyncCheck> = {}): SyncCheck =>
  ({ on_time, lines: "sss", unsure: 0, reheard: 0, fixed: 0, ...extra });
const row = (key: string, saved_at: number, c: SyncCheck | null): SavedSync =>
  ({ key, title: key, artist: "", thumb_key: null, saved_at, refresh: false, relearns: !c, check: c });

describe("sync check", () => {
  it("rounds the estimate and never claims 100%", () => {
    expect(onTimePercent(check(0.942))).toBe(94);
    expect(onTimePercent(check(0.993))).toBe(99);
  });

  it("tells what the second listen did", () => {
    expect(checkSummary(check(0.95, { reheard: 7, fixed: 5, unsure: 2 })))
      .toBe("A second listen on isolated vocals fixed 5 of 7 lines it doubted. 2 may still be off.");
    expect(checkSummary(check(0.97, { reheard: 3, fixed: 3 })))
      .toBe("A second listen on isolated vocals fixed 3 of 3 lines it doubted.");
    expect(checkSummary(check(0.9, { unsure: 1 }))).toBe("1 line may be off.");
    expect(checkSummary(check(0.99))).toBe("Every line passed the check.");
  });

  it("says when a sync was learned in days, then as a date", () => {
    const now = new Date(2026, 10, 9, 15).getTime();
    const at = (d: number, h = 12) => new Date(2026, 10, d, h).getTime() / 1000;
    expect(savedWhen(at(9, 1), now)).toBe("today");
    expect(savedWhen(at(8, 23), now)).toBe("yesterday");
    expect(savedWhen(at(6), now)).toBe("3 days ago");
    expect(savedWhen(at(1), now)).not.toMatch(/ago/);
  });

  it("sorts the least sure first and unchecked songs last", () => {
    const rows = [row("new", 30, null), row("good", 20, check(0.97)), row("old", 10, null), row("shaky", 5, check(0.86))];
    expect(sortSyncs(rows, "recent")).toBe(rows);
    expect(sortSyncs(rows, "least").map(r => r.key)).toEqual(["shaky", "good", "new", "old"]);
  });
});
