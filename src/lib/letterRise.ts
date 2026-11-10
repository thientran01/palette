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
 * Each rise takes RISE_MS and eases in and out (no jolt when it starts),
 * so neighbouring rises overlap into one smooth wave. It is quick and
 * centred near the onset: a letter is half up 20ms after it is sung and
 * fully up 120ms after (Thien, 2026-11-09: a rise that took the whole
 * gap, or a held letter's whole hold, "doesn't happen fast enough and it
 * looks weird relative to the beat or the actual singing"). Word onsets
 * are exactly as measured; only the motion between them is smoothed.
 *
 * English and other Latin-script words rise a syllable at a time instead
 * (syllableLetterOnsets). A Hangul block or a kana is one sung sound, so
 * a rise per character matches the singing; an English letter is not a
 * sound, and timing letters evenly made "away," rise a-w-a-y-comma while
 * the singer held "way", with the same word moving differently on every
 * repeat (Thien, 2026-11-10).
 *
 * docs/specs/2026-11-08-karaoke-sung-highlight.md
 */
import { validWordPoints, wordWipeFraction, type LyricWord } from "./lrc";
import { lineCursor } from "./lineCursor";
import { isLatinWord, syllableStarts } from "./syllables";

/** How long a letter takes to rise. Long enough that neighbouring letters
 * overlap, short enough to land with the singing. */
export const RISE_MS = 200;
/** A rise starts this much before its letter's onset, so the letter is
 * visibly on its way as it is sung, not just after. */
export const RISE_EARLY_MS = 80;
/** Letters of one syllable rise this far apart: one sound, a slight wave. */
export const SYLLABLE_LAG_MS = 16;
/** A syllable never starts sooner than this after the one before it. */
const MIN_SYLLABLE_MS = 40;
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

const silent = (g: string) => !/[\p{L}\p{N}'’]/u.test(g);

/**
 * When the aligner heard each letter of an ASCII word: the start of the
 * letter's CTC span, which acoustic.rs spelling_points stores as the last
 * point at the letter's fraction (the earlier one ends the letter before).
 * Undefined where the word has no checkpoints or the letter has none
 * (punctuation).
 */
function heardLetters(w: LyricWord, letters: readonly string[]): (number | undefined)[] {
  const out: (number | undefined)[] = letters.map(() => undefined);
  const end = w.end;
  const points = end === undefined ? undefined : validWordPoints(w, end);
  if (!points || !/^[\x00-\x7f]*$/.test(w.text)) return out;
  const width = w.text.trimEnd().length;
  let offset = 0;
  letters.forEach((g, k) => {
    offset = w.text.indexOf(g, offset);
    if (offset < 0 || silent(g)) return;
    const f = k === 0 ? 0 : offset / width;
    let t: number | undefined;
    for (const p of points) if (Math.abs(p.fraction - f) < 1e-6) t = p.t;
    out[k] = t;
    offset += g.length;
  });
  return out;
}

/**
 * Letter onsets for a Latin-script word, one syllable at a time
 * (syllables.ts), the way a Hangul block or a kana rises as one sound.
 * Each syllable starts when the aligner heard its first letter, or, for a
 * word without letter checkpoints, at an even share of the word's own
 * sound (never of the gap after it, so a repeated word looks the same each
 * time). Its letters follow SYLLABLE_LAG_MS apart; punctuation rises with
 * the letter before it. They rise on letterSnap, not letterLit.
 */
export function syllableLetterOnsets(w: LyricWord, nextT?: number): number[] {
  const letters = letterSegments(w.text).filter(s => s.letter).map(s => s.text);
  const starts = [0, ...syllableStarts(letters)];
  const n = starts.length;
  const end = Math.max(w.end ?? Math.min(nextT ?? Infinity, w.t + n * 250), w.t + 1);
  const heard = heardLetters(w, letters);
  const unit = letters.map((_, k) => starts.filter(s => s <= k).length - 1);
  const onset = starts.map((s, u) => {
    if (u === 0) return w.t;
    for (let k = s; k < letters.length && unit[k] === u; k++) if (heard[k] !== undefined) return heard[k]!;
    return w.t + (end - w.t) * u / n;
  });
  for (let u = 1; u < n; u++) onset[u] = Math.min(end, Math.max(onset[u], onset[u - 1] + MIN_SYLLABLE_MS));
  const out: number[] = [];
  let j = 0;
  letters.forEach((g, k) => {
    const u = unit[k];
    if (k > 0 && unit[k - 1] !== u) j = 0;
    if (k > 0 && silent(g)) { out.push(out[k - 1]); return; }
    const sounded = letters.filter((x, i) => unit[i] === u && !silent(x)).length;
    const span = (u + 1 < n ? onset[u + 1] : end) - onset[u];
    const lag = sounded > 1 ? Math.min(SYLLABLE_LAG_MS, (span * 0.4) / (sounded - 1)) : 0;
    out.push(onset[u] + j++ * lag);
  });
  return out;
}

/** Song-clock onset of every letter in a row, per word. Latin-script
 * words rise a syllable at a time (syllableLetterOnsets). Other in-order
 * words share the row cursor; a word sung out of order (a backing reply)
 * keeps its own fill (lrc.ts wordWipeFraction). Each word's first letter
 * starts exactly at its onset. */
export function rowLetterOnsets(words: readonly LyricWord[]): number[][] {
  const counts = words.map(w => letterCount(w.text));
  const cursor = lineCursor(words, counts);
  return words.map((w, i) => {
    const n = counts[i];
    if (risesBySyllable(w)) return syllableLetterOnsets(w, words[i + 1]?.t);
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
export function letterLit(onset: number, clock: number): number {
  const u = Math.min(1, Math.max(0, (clock - onset + RISE_EARLY_MS) / RISE_MS));
  return u * u * (3 - 2 * u);
}

/** Latin-script words sung in order rise by syllable, on letterSnap. */
export function risesBySyllable(w: LyricWord): boolean {
  return w.timing !== "phrase" && isLatinWord(w.text);
}

/** The rise curve for a word's letters. */
export function riseCurve(w: LyricWord): (onset: number, clock: number) => number {
  return risesBySyllable(w) ? letterSnap : letterLit;
}

/** A syllable's snap: quicker than a letter's rise, and fast first. */
export const SNAP_MS = 130;
export const SNAP_EARLY_MS = 20;

/**
 * How far a Latin letter has risen. Syllables are fewer and further apart
 * than letters, so the soft letter rise read as slow on them; Thien
 * (2026-11-10): syllables "feels right, but it doesn't feel fast enough or
 * punchy enough". Ease-out cubic: most of the move lands as the syllable
 * is sung (half up 15ms after its onset, fully up 110ms after), then
 * settles softly. Same 1.75px drop, so the punch is timing, not distance.
 */
export function letterSnap(onset: number, clock: number): number {
  const u = Math.min(1, Math.max(0, (clock - onset + SNAP_EARLY_MS) / SNAP_MS));
  return 1 - (1 - u) ** 3;
}
