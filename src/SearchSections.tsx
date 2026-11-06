/*
 * The search window's Library and History sections (Search.tsx owns the
 * shell, the input, the keys and Songs). Split by intent (design board
 * "1 · Split by intent", 2026-11): the queue button answers "what plays
 * next"; Search answers "find me something" and holds Songs, Library and
 * History.
 *
 * Library = Liked Songs, the user's playlists, then the ones they follow.
 * Loaded ONCE per app session (library.rs), cached in this realm and
 * filtered locally on every keystroke; public "Playlists on Spotify" come
 * from a debounced playlist search below. Enter on a playlist opens its songs
 * in the same window (Play in the header); Shift+Enter plays it outright.
 *
 * History = the local play log (history.rs history_page), newest first,
 * grouped by day with play times.
 *
 * Monochrome like the rest of this window (Search.tsx header): the design
 * board tinted the active tab icon, the Liked Songs tile and the playing row
 * with the song accent; this window renders no now-playing identity, so
 * those stay neutral.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { commands, onHistoryCleared, onSpotifyStatus } from "./lib/backend";
import {
  filterRanked,
  fmtDuration,
  groupByDay,
  likedContext,
  matchRank,
  matchSongs,
  norm,
  playlistSubtitle,
  playTime,
  songCount,
  sortRecent,
} from "./lib/searchLibrary";
import type { HistoryEntry, NowContext, PlaylistSummary, QueueTrack } from "./types";

/** One playable song row (Songs, Library songs, History). */
export interface SearchRow {
  key: string;
  title: string;
  artist: string;
  artUrl: string | null;
  /** Full track when it came from Spotify; history rows resolve on demand. */
  track: QueueTrack | null;
  uri: string | null;
  /** Why a resurfaced row is here — rendered on the row. Unlabeled, the
   * picks read as a bug ("why these two? why did one change?") — Thien's
   * live feedback, 2026-07-12. */
  reason?: string;
}

export function trackRow(t: QueueTrack): SearchRow {
  return { key: t.uri, title: t.title, artist: t.artist, artUrl: t.art_url, track: t, uri: t.uri };
}

/** Something Enter can open: Liked Songs or a playlist. */
export interface OpenTarget {
  kind: "liked" | "playlist";
  /** Playback context. */
  uri: string;
  /** Playlist id ("" for Liked Songs). */
  id: string;
  name: string;
  subtitle: string;
  artUrl: string | null;
  /** Spotify lists the songs (Liked Songs, owned or collaborative). */
  readable: boolean;
}

/** One row of the Library / drill-in / History lists. */
export interface Item {
  key: string;
  /** A section header rendered above this row. */
  header?: string | null;
  title: string;
  subtitle: string;
  artUrl: string | null;
  /** Placeholder tile when there's no art. */
  tile?: "liked" | "playlist";
  right?: string;
  /** The right label marks what's playing ("Playing" / "Now"): brighter
   * text, never accent. */
  rightActive?: boolean;
  /** History rows: the thumb-cache key their art resolves through
   * (useHistoryThumbs), looked up at render. */
  thumbKey?: string;
  song?: SearchRow;
  /** Song inside a playlist: Enter plays it in this context. */
  context?: string;
  open?: OpenTarget;
}

// ---- library cache (lives as long as the window: the whole app session) ----

type LibStatus = "idle" | "loading" | "ok" | "needs_reconnect" | "offline" | "disconnected";

export interface Library {
  status: LibStatus;
  userId: string;
  playlists: PlaylistSummary[];
  /** Context uris from recently played, newest first. */
  recent: string[];
  /** null until Liked Songs land (it loads after the playlists). */
  liked: QueueTrack[] | null;
  likedTotal: number | null;
  /** The Liked Songs load failed (the playlists may still be fine). */
  likedFailed: boolean;
  loadedAt: number;
}

