import { describe, expect, it } from "vitest";
import { groupCredits, type CreditName } from "./credits";

const n = (main: string, aside: string | null = null): CreditName => ({ main, aside });

describe("groupCredits", () => {
  it("merges each person's roles, then shares a line between people with the same roles", () => {
    const groups = groupCredits([
      { role: "Lyrics", names: ["Kenzie", "Chrishan"] },
      { role: "Composition", names: ["Kenzie", "Chrishan", "Dem Jointz"] },
      { role: "Arrangement", names: ["Dem Jointz"] },
      { role: "Mixed by", names: ["Kim Cheol-sun"] },
      { role: "Produced by", names: ["Mob (KOR)", "SAINT MINO"] },
    ]);
    expect(groups.map((g) => g.title)).toEqual(["Production", "Songwriting", "Engineering"]);
    expect(groups[0].lines).toEqual([{ roles: ["Producer"], names: [n("Mob", "KOR"), n("SAINT MINO")] }]);
    expect(groups[1].lines).toEqual([
      { roles: ["Lyrics", "Composition"], names: [n("Kenzie"), n("Chrishan")] },
      { roles: ["Composition", "Arrangement"], names: [n("Dem Jointz")] },
    ]);
    expect(groups[2].lines).toEqual([{ roles: ["Mixing"], names: [n("Kim Cheol-sun")] }]);
  });

  it("drops a tag that only repeats the playing artist and keeps others as an aside", () => {
    const groups = groupCredits(
      [{ role: "Vocals", names: ["Gawon (MEOVV)", "Sooin (meovv)", "구종필 (JongPil Gu)", "NARIN (나린)"] }],
      "MEOVV",
    );
    expect(groups[0].lines[0].names).toEqual([n("Gawon"), n("Sooin"), n("구종필", "JongPil Gu"), n("NARIN", "나린")]);
  });

  it("files pass-through roles by keyword, instruments under Performers", () => {
    const groups = groupCredits([
      { role: "Digital editor", names: ["Noh Min-ji"] },
      { role: "Bass", names: ["FLYT"] },
      { role: "Programming", names: ["Rado"] },
    ]);
    expect(groups.map((g) => [g.title, g.lines[0].roles[0]])).toEqual([
      ["Production", "Programming"],
      ["Performers", "Bass"],
      ["Engineering", "Digital editor"],
    ]);
  });
});
