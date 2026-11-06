import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
vi.mock("./backend", () => ({}));
vi.mock("./wordLead", () => ({ useWordLead: () => ({ leadMs: 160, nudges: 0 }), describeWordLead: () => "" }));
vi.mock("./posClock", () => ({ now: () => 1000, isPlaying: () => false, subscribe: () => () => {} }));
import { LyricsPanel, lineDistances } from "../LyricsPanel";

describe("word timing does not replace rows at line activation", () => {
  it("turns off word wipes in both rooms while keeping the current line and original text", () => {
    for (const scale of ["base", "focus"] as const) {
      const html = renderToStaticMarkup(<LyricsPanel karaoke={false} scale={scale} seekable={true} leadMs={0} lines={[
        {t: 1000, text: "original lyric text", words: [{t: 1000, text: "aligned text", end: 1800}]},
        {t: 3000, text: "next lyric"},
      ]} />);
      expect(html).not.toContain("data-word");
      expect(html).toContain("original lyric text");
      expect(html).not.toContain("aligned text");
      expect(html).toContain('data-current="true"');
      expect(html).toContain("lyric-strong text-fg");
      expect(html).toContain('aria-label="Seek to original lyric text"');
    }
  });
  it("mounts upcoming word spans in their own source row and preserves untimed text", () => {
    const html = renderToStaticMarkup(<LyricsPanel seekable={false} leadMs={0} lines={[
      { t: 1000, text: "one", words: [{ t: 1000, text: "one", end: 1800 }] },
      { t: 3000, text: "two", words: [{ t: 2800, text: "two", end: 3800 }] },
      { t: 5000, text: "untimed line" },
    ]} />);
    expect(html).toContain('data-word-row="0"');
    expect(html).toContain('data-word-row="1"');
    expect((html.match(/data-word="true"/g) ?? []).length).toBe(2);
    expect(html).toContain("untimed line");
    expect(html).not.toContain('data-word-row="2"');
  });
  it("never changes font-weight on the current line, so it can't rewrap", () => {
    for (const scale of ["base", "focus"] as const) {
      const html = renderToStaticMarkup(<LyricsPanel scale={scale} seekable={true} leadMs={0} lines={[
        { t: 1000, text: "You, you, you, you, I better not fall for you" },
        { t: 3000, text: "I better not fall for you" },
      ]} />);
      expect(html).toContain('data-current="true"');
      expect(html).not.toMatch(/font-(medium|semibold|bold)/);
    }
  });
});

describe("lineDistances", () => {
  const sung = (t: number) => ({ t, text: `l${t}` });
  const brk = (t: number) => ({ t, text: "", end: t + 5000 });
  it("counts signed distance from the current line, clamped to ±3", () => {
    const lines = [sung(0), sung(1), sung(2), sung(3), sung(4), sung(5), sung(6)];
    expect(lineDistances(lines, 3)).toEqual([-3, -2, -1, 0, 1, 2, 3]);
    expect(lineDistances(lines, 0)).toEqual([0, 1, 2, 3, 3, 3, 3]);
  });
  it("skips a dormant break so the next sung line stays next", () => {
    const lines = [sung(0), sung(1), brk(2), sung(3)];
    expect(lineDistances(lines, 1)).toEqual([-1, 0, 1, 1]);
    expect(lineDistances(lines, 3)).toEqual([-2, -1, 0, 0]);
  });
  it("seats a current break between its neighbors", () => {
    const lines = [sung(0), brk(1), sung(2), sung(3)];
    expect(lineDistances(lines, 1)).toEqual([-1, 0, 1, 2]);
  });
  it("treats the intro (no current line) as just before the first line", () => {
    expect(lineDistances([sung(0), sung(1)], -1)).toEqual([1, 2]);
  });
});
