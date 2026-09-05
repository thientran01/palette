import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
vi.mock("./backend", () => ({}));
vi.mock("./wordLead", () => ({ useWordLead: () => ({ leadMs: 160, nudges: 0 }), describeWordLead: () => "" }));
vi.mock("./posClock", () => ({ now: () => 1000, isPlaying: () => false, subscribe: () => () => {} }));
import { LyricsPanel } from "../LyricsPanel";

describe("word timing does not replace rows at line activation", () => {
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
});
