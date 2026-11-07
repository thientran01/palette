import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
vi.mock("./lib/backend", () => ({ commands: {}, onSettingsChanged: () => () => {} }));
import { CreditsBlock, groupRows, NO_CREDITS, splitName } from "./Credits";

describe("splitName", () => {
  it("mutes a romanized parenthetical", () => {
    expect(splitName("구종필 (JongPil Gu)", "STAYC")).toEqual({ main: "구종필", aside: "JongPil Gu" });
  });

  it("drops a group suffix that repeats the artist", () => {
    expect(splitName("Sieun (STAYC)", "STAYC")).toEqual({ main: "Sieun", aside: null });
    expect(splitName("JOY (Red Velvet)", "Red Velvet")).toEqual({ main: "JOY", aside: null });
  });

  it("keeps the outer parenthetical when a name has two", () => {
    expect(splitName("라도 (Rado) (Black Eyed Pilseung)", "STAYC")).toEqual({
      main: "라도 (Rado)",
      aside: "Black Eyed Pilseung",
    });
  });

  it("leaves plain names alone", () => {
    expect(splitName("Joe LaPorta", "STAYC")).toEqual({ main: "Joe LaPorta", aside: null });
  });
});

describe("groupRows", () => {
  it("splits writing, performance and engineering into runs", () => {
    const rows = [
      { role: "Produced by", names: ["A"] },
      { role: "Lyrics", names: ["B"] },
      { role: "Vocals", names: ["C"] },
      { role: "Guitar", names: ["D"] },
      { role: "Recorded by", names: ["E"] },
      { role: "Mastered by", names: ["F"] },
      { role: "Digital audio editor", names: ["G"] },
    ];
    expect(groupRows(rows).map((g) => g.map((r) => r.role))).toEqual([
      ["Produced by", "Lyrics"],
      ["Vocals", "Guitar"],
      ["Recorded by", "Mastered by", "Digital audio editor"],
    ]);
  });

  it("omits empty runs", () => {
    expect(groupRows([{ role: "Written by", names: ["A"] }])).toHaveLength(1);
  });
});

describe("CreditsBlock", () => {
  it("renders nothing without rows", () => {
    expect(renderToStaticMarkup(<CreditsBlock credits={NO_CREDITS} />)).toBe("");
  });

  it("renders liner notes with a source link when Genius gave a page", () => {
    const html = renderToStaticMarkup(
      <CreditsBlock
        credits={{
          artist: "STAYC",
          url: "https://genius.com/Stayc-teddy-bear-lyrics",
          rows: [
            { role: "Mixed by", names: ["구종필 (JongPil Gu)"] },
            { role: "Vocals", names: ["Sieun (STAYC)"] },
          ],
        }}
        scale="focus"
      />,
    );
    expect(html).toContain("Credits</span>");
    expect(html).toContain('<span class="text-muted"> JongPil Gu</span>');
    expect(html).not.toContain("(STAYC)");
    expect(html).toContain("<button");
  });

  it("falls back to plain attribution without a page", () => {
    const html = renderToStaticMarkup(
      <CreditsBlock credits={{ artist: "", url: null, rows: [{ role: "Written by", names: ["A"] }] }} />,
    );
    expect(html).not.toContain("<button");
    expect(html).toContain("Credits from Genius");
  });
});