/*
 * The card's one-row lyric line (CardLyricLine in LyricsPanel.tsx). Two
 * rows looked wrong in the small card (Thien, 2026-11-25: "when lyrics are
 * double lines and stuff it looks bad"), so a line that doesn't fit stays
 * on one row and pans with the singing instead: it holds on its start while
 * the opening words are read, slides at the pace of the song, and rests on
 * its end before the next line takes over. The sung position stays inside
 * the row the whole way (the slide finishes by PAN_TO, and a cursor moving
 * at an even pace never outruns a linear pan that starts after it).
 */
import type { LyricLine } from "./lrc";

/** Fraction of the line's time held on its start before the pan begins. */
export const PAN_FROM = 0.2;
/** Fraction of the line's time by which the pan has reached the end. */
export const PAN_TO = 0.8;
/** A line with no word timing and no next line pans over this long. */
export const FALLBACK_LINE_MS = 4_000;

/** The song-clock span the pan maps over: the sung words when the line has
 * word timing, else its stamp up to the next line's. */
export function lineSpan(line: LyricLine, next: LyricLine | undefined): { start: number; end: number } {
  if (line.words?.length) {
    const start = Math.min(...line.words.map((w) => w.t));
    const end = Math.max(...line.words.map((w) => w.end ?? w.t));
    if (end > start) return { start, end };
  }
  const end = next ? next.t : line.t + FALLBACK_LINE_MS;
  return { start: line.t, end: Math.max(end, line.t + 1) };
}

/** How far left to shift the line, in px, at song-clock instant `clock`
 * (already led). 0 for a line that fits. */
export function panOffset(overflowPx: number, span: { start: number; end: number }, clock: number): number {
  if (overflowPx <= 0) return 0;
  const frac = (clock - span.start) / (span.end - span.start);
  const t = Math.min(Math.max((frac - PAN_FROM) / (PAN_TO - PAN_FROM), 0), 1);
  return Math.round(overflowPx * t);
}
