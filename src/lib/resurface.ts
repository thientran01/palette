/*
 * Search's "From your history" picks: the play log, its per-track totals,
 * and the four resurfacing rows chosen from them.
 *
 * Why this is not a ranked walk any more (Thien, 2026-11-09: "repeats a lot
 * when refreshing"). The old picker read only the newest 600 plays (about two
 * days of a 12,000-play log), dropped the last day, and moved every category
 * one step down its list per refresh. That left ~28 candidates, the filler
 * row on refresh N became the first row on refresh N+1, and the month-scale
 * labels ("Forgotten favorite", "On this day") could never fire. Replayed on
 * Thien's real log: 10 refreshes, 40 rows, 21 different songs.
 *
 * Now:
 * - The whole log is read once per app session and topped up with only the
 *   new plays on each summon (HistoryLog).
 * - Each category ranks its candidates, keeps the best few that haven't been
 *   shown yet, and draws one by weight with a seeded RNG, so a refresh moves
 *   on to the next favorites instead of re-serving the same top of the list.
 * - What the section has shown in the current day-block is remembered
 *   (ShownMemory, in localStorage), and refreshes exclude it, so neither a
 *   refresh nor a reopen-then-refresh repeats a song until the candidates run
 *   out. The summon set itself stays the stable, dated "curated" set for the
 *   block: a per-summon shuffle read as a bug before (Search.tsx blockSeed).
 */
import type { HistoryEntry } from "../types";

export const DAY_MS = 86_400_000;
const HOUR_MS = 3_600_000;
/** A pick must have been genuinely listened to (cumulative). */
export const RESURFACE_MIN_MS = 60_000;
/** History page size (history_page clamps to 200). */
const PAGE = 200;
/** Safety cap on pages read per sync: 100k plays. */
const MAX_PAGES = 500;
/** Rows in the section. */
export const RESURFACE_ROWS = 4;

/** One logged listen, trimmed to what the picker needs. */
export interface Listen {
  key: string;
  title: string;
  artist: string;
  started: number;
  ended: number;
  ms: number;
  uri: string | null;
}

/** One history track, aggregated across its listens. */
export interface Agg {
  key: string;
  title: string;
  artist: string;
  /** Cumulative ms listened, all time. */
  ms: number;
  /** Cumulative ms listened within the last 7 days (the "on repeat" signal). */
  ms7: number;
  /** Ms listened on this calendar date (day of month) in an earlier month. */
  msOnThisDay: number;
  last: number;
  uri: string | null;
}

type Pager = (before: number | null, limit: number) => Promise<HistoryEntry[]>;

/** The whole play log, newest first, read once and then topped up with only
 * the plays logged since the last sync. history.jsonl is append-only, so a
 * top-up pages from the newest entry back until it meets one it already
 * holds. reset() is for history-cleared. */
export interface HistoryLog {
  sync(): Promise<Listen[]>;
  reset(): void;
}

const listenId = (e: { key: string; started_at_ms: number }) => `${e.key}@${e.started_at_ms}`;

export function createHistoryLog(page: Pager): HistoryLog {
  let listens: Listen[] = [];
  let ids = new Set<string>();
  let epoch = 0;

  const run = async (): Promise<Listen[]> => {
    const mine = epoch;
    const fresh: Listen[] = [];
    const freshIds = new Set<string>();
    let before: number | null = null;
    for (let i = 0; i < MAX_PAGES; i++) {
      const batch = await page(before, PAGE);
      if (mine !== epoch) return listens;
      let met = false;
      for (const e of batch) {
        const id = listenId(e);
        if (ids.has(id)) {
          met = true;
          break;
        }
        if (freshIds.has(id)) continue;
        freshIds.add(id);
        fresh.push({
          key: e.key,
          title: e.title,
          artist: e.artist,
          started: e.started_at_ms,
          ended: e.ended_at_ms,
          ms: e.ms_listened,
          uri: e.spotify_uri ?? null,
        });
      }
      if (met || batch.length < PAGE) break;
      before = batch[batch.length - 1].started_at_ms;
    }
    if (fresh.length) {
      listens = fresh.concat(listens);
      for (const id of freshIds) ids.add(id);
    }
    return listens;
  };
  // One sync at a time: a summon and a Spotify flip can ask together, and
  // two overlapping top-ups would both prepend the same new plays.
  let flight: Promise<Listen[]> | null = null;

  return {
    sync() {
      if (!flight) {
        const p: Promise<Listen[]> = run().finally(() => {
          if (flight === p) flight = null;
        });
        flight = p;
      }
      return flight;
    },
    reset() {
      epoch++;
      flight = null;
      listens = [];
      ids = new Set();
    },
  };
}

