import type { LyricWord } from "./lrc";

const PREPARE_MS = 260; // DUR5: get ready before the displayed word onset.
const SETTLE_MS = 140; // DUR2: settle into the vocal, rather than flash off.

/** A vocal-entry cue, not a beat detector. Build once per word set, then
 * sample the same song clock as the wipe. Phrase-estimated words are excluded
 * because their distributed fractions are not measured vocal entrances. */
export function createVocalCue(words: LyricWord[]): (positionMs: number) => number {
  const events = words.filter(w => w.timing !== "phrase" && Number.isFinite(w.t))
    .map(w => ({ t: w.t, end: Number.isFinite(w.end) ? Math.max(w.t, w.end!) : w.t }))
    .sort((a, b) => a.t - b.t);
  let latestEnd = -Infinity;
  const ends = events.map(event => (latestEnd = Math.max(latestEnd, event.end)));
  return position => {
    let lo = 0, hi = events.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      if (events[mid].t <= position) lo = mid + 1;
      else hi = mid;
    }
    const next = events[lo], previous = events[lo - 1];
    const u = next ? Math.max(0, 1 - (next.t - position) / PREPARE_MS) : 0;
    const prepare = u * u * (3 - 2 * u);
    const elapsed = previous ? position - previous.t : Infinity;
    const settle = elapsed < SETTLE_MS ? (1 - elapsed / SETTLE_MS) ** 2 : 0;
    const held = previous && position < ends[lo - 1] ? 0.18 : 0;
    return Math.max(prepare, settle, held);
  };
}
