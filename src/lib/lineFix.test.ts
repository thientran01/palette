import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const backend = vi.hoisted(() => ({ fix: vi.fn(), unfix: vi.fn(), seek: vi.fn() }));
vi.mock("./backend", () => ({
  commands: { karaokeFixLine: backend.fix, karaokeUnfixLine: backend.unfix, seekAbs: backend.seek },
}));

import {
  PRE_ROLL_MS,
  cancelRetime,
  getRetime,
  lineWords,
  nothingHeard,
  plainLine,
  pressDown,
  pressUp,
  settle,
  startRetime,
  tappedWords,
  undoFix,
} from "./lineFix";
import type { LyricLine } from "./lrc";

const song = { artist: "A", title: "T", album: "B", duration_ms: 200_000 };
const line: LyricLine = { t: 10_000, text: "hold me close" };

beforeEach(() => {
  vi.useFakeTimers();
  backend.fix.mockReset().mockResolvedValue(undefined);
  backend.unfix.mockReset().mockResolvedValue(undefined);
  backend.seek.mockReset();
});
afterEach(() => {
  cancelRetime();
  vi.useRealTimers();
});

describe("lineWords", () => {
  it("splits on spaces and joins back into the line", () => {
    expect(lineWords("hold me  close")).toEqual(["hold ", "me  ", "close"]);
    expect(lineWords("너를 사랑해 (oh)").join("")).toBe("너를 사랑해 (oh)");
    expect(lineWords("")).toEqual([]);
  });
});

describe("tappedWords", () => {
  it("gives each press to a word, press to release", () => {
    expect(tappedWords("hold me close", [
      { down: 1000, up: 1300 }, { down: 1400, up: 1500 }, { down: 1600, up: 2600 },
    ])).toEqual([
      { t: 1000, end: 1300, text: "hold " },
      { t: 1400, end: 1500, text: "me " },
      { t: 1600, end: 2600, text: "close" },
    ]);
  });

  it("spreads one long press over the line by letters", () => {
    const words = tappedWords("hold me close", [{ down: 1000, up: 2100 }])!;
    // 4 + 2 + 5 letters over 1100ms: 100ms a letter.
    expect(words.map((w) => [w.t, w.end])).toEqual([[1000, 1400], [1400, 1600], [1600, 2100]]);
  });

  it("refuses presses that don't fit the line", () => {
    expect(tappedWords("hold me close", [{ down: 1, up: 2 }, { down: 3, up: 4 }])).toBeNull();
    expect(tappedWords("hold", [])).toBeNull();
    expect(tappedWords("hold", [{ down: 5, up: 4 }])).toBeNull();
  });

  it("doesn't let punctuation take time", () => {
    const words = tappedWords("oh, yeah!", [{ down: 0, up: 500 }])!;
    // "oh" and "yeah": 2 and 4 letters.
    expect(words.map((w) => w.end)).toEqual([167, 500]);
  });
});

describe("tapping a line in", () => {
  it("starts before the line and saves once every word is tapped", async () => {
    startRetime(song, line, true);
    expect(backend.seek).toHaveBeenCalledWith(line.t - PRE_ROLL_MS);
    expect(getRetime().note).toBe("ready");
    for (const [d, u] of [[10_100, 10_300], [10_400, 10_500], [10_600, 11_400]]) {
      pressDown(d);
      pressUp(u);
    }
    await Promise.resolve();
    expect(backend.fix).toHaveBeenCalledWith(song, line.t, line.text, [
      { t: 10_100, end: 10_300, text: "hold " },
      { t: 10_400, end: 10_500, text: "me " },
      { t: 10_600, end: 11_400, text: "close" },
    ]);
    expect(getRetime()).toEqual({ retime: null, note: "saved" });
    vi.advanceTimersByTime(2_000);
    expect(getRetime().note).toBeNull();
  });

  it("takes one long press as the whole line", async () => {
    startRetime(song, line, true);
    pressDown(10_000);
    pressUp(11_100);
    settle();
    await Promise.resolve();
    expect(backend.fix.mock.calls[0][3]).toHaveLength(3);
  });

  it("replays the line when a word was missed", () => {
    startRetime(song, line, true);
    pressDown(10_000);
    pressUp(10_200);
    pressDown(10_300);
    pressUp(10_400);
    settle();
    expect(backend.fix).not.toHaveBeenCalled();
    expect(getRetime().note).toBe("missed");
    expect(getRetime().retime?.presses).toEqual([]);
    expect(backend.seek).toHaveBeenCalledTimes(2);
  });

  it("can't replay where the player can't seek", () => {
    startRetime(song, line, false);
    expect(backend.seek).not.toHaveBeenCalled();
    pressDown(10_000);
    pressUp(10_200);
    pressDown(10_300);
    pressUp(10_400);
    settle();
    expect(getRetime()).toEqual({ retime: null, note: "missed" });
  });

  it("gives up when nothing was tapped, and Esc stops it", () => {
    startRetime(song, line, true);
    nothingHeard();
    expect(getRetime()).toEqual({ retime: null, note: "silent" });
    startRetime(song, line, true);
    cancelRetime();
    expect(getRetime().retime).toBeNull();
  });

  it("ignores a held key's repeats and a release with no press", () => {
    startRetime(song, line, true);
    pressUp(9_000);
    pressDown(10_000);
    pressDown(10_050);
    pressUp(10_200);
    expect(getRetime().retime?.presses).toEqual([{ down: 10_000, up: 10_200 }]);
  });
});

describe("plain and undo", () => {
  it("sends the line with no words, or takes the fix back", async () => {
    await plainLine(song, line);
    expect(backend.fix).toHaveBeenCalledWith(song, line.t, line.text, []);
    await undoFix(song, line);
    expect(backend.unfix).toHaveBeenCalledWith(song, line.t, line.text);
  });

  it("says so when saving fails", async () => {
    backend.fix.mockRejectedValueOnce(new Error("disk"));
    await plainLine(song, line);
    expect(getRetime().note).toBe("failed");
  });
});
