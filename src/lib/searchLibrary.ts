/*
 * Pure helpers for the search window's Library and History sections
 * (Search.tsx): local filtering over the cached library, the History day
 * grouping, and the right-hand labels. No backend, no React, so they are
 * unit-tested directly (searchLibrary.test.ts).
 */
import type { HistoryEntry, NowContext, PlaylistSummary, QueueTrack, SearchSection } from "../types";

/** Tab order of the section strip. */
export const SECTIONS: readonly SearchSection[] = ["songs", "library", "history"];

/** The next section for Tab (dir 1) or Shift+Tab (dir -1), wrapping. */
export function nextSection(cur: SearchSection, dir: 1 | -1): SearchSection {
  const i = SECTIONS.indexOf(cur);
  return SECTIONS[(i + dir + SECTIONS.length) % SECTIONS.length];
}

/** Case- and width-insensitive form for matching (NFKC folds full-width
 * Latin and compatibility Hangul; lowercasing leaves Hangul untouched). */
export function norm(s: string): string {
  let v = normCache.get(s);
  if (v === undefined) {
    v = s.normalize("NFKC").toLowerCase().trim();
    if (normCache.size >= NORM_CACHE_MAX) normCache.clear();
    normCache.set(s, v);
  }
  return v;
}
/** norm() by input string. A History filter matches every play's title and
 * artist on each keystroke, and a long log repeats the same few thousand
 * strings, so NFKC ran ~26,000 times a keystroke for ~2,500 distinct
 * strings. Cleared wholesale at the cap (cheap, and refills in one pass). */
const normCache = new Map<string, string>();
const NORM_CACHE_MAX = 20_000;

/** Match rank for `q` in `text`: 0 = starts with it, 1 = a word starts with
 * it, 2 = contains it, null = no match. `q` must already be normalized. */
export function matchRank(text: string, q: string): number | null {
  if (!q) return 0;
  const t = norm(text);
  if (t.startsWith(q)) return 0;
  const i = t.indexOf(q);
  if (i === -1) return null;
  return /[\s\-(/·,&]/.test(t[i - 1]) ? 1 : 2;
}

/** `items` that match `q`, best rank first, stable within a rank. */
export function filterRanked<T>(items: readonly T[], q: string, text: (t: T) => string[]): T[] {
  const nq = norm(q);
  const scored: { t: T; r: number; i: number }[] = [];
  items.forEach((t, i) => {
    let best: number | null = null;
    for (const s of text(t)) {
      const r = matchRank(s, nq);
      if (r !== null && (best === null || r < best)) best = r;
    }
    if (best !== null) scored.push({ t, r: best, i });
  });
  scored.sort((a, b) => a.r - b.r || a.i - b.i);
  return scored.map((s) => s.t);
}

/** A song found in the cached library, with where it came from. */
export interface LibrarySong {
  track: QueueTrack;
  /** "Liked Songs" or the playlist's name. */
  from: string;
}

/** Library songs matching `q` (title or artist), one per uri, Liked Songs
 * first since that list is the deliberate one. */
export function matchSongs(
  liked: readonly QueueTrack[],
  playlists: readonly { name: string; tracks: readonly QueueTrack[] }[],
  q: string,
  limit: number,
): LibrarySong[] {
  const all: LibrarySong[] = [];
  const seen = new Set<string>();
  const add = (track: QueueTrack, from: string) => {
    if (seen.has(track.uri)) return;
    seen.add(track.uri);
    all.push({ track, from });
  };
  liked.forEach((t) => add(t, "Liked Songs"));
  playlists.forEach((p) => p.tracks.forEach((t) => add(t, p.name)));
  return filterRanked(all, q, (s) => [s.track.title, s.track.artist]).slice(0, limit);
}

/** "53 songs" / "1 song", or "" when Spotify gave no count. */
export function songCount(n: number | null | undefined): string {
  if (n === null || n === undefined) return "";
  return `${n} song${n === 1 ? "" : "s"}`;
}

/** Library row subtitle: the count for the user's own playlists, owner
 * first for everyone else's ("by andy · 88 songs"). */
export function playlistSubtitle(p: PlaylistSummary, ownedByMe: boolean): string {
  const count = songCount(p.total);
  if (ownedByMe) return count;
  return count ? `by ${p.owner} · ${count}` : `by ${p.owner}`;
}

/** m:ss for a song's length; "" for 0 / missing. */
export function fmtDuration(ms: number): string {
  if (!ms || ms < 0) return "";
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Local-midnight epoch ms of the day containing `ms`. */
function dayStart(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** The History day header: "Today", "Yesterday", then "Mon, Nov 3" (with
 * the year once it isn't this year). */
// toLocale*String builds a new Intl formatter on every call, which is slow
// across hundreds of history rows; these are built once and reused.
const DAY_FMT = new Intl.DateTimeFormat(undefined, { weekday: "short", month: "short", day: "numeric" });
const DAY_YEAR_FMT = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  month: "short",
  day: "numeric",
  year: "numeric",
});
const TIME_FMT = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });

