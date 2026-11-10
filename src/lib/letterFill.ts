/**
 * Fill (2026-11-10): letters brighten as the singing passes through them,
 * and nothing moves. Replaces the Rise, which floated each letter up as it
 * was sung. Thien: Rise "doesn't feel like it conveys the singer enough.
 * It just feels flat ... On slower songs it doesn't have that slow impact
 * or that drag on notes. On fast songs it struggles to keep up." A rise
 * marks only the instant a sound starts and is over in a fraction of a
 * second, so a held note sat finished while the singer held it, and fast
 * lines started rises faster than they could finish.
 *
 * Each letter now fills over the time the singing takes to cross it: one
 * smooth cursor per row (lineCursor.ts) passes every word onset exactly,
 * slows through a held note on the aligner's letter checkpoints, and runs
 * straight on inside a phrase. A letter comes half up as the cursor
 * reaches it (starting just before, so a line's first letter doesn't pop)
 * and fills the rest while the cursor crosses it, so it is fully bright the
 * moment the singing leaves it: a word is done exactly as its sound ends,
 * with no flash after (Thien disliked the old Glide's word "flashing at
 * the end"). A held vowel keeps brightening across its whole hold: the
 * drag. Plain colored text, so it keeps ClearType.
 *
 * docs/specs/2026-11-08-karaoke-sung-highlight.md
 */
import { wordWipeFraction, type LyricWord } from "./lrc";
import { lineCursor } from "./lineCursor";

/** A letter comes half up quickly as the singing reaches it (the attack,
 * starting FILL_LEAD_MS early and done ATTACK_MS after), then fills the
 * rest across the time the singing stays on it (the drag). */
export const FILL_LEAD_MS = 60;
export const ATTACK_MS = 60;
export const ATTACK = 0.5;

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

/** When the singing reaches a letter and when it leaves it, song clock. */
export interface LetterSpan { on: number; off: number }

/** Every letter's span in a row, per word. In-order words share the row
 * cursor; a word sung out of order (a backing reply) keeps its own fill
 * (lrc.ts wordWipeFraction). Each word's first letter is reached exactly
 * at its onset. */
export function rowLetterSpans(words: readonly LyricWord[]): LetterSpan[][] {
  const counts = words.map(w => letterCount(w.text));
  const cursor = lineCursor(words, counts);
  return words.map((w, i) => {
    const n = counts[i];
    let at: (x: number) => number;
    if (cursor.owns[i]) {
      const end = Math.max(cursor.end, w.t);
      at = x => reach(t => cursor.at(t), cursor.offsets[i] + x - 1e-6, w.t, end);
    } else {
      const nextT = words[i + 1]?.t;
      const end = Math.max(w.end ?? w.t, nextT ?? w.t, w.t + 1);
      const fill = (t: number) => wordWipeFraction(w, t, 0, nextT) * n;
      at = x => reach(fill, x - 1e-6, w.t, end);
    }
    const marks = Array.from({ length: n + 1 }, (_, k) => (k === 0 ? w.t : at(k)));
    return Array.from({ length: n }, (_, k) => ({ on: marks[k], off: Math.max(marks[k + 1], marks[k] + 1) }));
  });
}

const smooth = (u: number) => {
  const x = Math.min(1, Math.max(0, u));
  return x * x * (3 - 2 * x);
};

/** How bright a letter is, 0 (unsung) to 1 (sung), at a song-clock instant
 * (already led): half from the attack around the moment the singing
 * reaches it, half spread across its span, so a quick letter is simply lit
 * and a held one keeps brightening until the note moves on. */
export function letterFill(span: LetterSpan, clock: number): number {
  const attack = smooth((clock - span.on + FILL_LEAD_MS) / (FILL_LEAD_MS + ATTACK_MS));
  const drag = smooth((clock - span.on) / (span.off - span.on));
  return ATTACK * attack + (1 - ATTACK) * drag;
}
