/**
 * C · Rise (2026-11-08, Thien's pick over a continuous wipe). The aligner
 * got accurate enough that the old per-word wipe showed every syllable as
 * its own sprint: 90ms flashes on short words, letter checkpoints that
 * crawled then snapped, a white sheen on each filling word and a halo on
 * each finished one, at one word per ~100ms syllable in Korean. Thien:
 * "it makes the words jump by syllables really fast."
 *
 * Now each letter brightens and floats up into place as it's sung. A
 * letter's rise starts at its own onset (the aligner's letter checkpoints
 * where it has them, else spread across the word's sung span) and lasts
 * at least RISE_MS, so rises overlap: a run of quick syllables reads as
 * one wave instead of a strobe, and a held letter rises as slowly as it is
 * held. Onsets are exactly as measured; only how long a letter takes to
 * settle is smoothed.
 *
 * docs/specs/2026-11-08-karaoke-sung-highlight.md
 */
import { wordFillEnd, type LyricWord } from "./lrc";

/** Shortest rise. Long enough that neighbouring syllables overlap. */
export const RISE_MS = 260;
/** A rise starts this much before its letter's onset so the letter is
 * visibly moving as it is sung, not just after. */
export const RISE_EARLY_MS = 30;
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

/** Song-clock onset of each letter of `w`. With acoustic checkpoints, a
 * letter starts where the fill first reaches its share; otherwise letters
 * are spread across the word's displayed fill (lrc.ts wordFillEnd). */
export function letterOnsets(w: LyricWord, nextT?: number): number[] {
  const n = letterCount(w.text);
  if (n === 0) return [];
  const pts = w.points;
  const valid = w.timing !== "phrase" && pts && pts.length >= 2 && pts[0].t === w.t &&
    pts.every((p, i) => i === 0 || (p.t >= pts[i - 1].t && p.fraction >= pts[i - 1].fraction));
  if (valid) {
    return Array.from({ length: n }, (_, k) => {
      const share = k / n;
      return (pts.find(p => p.fraction >= share - 1e-6) ?? pts[pts.length - 1]).t;
    });
  }
  const span = Math.max(wordFillEnd(w, nextT) - w.t, 90);
  return Array.from({ length: n }, (_, k) => w.t + span * k / n);
}

/** How far a letter has risen, 0 (unsung) to 1 (in place), at a
 * song-clock instant (already led). Ease-out: quick to start moving,
 * gentle to land. */
export function letterLit(onset: number, nextOnset: number, clock: number): number {
  const duration = Math.max(RISE_MS, nextOnset - onset);
  const u = Math.min(1, Math.max(0, (clock - onset + RISE_EARLY_MS) / duration));
  return 1 - (1 - u) ** 3;
}
