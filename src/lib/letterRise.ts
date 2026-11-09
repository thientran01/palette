/**
 * C · Rise (2026-11-08, Thien's pick over a continuous wipe). The aligner
 * got accurate enough that the old per-word wipe showed every syllable as
 * its own sprint: 90ms flashes on short words, letter checkpoints that
 * crawled then snapped, a white sheen on each filling word and a halo on
 * each finished one, at one word per ~100ms syllable in Korean. Thien:
 * "it makes the words jump by syllables really fast."
 *
 * Now each letter brightens and floats up into place as it's sung. Letter
 * onsets come from one smooth cursor per row (lineCursor.ts), which passes
 * every word onset exactly and keeps its speed continuous between them, so
 * letters and syllables follow each other evenly instead of in bursts.
 * Each rise lasts at least RISE_MS and eases in and out (no jolt when it
 * starts), so neighbouring rises overlap into one smooth wave, and a held
 * letter rises as slowly as it is held. Word onsets are exactly as
 * measured; only the motion between them is smoothed.
 *
 * docs/specs/2026-11-08-karaoke-sung-highlight.md
 */
import { wordWipeFraction, type LyricWord } from "./lrc";
import { lineCursor } from "./lineCursor";

/** Shortest rise. Long enough that neighbouring letters overlap. */
export const RISE_MS = 300;
/** A rise starts this much before its letter's onset, so the letter is
 * visibly on its way as it is sung, not just after. */
export const RISE_EARLY_MS = 90;
// Intl.Segmenter is in WebView2 and Node; the project's TS lib predates it.
type Graphemes = { segment(text: string): Iterable<{ segment: string }> };
const Segmenter = (Intl as unknown as {
  Segmenter?: new (locale: undefined, options: { granularity: "grapheme" }) => Graphemes;
}).Segmenter;
const segmenter = Segmenter ? new Segmenter(undefined, { granularity: "grapheme" }) : undefined;

/** A word's text as letters (graphemes, so a Hangul block, an accented
 * letter or an emoji is one) and the spaces between them. */
export function letterSegments(text: string): { text: string; letter: boolean }[] {
  const parts = segmenter ? Array.from(segmenter.segment(text), s => s.segment) : [...text];
  return parts.map(part => ({ text: part, letter: part.trim().length > 0 }));
}

export function letterCount(text: string): number {
  return letterSegments(text).filter(s => s.letter).length;
}

/** Earliest instant in [lo, hi] at which monotone `f` reaches `x`. */
function reach(f: (t: number) => number, x: number, lo: number, hi: number): number {
  if (f(lo) >= x) return lo;
  if (!(f(hi) >= x)) return hi;
  while (hi - lo > 0.5) {
    const mid = (lo + hi) / 2;
    if (f(mid) >= x) hi = mid;
    else lo = mid;
  }
  return hi;
}

/** Song-clock onset of every letter in a row, per word. In-order words
 * share the row cursor; a word sung out of order (a backing reply) keeps
 * its own fill (lrc.ts wordWipeFraction). Each word's first letter starts
 * exactly at its onset. */
export function rowLetterOnsets(words: readonly LyricWord[]): number[][] {
  const counts = words.map(w => letterCount(w.text));
  const cursor = lineCursor(words, counts);
  return words.map((w, i) => {
    const n = counts[i];
    if (cursor.owns[i]) {
      const end = Math.max(cursor.end, w.t);
      return Array.from({ length: n }, (_, k) =>
        k === 0 ? w.t : reach(t => cursor.at(t), cursor.offsets[i] + k - 1e-6, w.t, end));
    }
    const nextT = words[i + 1]?.t;
    const end = Math.max(w.end ?? w.t, nextT ?? w.t, w.t + 1);
    const fill = (t: number) => wordWipeFraction(w, t, 0, nextT);
    return Array.from({ length: n }, (_, k) => k === 0 ? w.t : reach(fill, k / n - 1e-6, w.t, end));
  });
}

/** How far a letter has risen, 0 (unsung) to 1 (in place), at a
 * song-clock instant (already led). Smoothstep: no jolt when a letter
 * starts moving and none when it lands. */
export function letterLit(onset: number, nextOnset: number, clock: number): number {
  const duration = Math.max(RISE_MS, nextOnset - onset);
  const u = Math.min(1, Math.max(0, (clock - onset + RISE_EARLY_MS) / duration));
  return u * u * (3 - 2 * u);
}