export function dayLabel(ms: number, now: number): string {
  const days = Math.round((dayStart(now) - dayStart(ms)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  const sameYear = new Date(ms).getFullYear() === new Date(now).getFullYear();
  return (sameYear ? DAY_FMT : DAY_YEAR_FMT).format(ms);
}

/** "5:58 PM" in the user's locale. Memoized by minute (all the label
 * shows): a History filter rebuilds every matching row's label on each
 * keystroke. */
export function playTime(ms: number): string {
  const minute = Math.floor(ms / 60_000);
  let v = timeCache.get(minute);
  if (v === undefined) {
    v = TIME_FMT.format(ms);
    if (timeCache.size >= TIME_CACHE_MAX) timeCache.clear();
    timeCache.set(minute, v);
  }
  return v;
}
const timeCache = new Map<number, string>();
const TIME_CACHE_MAX = 20_000;

/** dayLabel for many timestamps: entries come grouped by day, so the label
 * is worked out once per day instead of once per row (building Dates and an
 * Intl format per row was most of a 12,900-row History rebuild). */
function dayLabeler(now: number): (ms: number) => string {
  let start = Infinity;
  let end = -Infinity;
  let label = "";
  return (ms) => {
    if (ms < start || ms >= end) {
      start = dayStart(ms);
      const next = new Date(start);
      next.setDate(next.getDate() + 1);
      end = next.getTime();
      label = dayLabel(ms, now);
    }
    return label;
  };
}

/** A history entry with the day header it opens (null = same day as the
 * row above). Entries arrive newest first. */
export function groupByDay(
  entries: readonly HistoryEntry[],
  now: number,
): { entry: HistoryEntry; header: string | null }[] {
  let last = "";
  const labelOf = dayLabeler(now);
  return entries.map((entry) => {
    const label = labelOf(entry.started_at_ms);
    const header = label === last ? null : label;
    last = label;
    return { entry, header };
  });
}

/** The rows of `all` (the whole log, newest first) older than `loaded`'s
 * last row, or null when that row isn't in `all` (the log moved under us).
 * Both lists come from history_page in the same order, so the tail after
 * the last loaded row is exactly what further pages would return. */
export function restOfLog(
  all: readonly HistoryEntry[],
  loaded: readonly HistoryEntry[],
): HistoryEntry[] | null {
  const last = loaded[loaded.length - 1];
  if (!last) return null;
  for (let i = all.length - 1; i >= 0; i--) {
    const e = all[i];
    if (e.key === last.key && e.started_at_ms === last.started_at_ms) return all.slice(i + 1);
  }
  return null;
}

/** Plays and listening time per History day header, keyed by its label.
 * `complete` false (more pages to load) drops the oldest day: it may be
 * cut off at the page edge, and a short count would be wrong. */
export function dayTotals(
  entries: readonly HistoryEntry[],
  now: number,
  complete: boolean,
): Map<string, { count: number; ms: number }> {
  const totals = new Map<string, { count: number; ms: number }>();
  let last = "";
  const labelOf = dayLabeler(now);
  for (const e of entries) {
    last = labelOf(e.started_at_ms);
    const t = totals.get(last) ?? { count: 0, ms: 0 };
    t.count += 1;
    t.ms += Math.max(0, e.ms_listened);
    totals.set(last, t);
  }
  if (!complete && last) totals.delete(last);
  return totals;
}

/** "14 songs · 52 min listened", "31 songs · 2 hr 5 min listened". The
 * word "listened" is load-bearing: a bare time read as the day's total
 * song length, not time actually heard (Thien, 2026-11-06). */
export function daySummary(count: number, ms: number): string {
  const min = Math.max(1, Math.round(ms / 60_000));
  const time = min < 60 ? `${min} min` : `${Math.floor(min / 60)} hr${min % 60 ? ` ${min % 60} min` : ""}`;
  return `${count} song${count === 1 ? "" : "s"} · ${time} listened`;
}

/** The Liked Songs playback context for an account. */
export function likedContext(userId: string): string {
  return `spotify:user:${userId}:collection`;
}

/** Recently played first (Thien's pick, 2026-11-06, like Spotify's
 * Recents): what's playing now, then the recently-played log's order, then
 * everything never played in that window A–Z. The library loads once per
 * session, so the live context keeps the top slot honest between loads. */
export function sortRecent(
  playlists: PlaylistSummary[],
  recent: readonly string[],
  now: NowContext | null,
): PlaylistSummary[] {
  const order = now?.context_uri ? [now.context_uri, ...recent] : [...recent];
  const rank = new Map<string, number>();
  order.forEach((uri, i) => {
    if (!rank.has(uri)) rank.set(uri, i);
  });
  return [...playlists].sort((a, b) => {
    const ra = rank.get(a.uri);
    const rb = rank.get(b.uri);
    if (ra !== undefined || rb !== undefined) {
      if (ra === undefined) return 1;
      if (rb === undefined) return -1;
      return ra - rb;
    }
    return a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true });
  });
}
