/*
 * Pure helpers for the search window's Library and History sections
 * (Search.tsx): local filtering over the cached library, the History day
 * grouping, and the right-hand labels. No backend, no React, so they are
 * unit-tested directly (searchLibrary.test.ts).
 */
import type { HistoryEntry, PlaylistSummary, QueueTrack, SearchSection } from "../types";

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
  return s.normalize("NFKC").toLowerCase().trim();
}

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
export function dayLabel(ms: number, now: number): string {
  const days = Math.round((dayStart(now) - dayStart(ms)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  const d = new Date(ms);
  const sameYear = d.getFullYear() === new Date(now).getFullYear();
  return d.toLocaleDateString(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  });
}

/** "5:58 PM" in the user's locale. */
export function playTime(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

/** A history entry with the day header it opens (null = same day as the
 * row above). Entries arrive newest first. */
export function groupByDay(
  entries: readonly HistoryEntry[],
  now: number,
): { entry: HistoryEntry; header: string | null }[] {
  let last = "";
  return entries.map((entry) => {
    const label = dayLabel(entry.started_at_ms, now);
    const header = label === last ? null : label;
    last = label;
    return { entry, header };
  });
}

/** The Liked Songs playback context for an account. */
export function likedContext(userId: string): string {
  return `spotify:user:${userId}:collection`;
}
