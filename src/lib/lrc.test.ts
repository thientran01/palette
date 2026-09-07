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
import { attachWords, currentLineIndex, parseLrc, wordWipe, wordWipeFraction, wordFillEnd, type LyricLine } from "./lrc";

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

  it("attaches by line stamp when words carry one, even before the stamp", () => {
    // A negative song lead puts a line's first word 60ms before its stamp;
    // by time it would glue onto the previous row ("MoveFly").
    const lines = parseLrc("[00:01.00]hello world\n[00:03.00]next up", 10_000);
    const got = attachWords(lines, [
      { t: 1000, text: "hello ", end: 1300, line_t: 1000 },
      { t: 1300, text: "world", end: 1600, line_t: 1000 },
      { t: 2940, text: "next ", end: 3200, line_t: 3000 },
      { t: 3200, text: "up", end: 3400, line_t: 3000 },
    ]);
    expect(got[0].words?.map((w) => w.text)).toEqual(["hello ", "world"]);
    expect(got[1].words?.map((w) => w.text)).toEqual(["next ", "up"]);
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

// Let Me In: MMS placed the second opening vocalization at 8.829s,
// after its explicit 8.610s end marker. These are source-timed phrases,
// not measured word onsets; ordinary/mixed lyrics must retain their data.
describe("repeated vocalization phrase fallback", () => {
  const source = "[00:02.76] Oh-oh\n[00:05.62] Oh-oh-oh-oh\n[00:08.61] \n[00:12.92] normal words";
  it("keeps source phrase boundaries even when a short break is not rendered", () => {
    const lines = attachWords(parseLrc(source, 20000), [
      { t: 2984, end: 3225, text: "Oh-oh", line_t: 2760 },
      { t: 8829, end: 9090, text: "Oh-oh-oh-oh", line_t: 5620 },
      { t: 13000, end: 14000, text: "normal words", line_t: 12920 },
    ]);
    expect(lines.map(l => l.text)).toEqual(["Oh-oh", "Oh-oh-oh-oh", "normal words"]);
    expect(lines[0].words?.[0]).toMatchObject({ t: 2760, end: 5620 });
    expect(lines[1].words?.[0]).toMatchObject({ t: 5620, end: 8610 });
    expect(lines[2].words?.[0]).toEqual({ t: 13000, end: 14000, text: "normal words", line_t: 12920 });
    expect(wordWipe(lines[1].words!, (5620 + 8610) / 2, 0)?.frac).toBeCloseTo(0.5);
    expect(attachWords(lines, [{t: 9000, end: 9100, text: "Oh-oh-oh-oh", line_t: 5620}])[1].words).toEqual(lines[1].words);
  });
  it("leaves mixed lyrics, single ohs, and missing end boundaries acoustic", () => {
    for (const text of ["oh", "oh, my love", "love (oh-oh)", "oh-oh baby", "uh-oh", "Oh-oh-ohh"]) {
      expect(parseLrc(`[00:01.00]${text}\n[00:04.00]next`, 9000)[0].words).toBeUndefined();
    }
    expect(parseLrc("[00:01.00]Oh-oh", 0)[0].words).toBeUndefined();
    expect(parseLrc("[00:01.00]Oh-oh", 180000)[0].words).toBeUndefined();
    expect(parseLrc("[00:01.00]Oh-oh\n[00:03.00]", 180000)[0].words?.[0].end).toBe(3000);
  });
});

describe("concurrent backing phrase attachment", () => {
  it("preserves source text order when timestamps overlap", () => {
    const words=[{t:1000,end:2000,text:"lead ",line_t:1000},{t:3000,end:4000,text:"later ",line_t:1000},{t:1000,end:4000,text:"(backing)",line_t:1000}];
    expect(attachWords([{t:1000,text:"lead later (backing)"}],words)[0].words).toEqual(words);
  });
  it("uses an explicit blank end marker and preserves normal timings on reattachment", () => {
    const source="[00:01.00]lead (oh, oh-oh)\n[00:04.00]\n[00:06.00]next";
    const words=[{t:1100,end:2500,text:"lead ",line_t:1000},{t:3900,end:4200,text:"(oh, oh-oh)",line_t:1000}];
    const once=attachWords(parseLrc(source,9000),words);
    expect(once[0].words?.map(w=>w.text).join("")).toBe("lead (oh, oh-oh)");
    expect(once[0].words?.[0]).toEqual(words[0]);
    expect(once[0].words?.[1]).toMatchObject({t:1000,end:4000,timing:"phrase"});
    expect(attachWords(once,words)).toEqual(once);
  });
});


describe("acoustic spelling fill", () => {
  const word = { t: 1000, end: 2000, text: "hallway ", points: [
    { t: 1000, fraction: 0 }, { t: 1200, fraction: 4 / 7 },
    { t: 1600, fraction: 4 / 7 }, { t: 2000, fraction: 1 },
  ] };
  it("unfolds each segment up to the next acoustic onset", () => {
    expect(wordWipeFraction(word, 1300, 0)).toBeCloseTo(0.2171428571);
    expect(wordWipeFraction(word, 1500, 0)).toBeCloseTo(0.4419047619);
    expect(wordWipeFraction(word, 1800, 0)).toBeCloseTo(0.7342857143);
    expect(wordWipeFraction(word, 2000, 0)).toBe(1);
  });
  it("uses the same lead and resets immediately on backward seek", () => {
    expect(wordWipeFraction(word, 1640, 160)).toBeCloseTo(0.7342857143);
    expect(wordWipeFraction(word, 940, 160)).toBeCloseTo(0.0609523810);
    expect(wordWipeFraction(word, 800, 160)).toBe(0);
  });
  it("uses measured duration for legacy English words instead of a 90ms flash", () => {
    expect(wordWipeFraction({ t: 1000, end: 2000, text: "comeback" }, 1500, 0)).toBe(.5);
    expect(wordWipeFraction({ t: 1000, end: 2000, text: "한" }, 1090, 0)).toBe(1);
  });
  it("ignores malformed detail and phrase remapping overrides acoustic detail", () => {
    expect(wordWipeFraction({ ...word, points: [...word.points].reverse() }, 1500, 0)).toBe(.5);
    expect(wordWipeFraction({ ...word, timing: "phrase" }, 1500, 0)).toBe(.5);
  });
});


describe("parentheses do not establish simultaneous vocals", () => {
  it("preserves Orchestra's measured replies instead of starting them with the lead", () => {
    const source="[00:00.08]Orchestra (Orches-orchestra)\n[00:05.96]Ah (Elegant elegant dangerous)\n[00:10.86]Hah (Slow slow)\n[00:15.07]Ad astra";
    const words=[
      {t:573,end:1894,text:"Orchestra ",line_t:80},
      {t:4578,end:5999,text:"(Orches-orchestra)",line_t:80},
      {t:5661,end:5821,text:"Ah ",line_t:5960},
      {t:5821,end:6524,text:"(Elegant ",line_t:5960},
      {t:6584,end:10216,text:"elegant ",line_t:5960},
      {t:10216,end:10818,text:"dangerous)",line_t:5960},
      {t:10881,end:11081,text:"Hah ",line_t:10860},
      {t:11101,end:11262,text:"(Slow ",line_t:10860},
      {t:11302,end:11582,text:"slow)",line_t:10860},
    ];
    const lines=attachWords(parseLrc(source,177000),words);
    expect(lines.slice(0,3).every(l=>l.backingPhrase===undefined)).toBe(true);
    expect(lines.flatMap(l=>l.words??[])).toEqual(words);
    expect(wordWipeFraction(lines[0].words![1],1000,220)).toBe(0);
    expect(wordWipeFraction(lines[0].words![1],4500,220)).toBeGreaterThan(0);
  });
});


it("computes completion from the displayed fill rather than always using word end", () => {
  expect(wordFillEnd({t:1000,end:2000,text:"한"})).toBe(1090);
  expect(wordFillEnd({t:1000,end:2000,text:"one"})).toBe(1090);
  expect(wordFillEnd({t:1000,end:1050,text:"one"})).toBe(1050);
  expect(wordFillEnd({t:1000,end:2000,text:"hallway"})).toBe(2000);
  expect(wordFillEnd({t:1000,text:"one"})).toBe(1000);
  expect(wordFillEnd({t:1000,text:"hallway"},2000)).toBe(2000);
  expect(wordFillEnd({t:1000,end:2000,text:"oh",timing:"phrase"})).toBe(2000);
  const word={t:1000,end:2000,text:"hallway",points:[
    {t:1000,fraction:0},{t:1800,fraction:1},{t:2000,fraction:1},
  ]};
  expect(wordFillEnd(word)).toBe(1800);
  expect(wordWipeFraction(word,1800,0)).toBe(1);
  expect(wordFillEnd({...word,points:[...word.points].reverse()})).toBe(2000);
});

it("unfolds Haunted's held vowel instead of freezing then flashing the next letter", () => {
  const word = { t: 30747, end: 32889, text: "lips", points: [
    {t:30747,fraction:0},{t:30767,fraction:.25},
    {t:30887,fraction:.25},{t:30907,fraction:.5},
    {t:32749,fraction:.5},{t:32769,fraction:.75},
    {t:32869,fraction:.75},{t:32889,fraction:1},
  ] };
  expect(wordWipeFraction(word, 31500, 0)).toBeLessThan(.5);
  expect(wordWipeFraction(word, 32500, 0)).toBeGreaterThan(wordWipeFraction(word, 31500, 0));
  expect(wordWipeFraction(word, 32749, 0)).toBe(.5);
  expect(wordWipeFraction(word, 30747, 0)).toBe(0);
  expect(wordWipeFraction(word, 32889, 0)).toBe(1);
  expect(wordFillEnd(word)).toBe(32889);
  expect(wordWipeFraction(word, 31280, 220)).toBe(wordWipeFraction(word,31500,0));
});
it("adds restrained dramatic lag while staying continuous and on time", () => {
  const word = {t:1000,end:3000,text:"held",points:[{t:1000,fraction:0},{t:3000,fraction:1}]};
  expect(wordWipeFraction(word,2000,0)).toBeCloseTo(.38);
  let previous=0;
  for(let t=1020;t<=3000;t+=20) {
    const fill=wordWipeFraction(word,t,0);
    expect(fill).toBeGreaterThan(previous);
    expect(fill-previous).toBeLessThan(.014);
    previous=fill;
  }
  expect(previous).toBe(1);
  expect(wordFillEnd(word)).toBe(3000);
});
// Captured failure: key 8009c5cda6f127, source 136770..139980,
// eight tokens first appear at 139718 (only 262ms before the next line).
// Timing values are from the cache; text is synthetic.
describe("source timing protects a missed line entrance", () => {
  const lines: LyricLine[] = [{t:136770,text:"one two three four five six seven eight"},{t:139980,text:"next"}];
  const late = Array.from({length:8},(_,i)=>({t:139718+i*80,end:139798+i*80,text:`word${i}`,line_t:136770}));
  it("uses ordinary source-line highlighting instead of a late burst", () => {
    const result=attachWords(lines,late);
    expect(result[0].words).toBeUndefined();
    expect(result[0].t).toBe(136770);
    expect(currentLineIndex(result,136770,0)).toBe(0);
    expect(late[0].t).toBe(139718);
  });
  it("removes stale attached words when a replacement misses the line", () => {
    const attached=[{...lines[0],words:late},lines[1]];
    const result=attachWords(attached,[{t:139990,end:140100,text:"next",line_t:139980}]);
    expect(result[0].words).toBeUndefined();
  });
  it("rejects malformed timing rather than poisoning the frame driver", () => {
    expect(attachWords(lines,[{t:NaN,end:139000,text:"one",line_t:136770}])[0].words).toBeUndefined();
    expect(attachWords(lines,[{t:137000,end:136900,text:"one",line_t:136770}])[0].words).toBeUndefined();
  });
  it("preserves early vocal entries and ordinary word timings", () => {
    const words=[{t:136370,end:137500,text:"one",line_t:136770}];
    expect(attachWords(lines,words)[0].words).toEqual(words);
  });
  it("does not mistake quick delivery followed by a rest for bad timing", () => {
    const fast=late.map((w,i)=>({...w,t:136770+i*25,end:136795+i*25}));
    expect(attachWords(lines,fast)[0].words).toEqual(fast);
  });
  it("leaves overlapping adlibs and ambiguous source windows alone", () => {
    expect(attachWords([{...lines[0],text:"one (oh)"},lines[1]],late)[0].words).toEqual(late);
    expect(attachWords([lines[0]],late)[0].words).toEqual(late);
    expect(attachWords([lines[0],{t:136770,text:"overlap"}],late)[0].words).toEqual(late);
  });
});
