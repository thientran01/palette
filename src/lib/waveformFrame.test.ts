import { describe, expect, it } from "vitest";
import { BANDS_STALE_MS, bandsStale } from "./waveformFrame";

describe("bandsStale", () => {
  it("keeps a live ~30Hz stream fresh, even with a few late packets", () => {
    expect(bandsStale(1033, 1000)).toBe(false);
    expect(bandsStale(1000 + 4 * 33, 1000)).toBe(false);
  });

  it("keeps a payload that landed after the frame started fresh", () => {
    // rAF's timestamp is the frame start, so it can precede the payload.
    expect(bandsStale(1000, 1004)).toBe(false);
  });

  it("reads a stream that stopped as silence", () => {
    expect(bandsStale(1000 + BANDS_STALE_MS + 1, 1000)).toBe(true);
  });
});
