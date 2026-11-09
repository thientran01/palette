import type { SavedSync, SyncCheck } from "./backend";
import type { LyricLine } from "./lrc";

/** Estimated words on time as a whole percent. Never shows 100: the
 * estimate tops out at 99.5% (sure words on a song the check never
 * doubted, JamendoLyrics). */
export function onTimePercent(check: SyncCheck): number {
  return Math.min(99, Math.round(check.on_time * 100));
}

/** One sentence on what the review and its second listen found. */
export function checkSummary(check: SyncCheck): string {
  const lines = (n: number) => `${n} ${n === 1 ? "line" : "lines"}`;
  if (check.reheard > 0) {
    const fixed = `A second listen on isolated vocals fixed ${check.fixed} of ${lines(check.reheard)} it doubted.`;
    return check.unsure > 0 ? `${fixed} ${check.unsure} may still be off.` : fixed;
  }
  if (check.unsure > 0) return `${lines(check.unsure)} may be off.`;
  return "Every line passed the check.";
}

/** "today", "yesterday", "3 days ago", then a short date. */
export function savedWhen(savedAt: number, now = Date.now()): string {
  const day = (ms: number) => { const d = new Date(ms); d.setHours(0, 0, 0, 0); return d.getTime(); };
  const days = Math.round((day(now) - day(savedAt * 1000)) / 86_400_000);
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  if (days < 7) return `${days} days ago`;
  return new Date(savedAt * 1000).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export type SyncSort = "recent" | "least";

/** Recent keeps the library's order (newest first). Least sure puts the
 * lowest estimate first; unchecked songs go last, newest first. */
export function sortSyncs(rows: SavedSync[], sort: SyncSort): SavedSync[] {
  if (sort === "recent") return rows;
  return [...rows].sort((a, b) => {
    if (!a.check || !b.check) return Number(!a.check) - Number(!b.check) || b.saved_at - a.saved_at;
    return a.check.on_time - b.check.on_time || b.saved_at - a.saved_at;
  });
}

/** Sung lines as the sync check counts them (sync_library::check): no
 * instrumental-break rows, no blank stamps, one row per stamp. */
export function lyricLineCount(lines: LyricLine[]): number {
  return new Set(lines.filter(l => l.end === undefined && l.text.trim()).map(l => l.t)).size;
}
