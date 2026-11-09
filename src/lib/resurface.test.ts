import { describe, expect, it } from "vitest";
import type { HistoryEntry } from "../types";
import {
  aggregate,
  createHistoryLog,
  createShownMemory,
  DAY_MS,
  pickResurfaced,
  type Agg,
  type Listen,
} from "./resurface";

const NOW = Date.UTC(2026, 10, 9, 12);

function entry(i: number, started: number, ms = 200_000, artist = `artist ${i % 50}`): HistoryEntry {
  return {
    v: 1,
    key: `k${i}`,
    app_id: "Spotify.exe",
    player: "spotify",
    title: `song ${i}`,
    artist,
    album: "",
    started_at_ms: started,
    ended_at_ms: started + ms,
    ms_listened: ms,
    duration_ms: ms,
    spotify_uri: null,
  } as HistoryEntry;
}

/** A newest-first pager over an oldest-first log, like history_page. */
function pager(log: HistoryEntry[]) {
  const calls: (number | null)[] = [];
  const page = async (before: number | null, limit: number) => {
    calls.push(before);
    let end = log.length;
    if (before !== null) {
      const i = log.findIndex((e) => e.started_at_ms >= before);
      if (i !== -1) end = i;
    }
    return log.slice(Math.max(0, end - limit), end).reverse();
  };
  return { page, calls };
}

function memStore() {
  const data = new Map<string, string>();
  return {
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => void data.set(k, v),
  };
}

function agg(i: number, ms: number, daysAgo: number, extra: Partial<Agg> = {}): Agg {
  return {
    key: `k${i}`,
    title: `song ${i}`,
    artist: `artist ${i}`,
    ms,
    ms7: 0,
    msOnThisDay: 0,
    last: NOW - daysAgo * DAY_MS,
    uri: null,
    ...extra,
  };
}

/** 600 tracks spread over four months, a few played heavily this week. */
function library(): Agg[] {
  const out: Agg[] = [];
  for (let i = 0; i < 600; i++) {
    const daysAgo = (i * 7) % 120;
    out.push(agg(i, 60_000 + ((i * 7919) % 3_000_000), daysAgo + 0.5, {
      ms7: daysAgo < 7 ? 600_000 : 0,
    }));
  }
  return out;
}

describe("history log", () => {
  it("reads the whole log, then only the new plays", async () => {
    const log: HistoryEntry[] = [];
    for (let i = 0; i < 1000; i++) log.push(entry(i, NOW - (1000 - i) * 300_000));
    const { page, calls } = pager(log);
    const h = createHistoryLog(page, 200);
    expect((await h.sync()).length).toBe(1000);
    // Five full pages, then the empty one that says the log ended.
    expect(calls.length).toBe(6);

    log.push(entry(1000, NOW), entry(1001, NOW + 300_000));
    calls.length = 0;
    const after = await h.sync();
    expect(after.length).toBe(1002);
    expect(after[0].key).toBe("k1001");
    expect(calls).toEqual([null]);
  });

  it("reads 1000 plays a page by default", async () => {
    const log: HistoryEntry[] = [];
    for (let i = 0; i < 2500; i++) log.push(entry(i, NOW - (2500 - i) * 300_000));
    const { page, calls } = pager(log);
    expect((await createHistoryLog(page).sync()).length).toBe(2500);
    expect(calls.length).toBe(3);
  });

  it("keeps the raw rows, newest first, in step with the listens", async () => {
    const log: HistoryEntry[] = [];
    for (let i = 0; i < 450; i++) log.push(entry(i, NOW - (450 - i) * 300_000));
    const h = createHistoryLog(pager(log).page, 200);
    const rows = await h.syncEntries();
    expect(rows.length).toBe(450);
    expect(rows[0]).toBe(log[449]);
    expect(rows[449]).toBe(log[0]);
    log.push(entry(450, NOW));
    const after = await h.syncEntries();
    expect(after.length).toBe(451);
    expect(after[0].key).toBe("k450");
    h.reset();
    log.length = 0;
    expect(await h.syncEntries()).toEqual([]);
  });

  it("forgets everything on reset", async () => {
    const log = [entry(1, NOW - DAY_MS)];
    const h = createHistoryLog(pager(log).page);
    await h.sync();
    log.length = 0;
    h.reset();
    expect(await h.sync()).toEqual([]);
  });
});

