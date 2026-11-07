/*
 * The "Something different" rows, keyed by day-block and kept in
 * localStorage. An in-memory map was lost on every restart, so the first
 * summon after launching the app always paid for a full Last.fm + Spotify
 * walk, even when the same block had already been fetched an hour earlier.
 *
 * Only the newest KEEP_BLOCKS blocks are kept: a block is the cache key, and
 * older blocks are never asked for again. Storage can be missing or throw (a
 * private profile, a full quota), and the cache then behaves as the plain
 * in-memory map it replaced.
 */

const STORAGE_KEY = "pulse.discoveryCache";
const KEEP_BLOCKS = 2;

type Store = Pick<Storage, "getItem" | "setItem">;

export interface DiscoveryCache<T> {
  get(block: number): T[] | undefined;
  set(block: number, rows: T[]): void;
  delete(block: number): void;
}

export function createDiscoveryCache<T>(storage: Store | null): DiscoveryCache<T> {
  let map: Map<number, T[]> | null = null;

  const load = (): Map<number, T[]> => {
    if (map) return map;
    map = new Map();
    try {
      const raw = storage?.getItem(STORAGE_KEY);
      const obj: unknown = raw ? JSON.parse(raw) : null;
      if (obj && typeof obj === "object") {
        for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
          const block = Number(k);
          if (Number.isFinite(block) && Array.isArray(v)) map.set(block, v as T[]);
        }
      }
    } catch {
      // Unreadable storage or a garbled entry: start empty.
    }
    return map;
  };

  const persist = () => {
    const m = load();
    const keep = [...m.keys()].sort((a, b) => b - a).slice(0, KEEP_BLOCKS);
    for (const k of [...m.keys()]) if (!keep.includes(k)) m.delete(k);
    try {
      storage?.setItem(STORAGE_KEY, JSON.stringify(Object.fromEntries(m)));
    } catch {
      // Quota or a blocked store: the in-memory copy still serves this run.
    }
  };

  return {
    get: (block) => load().get(block),
    set: (block, rows) => {
      load().set(block, rows);
      persist();
    },
    delete: (block) => {
      if (load().delete(block)) persist();
    },
  };
}

/** window.localStorage, or null where reading it throws. */
export function browserStorage(): Store | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}
