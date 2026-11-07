/*
 * The credits roll (Thien, 2026-11-07: "the song ends and it just stays at
 * the end of the song. You basically never get to see the full credits").
 * Once the singing is over, the lyric list glides up so the credits' top
 * sits at the panel's top edge, holds a beat, then rolls through them at a
 * steady reading pace like film credits, timed to finish before the song
 * does.
 *
 * Driven by playback position like the lyric follow, never by wall time:
 * pausing freezes it, seeking back before the last line hands control back
 * to the lyrics, and a wheel scroll takes over the way it does for lyrics.
 */
import type { LyricLine } from "./lrc";

/** Beat between arriving at the credits and the roll starting. */
export const ROLL_DWELL_MS = 1500;
/** The roll aims to finish this long before the track ends. */
export const ROLL_TAIL_MS = 2000;
/** Reading pace bounds, px per second. A long outro with short credits
 * rolls at the floor and finishes early; a short outro with long credits
 * rolls at the ceiling and may not reach the end (scrolling still can). */
export const ROLL_MIN_PX_S = 14;
export const ROLL_MAX_PX_S = 40;

/** When the singing ends: the start of the trailing instrumental break if
 * the song has one (parseLrc makes one for an outro), otherwise the last
 * line's last word, otherwise a few seconds into the last line. null when
 * there are no sung lines. */
export function singingEndMs(lines: LyricLine[]): number | null {
  let i = lines.length - 1;
  while (i >= 0 && lines[i].end !== undefined) i--;
  if (i < 0) return null;
  if (i < lines.length - 1) return lines[i + 1].t;
  const last = lines[i];
  const word = last.words?.[last.words.length - 1];
  const wordEnd = word ? (word.end ?? word.t) : 0;
  return Math.max(wordEnd, last.t + 4000);
}

export interface RollPlan {
  /** Playback position at which the roll starts moving. */
  start: number;
  /** Offsets (px down the list) at the credits' top and at their end. */
  from: number;
  to: number;
  /** px per ms. */
  speed: number;
}

export function planRoll(
  singingEnd: number,
  durationMs: number,
  from: number,
  to: number,
): RollPlan {
  const start = singingEnd + ROLL_DWELL_MS;
  const distance = Math.max(to - from, 0);
  const time = durationMs > 0 ? durationMs - ROLL_TAIL_MS - start : 0;
  const wanted = time > 0 ? distance / time : Infinity;
  const speed = Math.min(Math.max(wanted, ROLL_MIN_PX_S / 1000), ROLL_MAX_PX_S / 1000);
  return { start, from, to: Math.max(to, from), speed };
}

/** Offset at a playback position: parked at `from` until the roll starts,
 * then linear, stopping at `to`. */
export function rollOffsetAt(plan: RollPlan, positionMs: number): number {
  const moved = Math.max(positionMs - plan.start, 0) * plan.speed;
  return Math.min(plan.from + moved, plan.to);
}
