import { describe, expect, it } from "vitest";
import type { HistoryEntry, PlaylistSummary, QueueTrack } from "../types";
import {
  dayLabel,
  filterRanked,
  fmtDuration,
  groupByDay,
  matchRank,
  matchSongs,
  nextSection,
  playlistSubtitle,
  songCount,
} from "./searchLibrary";

const track = (title: string, artist: string): QueueTrack => ({
  uri: `spotify:track:${title}`,
  title,
  artist,
  album: "",
  duration_ms: 200_000,
  art_url: null,
});

describe("section strip", () => {
  it("Tab cycles Songs → Library → History and wraps both ways", () => {
    expect(nextSection("songs", 1)).toBe("library");
    expect(nextSection("library", 1)).toBe("history");
    expect(nextSection("history", 1)).toBe("songs");
    expect(nextSection("songs", -1)).toBe("history");
  });
});

describe("library filter", () => {
  it("ranks starts-with over word-start over contains", () => {
    expect(matchRank("Late drives", "late")).toBe(0);
    expect(matchRank("Too late to sleep", "late")).toBe(1);
    expect(matchRank("Chocolate", "late")).toBe(2);
    expect(matchRank("Gym rotation", "late")).toBeNull();
  });

  it("matches Hangul and folds case", () => {
    expect(matchRank("새벽 드라이브", "드라이브")).toBe(1);
    expect(matchRank("LATE DRIVES", "late")).toBe(0);
  });

  it("keeps the original order inside a rank", () => {
    const names = ["Chocolate", "Late drives", "Too late", "Late again"];
    expect(filterRanked(names, "late", (n) => [n])).toEqual([
      "Late drives",
      "Late again",
      "Too late",
      "Chocolate",
    ]);
  });

  it("finds songs once, Liked Songs first, with their source", () => {
    const a = track("Late Again", "The Low Hours");
    const b = track("Lately", "Kaji");
    const found = matchSongs([a], [{ name: "2016 forever", tracks: [a, b] }], "late", 5);
    expect(found.map((s) => [s.track.title, s.from])).toEqual([
      ["Late Again", "Liked Songs"],
      ["Lately", "2016 forever"],
    ]);
  });
});

describe("labels", () => {
  const p: PlaylistSummary = {
    uri: "spotify:playlist:x",
    id: "x",
    name: "City pop rooms",
    owner: "andy",
    mine: false,
    total: 88,
    art_url: null,
  };

  it("counts songs and names other owners", () => {
    expect(songCount(1)).toBe("1 song");
    expect(songCount(53)).toBe("53 songs");
    expect(songCount(null)).toBe("");
    expect(playlistSubtitle(p, false)).toBe("by andy · 88 songs");
    expect(playlistSubtitle(p, true)).toBe("88 songs");
    expect(playlistSubtitle({ ...p, total: null }, false)).toBe("by andy");
  });

  it("formats song length", () => {
    expect(fmtDuration(221_000)).toBe("3:41");
    expect(fmtDuration(0)).toBe("");
  });
});

describe("history by day", () => {
  const now = new Date(2026, 10, 6, 18, 0).getTime();
  const entry = (at: number): HistoryEntry => ({
    v: 1,
    key: String(at),
    app_id: "Spotify",
    player: "spotify",
    title: "t",
    artist: "a",
    album: "",
    started_at_ms: at,
    ended_at_ms: at + 1,
    ms_listened: 1,
    duration_ms: 1,
    spotify_uri: null,
  });

  it("labels today and yesterday by name", () => {
    expect(dayLabel(new Date(2026, 10, 6, 0, 5).getTime(), now)).toBe("Today");
    expect(dayLabel(new Date(2026, 10, 5, 23, 20).getTime(), now)).toBe("Yesterday");
    expect(dayLabel(new Date(2026, 10, 3, 12).getTime(), now)).not.toBe("Yesterday");
  });

  it("opens a header only where the day changes", () => {
    const rows = groupByDay(
      [
        entry(new Date(2026, 10, 6, 17, 58).getTime()),
        entry(new Date(2026, 10, 6, 17, 55).getTime()),
        entry(new Date(2026, 10, 5, 23, 20).getTime()),
      ],
      now,
    );
    expect(rows.map((r) => r.header)).toEqual(["Today", null, "Yesterday"]);
  });
});
