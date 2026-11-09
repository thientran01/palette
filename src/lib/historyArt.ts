/*
 * Covers for Search's "From your history" rows that have no disk thumb.
 *
 * The thumb cache (history.rs) keeps only the newest 300 covers, which on
 * Thien's PC is about the last day of listening. The resurfacing picks are
 * chosen for being rested (days to months old), so nearly every pick misses
 * it and falls back to a Spotify search for its cover. Those lookups were
 * cached in memory only, so every app start, every new day-block and every
 * refresh paid up to four Spotify searches before the history rows (and the
 * discovery fetch behind them) could appear.
 *
 * This keeps the found cover urls across restarts (localStorage), and
 * `withinBudget` lets the rows land on time when Spotify is slow: covers
 * that miss the budget arrive in place a moment later. Only successes are
 * kept; a cover a lookup couldn't find is tried again next time.
 */

type Store = Pick<Storage, "getItem" | "setItem">;

const STORE_KEY = "pulse.historyArt";
/** A Spotify cover url for an album is stable for a long time; a month
 * bounds how long a changed one can linger. */
const TTL_MS = 30 * 86_400_000;
/** About 150 KB of JSON at the cap. */
const MAX_ENTRIES = 1500;

export interface ArtCache {
  get(key: string, now?: number): string | undefined;
  set(key: string, url: string, now?: number): void;
  clear(): void;
}

export function createArtCache(storage: Store | null): ArtCache {
  // Insertion order = age: a re-set moves a key to the end.
  let map: Map<string, [string, number]> | null = null;
  const load = () => {
    if (map) return map;
    map = new Map();
    try {
      const raw = storage?.getItem(STORE_KEY);
      const obj: unknown = raw ? JSON.parse(raw) : null;
      if (obj && typeof obj === "object") {
        for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
          if (Array.isArray(v) && typeof v[0] === "string" && typeof v[1] === "number") {
            map.set(k, [v[0], v[1]]);
          }
        }
      }
    } catch {
      // Unreadable storage: start empty.
    }
    return map;
  };
  const persist = () => {
    try {
      storage?.setItem(STORE_KEY, JSON.stringify(Object.fromEntries(load())));
    } catch {
      // Quota or a blocked store: memory still serves this run.
    }
  };
  return {
    get(key, now = Date.now()) {
      const hit = load().get(key);
      if (!hit) return undefined;
      if (now - hit[1] > TTL_MS) {
        load().delete(key);
        return undefined;
      }
      return hit[0];
    },
    set(key, url, now = Date.now()) {
      const m = load();
      m.delete(key);
      m.set(key, [url, now]);
      while (m.size > MAX_ENTRIES) {
        const oldest = m.keys().next().value;
        if (oldest === undefined) break;
        m.delete(oldest);
      }
      persist();
    },
    clear() {
      map = new Map();
      persist();
    },
  };
}

/** Settle `promises` as far as `budgetMs` allows: each slot holds its value
 * if it resolved in time, else `fallback`. Never rejects (a rejection counts
 * as `fallback`). */
export async function withinBudget<T>(
  promises: readonly Promise<T>[],
  budgetMs: number,
  fallback: T,
): Promise<T[]> {
  const out: T[] = promises.map(() => fallback);
  const settled = Promise.all(
    promises.map((p, i) =>
      p.then(
        (v) => {
          out[i] = v;
        },
        () => {},
      ),
    ),
  );
  let timer: ReturnType<typeof setTimeout> | undefined;
  await Promise.race([settled, new Promise<void>((r) => (timer = setTimeout(r, budgetMs)))]);
  clearTimeout(timer);
  return out.slice();
}