const EMPTY_LIB: Library = {
  status: "idle",
  userId: "",
  playlists: [],
  recent: [],
  liked: null,
  likedTotal: null,
  likedFailed: false,
  loadedAt: 0,
};
/** A Library opened after this long since the last load refreshes in the
 * background (the cached list keeps showing meanwhile). */
const LIB_TTL_MS = 30 * 60_000;

let libCache: Library = EMPTY_LIB;
/** Songs of opened playlists ("ok" and "restricted" answers only — a
 * transient miss must not pin a dead playlist for the session). */
const playlistCache = new Map<string, { status: "ok" | "restricted"; tracks: QueueTrack[] }>();

/** The cached library, loaded on first activation (and refreshed past the
 * TTL). A Spotify connection change (connect, the scope reconnect,
 * disconnect) drops the cache — the grant behind it changed. */
export function useLibrary(active: boolean): Library {
  const [lib, setLib] = useState<Library>(libCache);
  const loading = useRef(false);
  const activeRef = useRef(active);
  activeRef.current = active;

  const load = useCallback(async () => {
    if (loading.current) return;
    if (libCache.status === "ok" && Date.now() - libCache.loadedAt < LIB_TTL_MS) {
      setLib(libCache);
      return;
    }
    loading.current = true;
    if (libCache.status !== "ok") {
      libCache = { ...EMPTY_LIB, status: "loading" };
      setLib(libCache);
    }
    try {
      // Both start now; playlists land first so the list paints before a
      // big Liked Songs walk finishes.
      const likedP = commands.spotifyLiked();
      const l = await commands.spotifyLibrary();
      const kept = libCache.status === "ok" ? libCache : null;
      if (l.status !== "ok" && kept) {
        // A failed background refresh keeps the list the user is looking at.
        loading.current = false;
        return;
      }
      libCache = {
        status: l.status,
        userId: l.user_id,
        playlists: l.playlists,
        recent: l.recent_contexts ?? [],
        liked: kept?.liked ?? null,
        likedTotal: kept?.likedTotal ?? null,
        likedFailed: false,
        loadedAt: Date.now(),
      };
      setLib(libCache);
      if (l.status !== "ok") return;
      const liked = await likedP.catch(() => null);
      libCache =
        liked?.status === "ok"
          ? { ...libCache, liked: liked.tracks, likedTotal: liked.total ?? liked.tracks.length }
          : { ...libCache, likedFailed: libCache.liked === null };
      setLib(libCache);
    } catch {
      if (libCache.status !== "ok") {
        libCache = { ...EMPTY_LIB, status: "offline" };
        setLib(libCache);
      }
    } finally {
      loading.current = false;
    }
  }, []);

  useEffect(() => {
    if (active) void load();
  }, [active, load]);

  // The first status callback is the seed, not a change.
  const seenStatus = useRef<boolean | null>(null);
  useEffect(
    () =>
      onSpotifyStatus((s) => {
        const first = seenStatus.current === null;
        seenStatus.current = s.connected;
        // Every event after the seed counts, even connected → connected: a
        // re-consent while already connected IS the scope reconnect.
        if (first) return;
        libCache = EMPTY_LIB;
        playlistCache.clear();
        setLib(libCache);
        if (activeRef.current && s.connected) void load();
      }),
    [load],
  );
  return lib;
}

/** Songs for an opened target: Liked Songs from the cache, a playlist from
 * playlistCache or a fetch. */