/** Per-track totals over `listens` (any order). `localDay` maps a timestamp
 * to its local day of month (injectable for tests). */
export function aggregate(
  listens: readonly Listen[],
  now: number,
  localDay: (ms: number) => number = (ms) => new Date(ms).getDate(),
): Agg[] {
  const today = localDay(now);
  const byKey = new Map<string, Agg>();
  // When each track's kept URI was logged: the newest stamped listen wins.
  const uriAt = new Map<string, number>();
  for (const e of listens) {
    let cur = byKey.get(e.key);
    if (!cur) {
      cur = {
        key: e.key,
        title: e.title,
        artist: e.artist,
        ms: 0,
        ms7: 0,
        msOnThisDay: 0,
        last: 0,
        uri: null,
      };
      byKey.set(e.key, cur);
    }
    cur.ms += e.ms;
    const age = now - e.started;
    if (age < 7 * DAY_MS) cur.ms7 += e.ms;
    else if (age > 25 * DAY_MS && localDay(e.started) === today) cur.msOnThisDay += e.ms;
    if (e.ended >= cur.last) {
      cur.last = e.ended;
      // Newest title/artist spelling wins, as the newest URI does.
      cur.title = e.title;
      cur.artist = e.artist;
    }
    if (e.uri && e.ended >= (uriAt.get(e.key) ?? -Infinity)) {
      cur.uri = e.uri;
      uriAt.set(e.key, e.ended);
    }
  }
  return [...byKey.values()];
}

/** mulberry32: a tiny seeded PRNG, so a given block + refresh is repeatable. */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Log-compressed listening minutes: a song played 10× as long is a stronger
 * favorite, not 10× as likely to be drawn. */
const affinity = (t: Agg) => Math.log1p(t.ms / 60_000);
const daysSince = (t: Agg, now: number) => (now - t.last) / DAY_MS;
const artistOf = (t: Agg) => t.artist.trim().toLowerCase();

export interface ResurfacePick {
  agg: Agg;
  reason: string;
}

export interface PickOptions {
  /** Seeds the weighted draws. */
  seed: number;
  /** Keys not to show (already shown this block). */
  exclude?: ReadonlySet<string>;
}

/** Each category: who qualifies, how they rank, how many of the best unshown
 * ones stay in the draw, and each one's weight in it. */
interface Category {
  reason: string;
  eligible: (t: Agg) => boolean;
  rank: (t: Agg) => number;
  /** Only the top `cap` of the whole ranking qualify, shown or not, so a
   * long refresh streak runs out of "favorites" instead of calling the 300th
   * song one. */
  cap: number;
  /** How many of the best unshown candidates are in the draw. */
  window: number;
  weight: (t: Agg) => number;
}

/** Up to four resurfacing picks with reason labels, artist-diversified, never
 * two of the same track, and never an excluded track unless nothing else is
 * left to fill a row. */
