import { describe, expect, it } from "vitest";
import type { NowPlaying } from "../types";
import { isProvisionalDuration, NO_SONG_MEMORY, rememberSong, trackIdOf } from "./trackId";

const np = (patch: Partial<NowPlaying>): NowPlaying => ({
  seq: 1,
  app_id: "Spotify.exe",
  player: "spotify",
  title: "Savior",
  artist: "THE BOYZ",
  album: "THE BOYZ",
  status: "playing",
  position_ms: 0,
  duration_ms: 204_000,
  position_at_ms: 0,
  can_seek: true,
  art_id: null,
  ...patch,
} as NowPlaying);

describe("trackIdOf", () => {
  it("ignores the late duration and album of the same song", () => {
    const first = trackIdOf(np({ duration_ms: 187_000, album: "" }));
    expect(trackIdOf(np({}))).toBe(first);
    expect(trackIdOf(np({ duration_ms: 0 }))).toBe(first);
  });

  it("changes with the song", () => {
    expect(trackIdOf(np({ title: "Instagram", artist: "DEAN" }))).not.toBe(trackIdOf(np({})));
  });

  it("is null without a session or a title", () => {
    expect(trackIdOf(null)).toBeNull();
    expect(trackIdOf(np({ player: "none" }))).toBeNull();
    expect(trackIdOf(np({ title: "" }))).toBeNull();
  });
});

describe("isProvisionalDuration", () => {
  const prev = { id: "spotify|A|a", durationMs: 204_000 };

  it("flags a new song still reporting the previous song's duration", () => {
    expect(isProvisionalDuration(prev, "spotify|B|b", 204_000)).toBe(true);
  });

  it("trusts a fresh duration, the same song, and a first track", () => {
    expect(isProvisionalDuration(prev, "spotify|B|b", 187_520)).toBe(false);
    expect(isProvisionalDuration(prev, "spotify|A|a", 204_000)).toBe(false);
    expect(isProvisionalDuration(null, "spotify|B|b", 204_000)).toBe(false);
  });
});

describe("rememberSong", () => {
  it("keeps waiting through an album straggler that still carries the old duration", () => {
    // A: played at 204s. Skip to B: title first (old album, old duration),
    // then the album fix (still the old duration), then the real duration.
    let memory = rememberSong(NO_SONG_MEMORY, "spotify|A|a", 204_000);
    memory = rememberSong(memory, "spotify|B|b", 204_000);
    expect(isProvisionalDuration(memory.previous, "spotify|B|b", 204_000)).toBe(true);
    memory = rememberSong(memory, "spotify|B|b", 204_000);
    expect(isProvisionalDuration(memory.previous, "spotify|B|b", 204_000)).toBe(true);
    memory = rememberSong(memory, "spotify|B|b", 187_520);
    expect(isProvisionalDuration(memory.previous, "spotify|B|b", 187_520)).toBe(false);
  });

  it("remembers the previous song's corrected duration", () => {
    let memory = rememberSong(NO_SONG_MEMORY, "spotify|A|a", 190_000);
    memory = rememberSong(memory, "spotify|A|a", 204_000);
    memory = rememberSong(memory, "spotify|B|b", 204_000);
    expect(memory.previous).toEqual({ id: "spotify|A|a", durationMs: 204_000 });
    expect(memory.current).toEqual({ id: "spotify|B|b", durationMs: 204_000 });
  });
});
