import { describe, expect, it } from "vitest";
import { groupCredits } from "./credits";

describe("groupCredits", () => {
  it("lists each person once per section with all their roles, sections in a fixed order", () => {
    const groups = groupCredits([
      { role: "Lyrics", names: ["Kenzie"] },
      { role: "Composition", names: ["Kenzie", "Dem Jointz"] },
      { role: "Arrangement", names: ["Dem Jointz"] },
      { role: "Mixed by", names: ["Kim Cheol-sun"] },
      { role: "Vocal direction", names: ["kenzie"] },
    ]);
    expect(groups.map((g) => g.title)).toEqual(["Production", "Songwriting", "Engineering"]);
    expect(groups[0].people).toEqual([{ name: "kenzie", roles: ["Vocal direction"] }]);
    expect(groups[1].people).toEqual([
      { name: "Kenzie", roles: ["Lyrics", "Composition"] },
      { name: "Dem Jointz", roles: ["Composition", "Arrangement"] },
    ]);
    expect(groups[2].people).toEqual([{ name: "Kim Cheol-sun", roles: ["Mixing"] }]);
  });

  it("files pass-through roles by keyword, instruments under Performers", () => {
    const groups = groupCredits([
      { role: "Digital editor", names: ["Noh Min-ji"] },
      { role: "Bass", names: ["FLYT"] },
      { role: "Programming", names: ["Rado"] },
    ]);
    expect(groups.map((g) => [g.title, g.people[0].roles[0]])).toEqual([
      ["Production", "Programming"],
      ["Performers", "Bass"],
      ["Engineering", "Digital editor"],
    ]);
  });
});