describe("aggregate", () => {
  it("sums listens and splits this week and this date in earlier months", () => {
    const day = (ms: number) => new Date(ms).getUTCDate();
    const l = (started: number, ms: number): Listen => ({
      key: "a",
      title: "t",
      artist: "x",
      started,
      ended: started + ms,
      ms,
      uri: null,
    });
    const [a] = aggregate(
      [l(NOW - 2 * DAY_MS, 100_000), l(NOW - 30 * DAY_MS, 50_000), l(NOW - 31 * DAY_MS, 70_000)],
      NOW,
      day,
    );
    expect(a.ms).toBe(220_000);
    expect(a.ms7).toBe(100_000);
    // Nov 9 minus 31 days is Oct 9: the same date a month earlier.
    expect(a.msOnThisDay).toBe(70_000);
    expect(a.last).toBe(NOW - 2 * DAY_MS + 100_000);
  });
});

describe("pickResurfaced", () => {
  it("never repeats a song across refreshes while candidates last", () => {
    const tracks = library();
    const mem = createShownMemory(null);
    const seen = new Set<string>();
    mem.add(1, pickResurfaced(tracks, NOW, { seed: 1 }).map((p) => p.agg.key));
    for (let r = 1; r <= 30; r++) {
      const picks = pickResurfaced(tracks, NOW, { seed: 1000 + r, exclude: mem.get(1) });
      expect(picks).toHaveLength(4);
      for (const p of picks) {
        expect(seen.has(p.agg.key)).toBe(false);
        seen.add(p.agg.key);
      }
      mem.add(1, picks.map((p) => p.agg.key));
    }
  });

  it("gives the same summon set for the same seed", () => {
    const tracks = library();
    const keys = (seed: number) => pickResurfaced(tracks, NOW, { seed }).map((p) => p.agg.key);
    expect(keys(7)).toEqual(keys(7));
  });

  it("labels month-old favorites and keeps artists apart", () => {
    const picks = pickResurfaced(library(), NOW, { seed: 3 });
    expect(picks.map((p) => p.reason)).toContain("Forgotten favorite");
    expect(picks.map((p) => p.reason)).toContain("On repeat lately");
    expect(new Set(picks.map((p) => p.agg.artist)).size).toBe(4);
  });

  it("still fills four rows from a one-artist library", () => {
    const tracks = [0, 1, 2, 3, 4].map((i) => agg(i, 300_000, 3, { artist: "solo" }));
    const picks = pickResurfaced(tracks, NOW, { seed: 1 });
    expect(picks).toHaveLength(4);
    expect(new Set(picks.map((p) => p.agg.key)).size).toBe(4);
  });

  it("reuses a shown song only when nothing else is left", () => {
    const tracks = [0, 1, 2, 3, 4].map((i) => agg(i, 300_000, 3));
    const picks = pickResurfaced(tracks, NOW, { seed: 1, exclude: new Set(["k0", "k1"]) });
    expect(picks).toHaveLength(4);
    const keys = picks.map((p) => p.agg.key);
    expect(keys.slice(0, 3).sort()).toEqual(["k2", "k3", "k4"]);
  });

  it("skips songs under a minute of total listening", () => {
    const tracks = [agg(0, 30_000, 10), agg(1, 300_000, 10)];
    expect(pickResurfaced(tracks, NOW, { seed: 1 }).map((p) => p.agg.key)).toEqual(["k1"]);
  });
});

describe("shown memory", () => {
  it("keeps the current block across restarts and drops older blocks", () => {
    const store = memStore();
    createShownMemory(store).add(5, ["a", "b"]);
    expect([...createShownMemory(store).get(5)]).toEqual(["a", "b"]);
    expect(createShownMemory(store).get(6).size).toBe(0);
  });

  it("clear starts a new cycle", () => {
    const mem = createShownMemory(memStore());
    mem.add(5, ["a"]);
    mem.clear();
    expect(mem.get(5).size).toBe(0);
  });
});
