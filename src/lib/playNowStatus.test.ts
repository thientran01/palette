import { describe, expect, it } from "vitest";
import { playNowNote } from "./playNowStatus";

describe("playNowNote", () => {
  it("stays quiet on ok — the track change is the confirmation", () => {
    expect(playNowNote("ok")).toBeNull();
  });

  it("names every known failure the backend returns", () => {
    expect(playNowNote("no_device")).toBe("Open Spotify somewhere first");
    expect(playNowNote("busy")).toBe("Still landing the last jump");
    expect(playNowNote("diverged")).toBe("Queue moved on — try again");
    expect(playNowNote("gone")).toBe("Queue moved on — try again");
    expect(playNowNote("partial")).toBe("Played — some items couldn't re-queue");
    expect(playNowNote("offline")).toBe("Spotify unreachable");
    expect(playNowNote("disconnected")).toBe("Spotify unreachable");
  });
});