export function useOpenedTracks(
  target: OpenTarget | null,
  lib: Library,
): { status: "loading" | "ok" | "restricted" | "offline"; tracks: QueueTrack[] } {
  const [state, setState] = useState<{
    id: string;
    status: "loading" | "ok" | "restricted" | "offline";
    tracks: QueueTrack[];
  }>({ id: "", status: "loading", tracks: [] });
  const id = target?.kind === "playlist" ? target.id : "";
  useEffect(() => {
    if (!id) return;
    const hit = playlistCache.get(id);
    if (hit) {
      setState({ id, ...hit });
      return;
    }
    let alive = true;
    setState({ id, status: "loading", tracks: [] });
    void commands
      .spotifyPlaylistTracks(id)
      .then((r) => {
        if (!alive) return;
        if (r.status === "ok" || r.status === "restricted") {
          const entry = { status: r.status, tracks: r.tracks };
          playlistCache.set(id, entry);
          setState({ id, ...entry });
        } else {
          setState({ id, status: "offline", tracks: [] });
        }
      })
      .catch(() => {
        if (alive) setState({ id, status: "offline", tracks: [] });
      });
    return () => {
      alive = false;
    };
  }, [id]);
  if (!target) return { status: "loading", tracks: [] };
  if (target.kind === "liked") {
    if (lib.liked) return { status: "ok", tracks: lib.liked };
    return { status: lib.likedFailed ? "offline" : "loading", tracks: [] };
  }
  if (!target.readable) return { status: "restricted", tracks: [] };
  return state.id === id ? state : { status: "loading", tracks: [] };
}

// ---- what's playing ----

/** A fresh currently-playing read whenever the section opens or the window
 * is summoned (`tick`), so the "Playing" labels follow changes made in the
 * Spotify app too. */