export function pickResurfaced(tracks: readonly Agg[], now: number, opts: PickOptions): ResurfacePick[] {
  const exclude = opts.exclude ?? new Set<string>();
  const rand = rng(opts.seed);
  const all = tracks.filter((t) => t.ms >= RESURFACE_MIN_MS);
  const rested = (days: number) => (t: Agg) => daysSince(t, now) > days;

  // A young log (under a week of plays) has nothing "a while" old: that row
  // is plain "Most played" instead, so the section still says something
  // rather than collapsing to filler.
  const oldest = all.reduce((m, t) => Math.min(m, t.last), now);
  const young = now - oldest < 7 * DAY_MS;

  // The top of the all-time list belongs to "Forgotten favorite" and
  // "Haven't heard in a while"; the deep cut draws from below it.
  const byMs = all.slice().sort((a, b) => b.ms - a.ms);
  const favorites = new Set(byMs.slice(0, 40).map((t) => t.key));

  const categories: Category[] = [
    {
      reason: "On this day",
      eligible: (t) => t.msOnThisDay >= RESURFACE_MIN_MS && rested(14)(t),
      rank: (t) => t.msOnThisDay,
      cap: 40,
      window: 12,
      weight: (t) => Math.log1p(t.msOnThisDay / 60_000),
    },
    {
      reason: "Forgotten favorite",
      eligible: rested(30),
      rank: (t) => t.ms,
      cap: 100,
      window: 30,
      weight: affinity,
    },
    {
      reason: "On repeat lately",
      // About two plays this week, and not the song from an hour ago: that
      // one is still in your ears.
      eligible: (t) => t.ms7 >= 5 * RESURFACE_MIN_MS && now - t.last > 2 * HOUR_MS,
      rank: (t) => t.ms7,
      cap: 60,
      window: 10,
      weight: (t) => t.ms7,
    },
    {
      reason: young ? "Most played" : "Haven't heard in a while",
      eligible: young ? () => true : rested(7),
      // A favorite gains pull the longer it rests, up to a month.
      rank: (t) => (young ? t.ms : affinity(t) * Math.min(1, daysSince(t, now) / 30)),
      cap: 150,
      window: 30,
      weight: (t) => (young ? affinity(t) : affinity(t) * Math.min(1, daysSince(t, now) / 30)),
    },
    {
      reason: "Today's pick",
      eligible: (t) => !favorites.has(t.key) && rested(3)(t) && t.ms >= 2 * RESURFACE_MIN_MS,
      rank: (t) => t.ms,
      cap: Infinity,
      window: 200,
      weight: affinity,
    },
  ];

  const picks: ResurfacePick[] = [];
  const taken = new Set<string>();
  const artists = new Set<string>();
  const add = (agg: Agg, reason: string) => {
    picks.push({ agg, reason });
    taken.add(agg.key);
    artists.add(artistOf(agg));
  };
  const draw = (cands: Agg[], weight: (t: Agg) => number): Agg | null => {
    if (!cands.length) return null;
    const ws = cands.map((t) => Math.max(weight(t), 1e-6));
    let r = rand() * ws.reduce((s, w) => s + w, 0);
    for (let i = 0; i < cands.length; i++) {
      r -= ws[i];
      if (r <= 0) return cands[i];
    }
    return cands[cands.length - 1];
  };

  for (const c of categories) {
    if (picks.length >= RESURFACE_ROWS) break;
    const cands = all
      .filter(c.eligible)
      .sort((a, b) => c.rank(b) - c.rank(a))
      .slice(0, c.cap)
      .filter((t) => !taken.has(t.key) && !exclude.has(t.key) && !artists.has(artistOf(t)))
      .slice(0, c.window);
    const got = draw(cands, c.weight);
    if (got) add(got, c.reason);
  }

  // Top-up when categories came up empty: rested tracks first, then anything;
  // unshown before shown; a new artist before a repeat one (a one-artist
  // library still fills its rows).
  const fill = (pool: Agg[], diverse: boolean, allowShown: boolean) => {
    while (picks.length < RESURFACE_ROWS) {
      const cands = pool.filter(
        (t) =>
          !taken.has(t.key) &&
          (allowShown || !exclude.has(t.key)) &&
          (!diverse || !artists.has(artistOf(t))),
      );
      const got = draw(cands.slice(0, 60), affinity);
      if (!got) return;
      add(got, "Played before");
    }
  };
  const restedPool = byMs.filter(rested(1));
  for (const pool of [restedPool, byMs]) {
    fill(pool, true, false);
    fill(pool, false, false);
  }
  fill(byMs, false, true);
  return picks.slice(0, RESURFACE_ROWS);
}

type Store = Pick<Storage, "getItem" | "setItem">;

const SHOWN_KEY = "pulse.resurfaceShown";

/** The history keys the section has shown in the current day-block. Only one
 * block is kept: a new block starts a fresh, freshly curated cycle. Storage
 * can be missing or throw; it then lives in memory for this run. */
export interface ShownMemory {
  get(block: number): Set<string>;
  add(block: number, keys: readonly string[]): void;
  clear(): void;
}

export function createShownMemory(storage: Store | null): ShownMemory {
  let mem: { block: number; keys: Set<string> } | null = null;
  const load = (block: number) => {
    if (!mem) {
      mem = { block, keys: new Set() };
      try {
        const raw = storage?.getItem(SHOWN_KEY);
        const obj: unknown = raw ? JSON.parse(raw) : null;
        if (
          obj &&
          typeof obj === "object" &&
          (obj as { block?: unknown }).block === block &&
          Array.isArray((obj as { keys?: unknown }).keys)
        ) {
          mem.keys = new Set((obj as { keys: unknown[] }).keys.filter((k): k is string => typeof k === "string"));
        }
      } catch {
        // Unreadable storage: start empty.
      }
    }
    if (mem.block !== block) mem = { block, keys: new Set() };
    return mem;
  };
  const persist = () => {
    if (!mem) return;
    try {
      storage?.setItem(SHOWN_KEY, JSON.stringify({ block: mem.block, keys: [...mem.keys] }));
    } catch {
      // Quota or a blocked store: memory still serves this run.
    }
  };
  return {
    get: (block) => new Set(load(block).keys),
    add: (block, keys) => {
      const m = load(block);
      for (const k of keys) m.keys.add(k);
      persist();
    },
    clear: () => {
      mem = mem ? { block: mem.block, keys: new Set() } : null;
      try {
        storage?.setItem(SHOWN_KEY, JSON.stringify({ block: mem?.block ?? 0, keys: [] }));
      } catch {
        // Nothing to clear.
      }
    },
  };
}
