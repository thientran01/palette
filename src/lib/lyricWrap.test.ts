import { describe, expect, it } from "vitest";
import { MAX_KEPT_PAREN, keepHangulWords, lyricWrapSegments } from "./lyricWrap";

const kept = (text: string) => lyricWrapSegments(text).filter((s) => s.keep).map((s) => s.text);

describe("lyricWrapSegments", () => {
  it("keeps a short parenthetical ad-lib together", () => {
    expect(lyricWrapSegments("dreaming of you (oh-oh)")).toEqual([
      { text: "dreaming of you ", keep: false },
      { text: "(oh-oh)", keep: true },
    ]);
    expect(kept("흔들지 마, 반칙이야 (반칙이야)")).toEqual(["(반칙이야)"]);
    expect(kept("夜に（ずっと）")).toEqual(["（ずっと）"]);
  });
  it("lets a long parenthetical wrap at spaces but never at its hyphens", () => {
    const long = "(ooh-ooh, show me baby, show me all night long)";
    expect([...long].length).toBeGreaterThan(MAX_KEPT_PAREN);
    expect(kept(`say it ${long}`)).toEqual(["(ooh-ooh,"]);
  });
  it("leaves lines without parentheses alone and loses no text", () => {
    expect(lyricWrapSegments("I better not fall for you")).toEqual([
      { text: "I better not fall for you", keep: false },
    ]);
    for (const t of ["a (b) c (d-e) f", "(oh)", "no close (paren", "(ooh-ooh, show me baby, show me all night long) x"]) {
      expect(lyricWrapSegments(t).map((s) => s.text).join("")).toBe(t);
    }
  });
});

describe("keepHangulWords", () => {
  it("applies to Korean lines, mixed with English or not", () => {
    expect(keepHangulWords("눈 감아도 잠이 안 와, dreaming of you")).toBe(true);
    expect(keepHangulWords("반칙 없이, honestly")).toBe(true);
  });
  it("skips Japanese, Chinese and plain English lines", () => {
    expect(keepHangulWords("夜明けの空に 光を探して too late")).toBe(false);
    expect(keepHangulWords("사랑해 愛してる")).toBe(false);
    expect(keepHangulWords("Oh, say that you want me")).toBe(false);
  });
});
