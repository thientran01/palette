/*
 * Break-synthesis tests — rest dots must mean the singer has actually
 * stopped. LRC only has line STARTS; the old holdEstimate (clamped to 5s)
 * invented a mid-phrase break whenever the next stamp was ≥12s away.
 * currentLineIndex then highlighted the 5-dot row while the vocal held.
 *
 * The suites lock the three contracts: a 13s+ unmarked gap keeps the
 * lyric (no break at t+5s), an empty-timestamp marker still pins a
 * break, and short gaps stay as they were (previous line current).
 */
import { describe, expect, it } from "vitest";
import { attachWords, currentLineIndex, parseLrc, wordWipe, type LyricLine } from "./lrc";

/** Lines that render as the five-dot rest row. */
function breaks(lines: LyricLine[]): LyricLine[] {
  return lines.filter((l) => l.end !== undefined);
}

describe("unmarked 13s+ gap keeps the lyric", () => {
  // Cadence that would have clamped holdEstimate to HOLD_MAX_MS (5s):
  // several ~6s lines, then a 13s gap. Old withBreaks: start = line.t+5s,
  // remaining 8s ≥ BREAK_MIN → rest dots mid-hold.
  const lines = parseLrc(
    [
      "[00:00.00]one",
      "[00:06.00]two",
      "[00:12.00]long hold",
      "[00:25.00]next",
    ].join("\n"),
    60_000,
  );

  it("does not invent a break at t+5s of the hold", () => {
    expect(lines.map((l) => l.text)).toEqual(["one", "two", "long hold", "next"]);
    expect(breaks(lines)).toEqual([]);
    expect(lines.some((l) => l.text === "" && l.t === 17_000)).toBe(false);
  });

  it("keeps the hold line current through the whole 13s gap", () => {
    const hold = lines.findIndex((l) => l.text === "long hold");
    expect(hold).toBeGreaterThanOrEqual(0);
    // Just after the old invented break (t+5s), mid-hold, and a beat
    // before the next stamp — none of these may land on rest dots.
    for (const pos of [17_000, 20_000, 24_999]) {
      expect(currentLineIndex(lines, pos, 0)).toBe(hold);
      expect(lines[currentLineIndex(lines, pos, 0)].end).toBeUndefined();
    }
  });
});

describe("empty-timestamp marker still creates a break", () => {
  it("pins a mid-track break at the marker, not at a guessed hold", () => {
    const lines = parseLrc(
      [
        "[00:00.00]verse",
        "[00:04.00]last vocal",
        "[00:08.00] ",
        "[00:20.00]next verse",
      ].join("\n"),
      60_000,
    );
    expect(breaks(lines)).toEqual([{ t: 8_000, text: "", end: 20_000 }]);
    const idx = currentLineIndex(lines, 12_000, 0);
    expect(lines[idx].end).toBe(20_000);
    expect(lines[idx].t).toBe(8_000);
    // Before the marker the lyric stays current — the pin is the vocal end.
    const vocal = lines.findIndex((l) => l.text === "last vocal");
    expect(currentLineIndex(lines, 7_999, 0)).toBe(vocal);
  });

  it("pins a marked outro to track duration", () => {
    // First line at 0 so this is not also an intro.
    const lines = parseLrc(
      ["[00:00.00]verse", "[00:10.00]last", "[00:14.00] "].join("\n"),
      30_000,
    );
    expect(breaks(lines)).toEqual([{ t: 14_000, text: "", end: 30_000 }]);
    expect(currentLineIndex(lines, 20_000, 0)).toBe(
      lines.findIndex((l) => l.end === 30_000),
    );
  });
});

