import { describe, expect, it, vi } from "vitest";
import { matchingCover } from "./fullCover";
vi.mock("./backend", () => ({}));
import type { NowPlaying } from "../types";

const np = (over: Partial<NowPlaying>): NowPlaying =>
  ({ player: "spotify", title: "Spark - WINTER Solo", artist: "aespa", ...over }) as NowPlaying;
const cover = { title: "Spark - WINTER Solo", artist: "aespa", url: "https://i.scdn.co/image/big" };

describe("matchingCover", () => {
  it("uses the full-size cover for the playing Spotify track", () => {
    expect(matchingCover(np({}), cover)).toBe(cover.url);
    expect(matchingCover(np({ title: " spark - winter solo " }), cover)).toBe(cover.url);
  });

  it("accepts artist lists that only overlap", () => {
    expect(matchingCover(np({ artist: "aespa" }), { ...cover, artist: "aespa, WINTER" })).toBe(cover.url);
  });

  it("ignores a cover from another track, artist or player", () => {
    expect(matchingCover(np({ title: "Next Song" }), cover)).toBeNull();
    expect(matchingCover(np({ artist: "HeeJin" }), cover)).toBeNull();
    expect(matchingCover(np({ player: "apple_music" }), cover)).toBeNull();
    expect(matchingCover(null, cover)).toBeNull();
    expect(matchingCover(np({}), null)).toBeNull();
  });
});
