import { describe, expect, it } from "vitest";
import { isLatinWord, syllables } from "./syllables";
import { letterSegments } from "./letterRise";

const cut = (word: string) =>
  syllables(letterSegments(word).filter(s => s.letter).map(s => s.text)).join("·");

describe("syllables", () => {
  it("cuts common lyric words where they're sung", () => {
    const cases: Record<string, string> = {
      "away,": "a·way,", pretending: "pre·ten·ding", happy: "hap·py", beautiful: "beau·ti·ful",
      strength: "strength", love: "love", little: "litt·le", table: "tab·le", wanted: "wan·ted",
      loved: "loved", makes: "makes", faces: "fa·ces", crying: "cry·ing", yeah: "yeah",
      mother: "mo·ther", singer: "sing·er", lovely: "love·ly", something: "some·thing",
      "I'm": "I'm", "'Cause": "'Cause", nobody: "no·bo·dy", "starlight,": "star·light,",
    };
    for (const [word, want] of Object.entries(cases)) expect(cut(word), word).toBe(want);
  });
  it("tells Latin words from other scripts", () => {
    expect(isLatinWord("away,")).toBe(true);
    expect(isLatinWord("café")).toBe(true);
    expect(isLatinWord("한국")).toBe(false);
    expect(isLatinWord("ことば")).toBe(false);
    expect(isLatinWord("Blue moon이")).toBe(false);
    expect(isLatinWord("...")).toBe(false);
  });
});