export function useNowContext(active: boolean, tick: number): NowContext | null {
  const [now, setNow] = useState<NowContext | null>(null);
  useEffect(() => {
    if (!active) return;
    let alive = true;
    void commands
      .spotifyNowContext()
      .then((n) => {
        if (alive) setNow(n);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [active, tick]);
  return now;
}

/** Whether `target` is the context Spotify is playing. Liked Songs can come
 * back as any `…:collection` spelling. */
export function isPlayingTarget(target: { uri: string; kind?: string }, now: NowContext | null): boolean {
  const ctx = now?.context_uri;
  if (!ctx) return false;
  if (target.uri.endsWith(":collection")) return ctx.endsWith(":collection");
  return ctx === target.uri;
}

// ---- public playlist search ----

const PUBLIC_DEBOUNCE_MS = 300;
const PUBLIC_LIMIT = 5;

/** Debounced, latest-wins "Playlists on Spotify" for a non-empty query. */
export function usePublicPlaylists(
  query: string,
  active: boolean,
): { pending: boolean; playlists: PlaylistSummary[] } {
  const [res, setRes] = useState<{ q: string; playlists: PlaylistSummary[] }>({ q: "", playlists: [] });
  const gen = useRef(0);
  const q = query.trim();
  useEffect(() => {
    if (!active || !q) return;
    const t = window.setTimeout(() => {
      const g = ++gen.current;
      void commands
        .spotifySearchPlaylists(q, PUBLIC_LIMIT)
        .then((r) => {
          if (g === gen.current) setRes({ q, playlists: r.status === "ok" ? r.playlists : [] });
        })
        .catch(() => {
          if (g === gen.current) setRes({ q, playlists: [] });
        });
    }, PUBLIC_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [q, active]);
  if (!q) return { pending: false, playlists: [] };
  return res.q === q ? { pending: false, playlists: res.playlists } : { pending: true, playlists: [] };
}

// ---- item builders ----

const LIKED_NAME = "Liked Songs";
/** Library songs shown under a filter (the playlists are the point). */
const SONG_MATCHES = 6;

function likedTarget(lib: Library): OpenTarget {
  return {
    kind: "liked",
    uri: likedContext(lib.userId),
    id: "",
    name: LIKED_NAME,
    subtitle:
      lib.likedTotal !== null ? songCount(lib.likedTotal) : lib.likedFailed ? "" : "Loading…",
    artUrl: null,
    readable: true,
  };
}

function playlistTarget(p: PlaylistSummary, ownedByMe: boolean): OpenTarget {
  return {
    kind: "playlist",
    uri: p.uri,
    id: p.id,
    name: p.name,
    subtitle: playlistSubtitle(p, ownedByMe),
    artUrl: p.art_url,
    readable: p.mine,
  };
}

function targetItem(t: OpenTarget, right?: string): Item {
  return {
    key: t.uri,
    title: t.name,
    subtitle: t.subtitle,
    artUrl: t.artUrl,
    tile: t.kind === "liked" ? "liked" : "playlist",
    right,
    open: t,
  };
}

/** The Library list. No query: Liked Songs, your playlists, then the ones
 * you follow. A query: matching library playlists and songs under "Your
 * library", then public playlists under "Playlists on Spotify". */
export function libraryItems(
  lib: Library,
  query: string,
  publicHits: PlaylistSummary[],
  now: NowContext | null = null,
): Item[] {
  if (lib.status !== "ok") return [];
  return markPlaying(libraryItemsBase(lib, query, publicHits, now), now);
}

/** "Playing" on the playlist Spotify is playing, over its usual label. */
function markPlaying(items: Item[], now: NowContext | null): Item[] {
  return items.map((it) =>
    it.open && isPlayingTarget(it.open, now) ? { ...it, right: "Playing", rightActive: true } : it,
  );
}

function libraryItemsBase(
  lib: Library,
  query: string,
  publicHits: PlaylistSummary[],
  now: NowContext | null,
): Item[] {
  const q = norm(query);
  // "Yours" = owned or collaborative (Spotify lists their songs); followed
  // playlists come after, labeled.
  // Recently played first within each group (Spotify's own order, roughly
  // newest playlist first, read as random — Thien, 2026-11-06).
  const own = sortRecent(lib.playlists.filter((p) => p.mine), lib.recent, now);
  const followed = sortRecent(lib.playlists.filter((p) => !p.mine), lib.recent, now);
  const ordered = [...own, ...followed];
  const items: Item[] = [];
  if (!q) {
    items.push(targetItem(likedTarget(lib)));
    own.forEach((p) => items.push(targetItem(playlistTarget(p, true))));
    followed.forEach((p) => items.push(targetItem(playlistTarget(p, false), "Following")));
  } else {
    if (matchRank(LIKED_NAME, q) !== null) items.push(targetItem(likedTarget(lib), "Playlist"));
    filterRanked(ordered, q, (p) => [p.name]).forEach((p) =>
      items.push(targetItem(playlistTarget(p, p.mine), "Playlist")),
    );
    const opened = ordered
      .map((p) => ({ name: p.name, tracks: playlistCache.get(p.id)?.tracks ?? [] }))
      .filter((p) => p.tracks.length > 0);
    matchSongs(lib.liked ?? [], opened, q, SONG_MATCHES).forEach((s) =>
      items.push({
        key: `song:${s.track.uri}`,
        title: s.track.title,
        subtitle: `${s.track.artist} · in ${s.from}`,
        artUrl: s.track.art_url,
        right: "Song",
        song: trackRow(s.track),
      }),
    );
  }
  if (items.length) items[0].header = "Your library";
  if (q) {
    const mineIds = new Set(lib.playlists.map((p) => p.id));
    const pub = publicHits
      .filter((p) => !mineIds.has(p.id))
      .map((p) => targetItem(playlistTarget(p, false), "Public"));
    if (pub.length) pub[0].header = "Playlists on Spotify";
    items.push(...pub);
  }
  return items;
}

/** An opened playlist's songs, filtered by the drill-in query. */
export function drillItems(
  target: OpenTarget,
  tracks: QueueTrack[],
  query: string,
  now: NowContext | null = null,
): Item[] {
  const q = query.trim();
  const list = q ? filterRanked(tracks, q, (t) => [t.title, t.artist]) : tracks;
  const nowUri = now?.track_uri ?? null;
  return list.map((t, i) => ({
    // Playlists can hold the same song twice; the index keeps keys unique.
    key: `${i}:${t.uri}`,
    title: t.title,
    subtitle: t.artist,
    artUrl: t.art_url,
    right: t.uri === nowUri ? "Now" : fmtDuration(t.duration_ms),
    rightActive: t.uri === nowUri,
    song: trackRow(t),
    context: target.uri,
  }));
}

// ---- history ----

const HISTORY_PAGE = 200;

/** The play log, newest first: a fresh first page every time the section
 * opens (a local read), more pages on scroll. */
export function useHistoryList(active: boolean): {
  entries: HistoryEntry[];
  loadMore: () => void;
  exhausted: boolean;
} {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [exhausted, setExhausted] = useState(false);
  const gen = useRef(0);
  const loading = useRef(false);
  const entriesRef = useRef(entries);
  entriesRef.current = entries;
  const exhaustedRef = useRef(exhausted);
  exhaustedRef.current = exhausted;

  const seed = useCallback(() => {
    const g = ++gen.current;
    loading.current = true;
    void commands
      .historyPage(null, HISTORY_PAGE)
      .then((page) => {
        if (g !== gen.current) return;
        setEntries(page);
        setExhausted(page.length < HISTORY_PAGE);
      })
      .catch(() => {})
      .finally(() => {
        if (g === gen.current) loading.current = false;
      });
  }, []);

  const activeRef = useRef(active);
  activeRef.current = active;
  useEffect(() => {
    if (active) seed();
  }, [active, seed]);
  useEffect(
    () =>
      onHistoryCleared(() => {
        thumbCache.clear();
        if (activeRef.current) seed();
        else {
          gen.current += 1;
          setEntries([]);
        }
      }),
    [seed],
  );

  const loadMore = useCallback(() => {
    const cur = entriesRef.current;
    if (loading.current || exhaustedRef.current || cur.length === 0) return;
    const g = gen.current;
    loading.current = true;
    void commands
      .historyPage(cur[cur.length - 1].started_at_ms, HISTORY_PAGE)
      .then((page) => {
        if (g !== gen.current) return;
        setEntries((prev) => [...prev, ...page]);
        if (page.length < HISTORY_PAGE) setExhausted(true);
      })
      .catch(() => {})
      .finally(() => {
        if (g === gen.current) loading.current = false;
      });
  }, []);
  return { entries, loadMore, exhausted };
}

/** Thumb data URLs by history key (successes and misses both — a key's
 * thumb doesn't appear later in a way this list needs). */
const thumbCache = new Map<string, string | null>();

/** Resolves thumbs for `keys` (the rendered rows only) and re-renders as
 * they land. */
export function useHistoryThumbs(keys: string[]): Map<string, string | null> {
  const [, setTick] = useState(0);
  const joined = keys.join("|");
  useEffect(() => {
    let alive = true;
    const missing = [...new Set(keys)].filter((k) => !thumbCache.has(k));
    if (!missing.length) return;
    void Promise.all(
      missing.map((k) =>
        commands
          .historyThumbUrl(k)
          .then((u) => thumbCache.set(k, u))
          .catch(() => thumbCache.set(k, null)),
      ),
    ).then(() => {
      if (alive) setTick((t) => t + 1);
    });
    return () => {
      alive = false;
    };
    // `joined` is the identity of `keys`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [joined]);
  return thumbCache;
}

/** History rows: filtered by the query (kept chronological, so the day
 * headers stay true), grouped by day, the play time on the right. */
export function historyItems(entries: HistoryEntry[], query: string, now: number): Item[] {
  const q = norm(query);
  const list = q
    ? entries.filter((e) => matchRank(e.title, q) !== null || matchRank(e.artist, q) !== null)
    : entries;
  return groupByDay(list, now).map(({ entry, header }) => ({
    key: `${entry.key}:${entry.started_at_ms}`,
    header,
    title: entry.title,
    subtitle: entry.artist,
    artUrl: null,
    thumbKey: entry.key,
    right: playTime(entry.started_at_ms),
    song: {
      key: entry.key,
      title: entry.title,
      artist: entry.artist,
      artUrl: null,
      track: null,
      uri: entry.spotify_uri,
    },
  }));
}

// ---- glyphs ----

const stroke = {
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

export function SongsGlyph({ size = 14 }: { size?: number }) {
  return (
    <svg {...stroke} width={size} height={size}>
      <circle cx="7" cy="7" r="4.2" />
      <path d="M 10.3,10.3 L 13.2,13.2" />
    </svg>
  );
}
export function LibraryGlyph({ size = 14 }: { size?: number }) {
  return (
    <svg {...stroke} width={size} height={size}>
      <path d="M 3.2,3.2 L 3.2,12.8" />
      <path d="M 6.4,3.2 L 6.4,12.8" />
      <path d="M 9.2,3.8 L 12.8,12.2" />
    </svg>
  );
}
export function HistoryGlyph({ size = 14 }: { size?: number }) {
  return (
    <svg {...stroke} width={size} height={size}>
      <circle cx="8" cy="8" r="5.6" />
      <path d="M 8,5 L 8,8 L 10.2,9.4" />
    </svg>
  );
}
/** Tab key: an arrow into a bar. */
export function TabGlyph() {
  return (
    <svg {...stroke} width={12} height={12}>
      <path d="M 2.5,8 L 11.5,8" />
      <path d="M 8.5,5 L 11.5,8 L 8.5,11" />
      <path d="M 13.5,4.5 L 13.5,11.5" />
    </svg>
  );
}
function HeartGlyph() {
  return (
    <svg viewBox="0 0 16 16" width="13" height="13" fill="currentColor" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" aria-hidden>
      <path d="M8,13 C5.3,11.1 2.7,9 2.7,6.3 C2.7,4.5 4,3.2 5.6,3.2 C6.7,3.2 7.5,3.8 8,4.7 C8.5,3.8 9.3,3.2 10.4,3.2 C12,3.2 13.3,4.5 13.3,6.3 C13.3,9 10.7,11.1 8,13 Z" />
    </svg>
  );
}
export function PlayGlyph() {
  return (
    <svg viewBox="0 0 16 16" width="11" height="11" fill="currentColor" aria-hidden>
      <path d="M 5,3.2 L 12.6,8 L 5,12.8 Z" />
    </svg>
  );
}

/** The drill-in breadcrumb's back chevron. */
export function BackGlyph() {
  return (
    <svg viewBox="0 0 16 16" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M10,3.5 L5.5,8 L10,12.5" />
    </svg>
  );
}

/** A tile for Liked Songs or a playlist without art — neutral, never accent. */
export function ItemTile({ tile, url, size }: { tile: "liked" | "playlist"; url: string | null; size: number }) {
  const [failed, setFailed] = useState<string | null>(null);
  return (
    <span
      aria-hidden
      className={`grid shrink-0 place-items-center overflow-hidden bg-surface-2 ${
        tile === "liked" ? "text-fg" : "text-muted"
      }`}
      style={{ width: size, height: size, borderRadius: size >= 48 ? 8 : 6 }}
    >
      {url && url !== failed ? (
        <img src={url} alt="" className="h-full w-full object-cover" draggable={false} onError={() => setFailed(url)} />
      ) : tile === "liked" ? (
        <HeartGlyph />
      ) : (
        <LibraryGlyph size={Math.round(size / 2.3)} />
      )}
    </span>
  );
}

/** The scope reconnect: an in-place re-consent (start_connect works while
 * connected and replaces the grant). Neutral ghost button, the
 * SpotifyConnectButton recipe. */
export function ReconnectButton() {
  const [waiting, setWaiting] = useState(false);
  return (
    <button
      type="button"
      disabled={waiting}
      onClick={() => {
        setWaiting(true);
        commands.spotifyConnect();
      }}
      className="inline-flex h-[30px] shrink-0 items-center gap-1.5 whitespace-nowrap rounded-lg border border-border/[0.14] bg-fg/[0.05] px-3 text-[12px] font-medium text-fg [transition:background-color_var(--transition-duration-2)_var(--ease-out-tk)] hover:bg-fg/[0.08] disabled:opacity-60"
    >
      {waiting ? "Approve in the browser…" : "Reconnect Spotify"}
    </button>
  );
}