describe("short gaps unchanged", () => {
  it("leaves a 4s cadence as lyrics only — previous line stays current", () => {
    const lines = parseLrc(
      ["[00:00.00]a", "[00:03.00]b", "[00:06.00]c", "[00:10.00]d"].join("\n"),
      30_000,
    );
    expect(lines.map((l) => l.text)).toEqual(["a", "b", "c", "d"]);
    expect(breaks(lines)).toEqual([]);
    expect(currentLineIndex(lines, 5_000, 0)).toBe(
      lines.findIndex((l) => l.text === "b"),
    );
  });

  it("does not promote a short marked gap into a break row", () => {
    // Marker 4s before the next line — under BREAK_MIN_MS, same as before.
    const lines = parseLrc(
      ["[00:00.00]a", "[00:04.00] ", "[00:08.00]b"].join("\n"),
      30_000,
    );
    expect(breaks(lines)).toEqual([]);
    expect(currentLineIndex(lines, 6_000, 0)).toBe(
      lines.findIndex((l) => l.text === "a"),
    );
  });
});

describe("intro and unmarked outro", () => {
  it("still synthesizes an intro before the first line", () => {
    const lines = parseLrc("[00:12.00]first", 60_000);
    expect(breaks(lines)[0]).toEqual({ t: 0, text: "", end: 12_000 });
    expect(currentLineIndex(lines, 3_000, 0)).toBe(0);
    expect(lines[0].end).toBe(12_000);
  });

  it("does not invent an unmarked outro from a guessed hold", () => {
    // Last line at 10s, track ends at 30s — old hold fallback (3s) would
    // have opened rest dots at 13s. Keep the lyric; no pin, no break.
    // First line at 0 so this is not also an intro.
    const lines = parseLrc(["[00:00.00]verse", "[00:10.00]last"].join("\n"), 30_000);
    expect(breaks(lines)).toEqual([]);
    const last = lines.findIndex((l) => l.text === "last");
    expect(currentLineIndex(lines, 20_000, 0)).toBe(last);
    expect(lines[last].end).toBeUndefined();
  });
});

describe("attachWords", () => {
  it("does not invent a fill when the store is empty", () => {
    const lines = parseLrc("[00:01.00]hello world\n[00:05.00]next", 30_000);
    expect(attachWords(lines, [])).toBe(lines);
    expect(attachWords(lines, []).every((l) => l.words === undefined)).toBe(true);
  });

  it("pins words onto the line whose window contains them", () => {
    const lines = parseLrc("[00:01.00]hello world\n[00:05.00]next", 30_000);
    const got = attachWords(lines, [
      { t: 1000, text: "hello ", end: 1200 },
      { t: 1300, text: "world", end: 1600 },
      { t: 5000, text: "next", end: 5400 },
    ]);
    expect(got[0].words?.map((w) => w.text)).toEqual(["hello ", "world"]);
    expect(got[1].words?.map((w) => w.text)).toEqual(["next"]);
  });

  it("never attaches words onto a break row", () => {
    const lines = parseLrc("[00:00.00]verse\n[00:04.00] \n[00:20.00]next", 60_000);
    const got = attachWords(lines, [{ t: 8000, text: "nope", end: 8200 }]);
    expect(breaks(got)[0].words).toBeUndefined();
  });
});

describe("wordWipe", () => {
  const words = [
    { t: 1000, text: "one ", end: 1300 },
    { t: 1300, text: "two", end: 1600 },
  ];

  it("is null before the first word", () => {
    expect(wordWipe(words, 900, 0)).toBeNull();
  });

  it("hits on the onset then holds, instead of crawling the whole word", () => {
    expect(wordWipe(words, 1000, 0)).toEqual({ index: 0, frac: 0 });
    expect(wordWipe(words, 1090, 0)).toEqual({ index: 0, frac: 1 });
    expect(wordWipe(words, 1200, 0)).toEqual({ index: 0, frac: 1 });
    expect(wordWipe(words, 1450, 0)?.index).toBe(1);
  });

  it("does not invent an end for a last word without one", () => {
    expect(wordWipe([{ t: 1000, text: "one" }], 1100, 0)).toEqual({ index: 0, frac: 1 });
  });
});
