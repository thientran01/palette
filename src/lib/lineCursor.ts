/**
 * One smooth cursor per lyric row, in letters, that sets when each letter
 * rises (letterRise.ts). Thien (2026-11-09) on the first Rise: "it needs
 * to feel smoother between syllables and letters. it feels too bumpy."
 * Each word used to time its own letters: short words packed theirs into a
 * 90ms attack and then paused, held words stepped on the aligner's
 * checkpoints, and every syllable started its own little burst.
 *
 * The cursor runs through the whole row as if its words sat on one line
 * and passes every word onset exactly when it is sung (the onsets are the
 * accurate part, so they are keyframes). Between keyframes it is a
 * monotone cubic (Fritsch-Carlson): velocity is continuous, it never runs
 * backward, and it never overshoots a keyframe. Inside a phrase it runs
 * straight on from one onset to the next; before a real gap it ends where
 * the word's sound ends and waits there. A word gets at least
 * MIN_LETTER_MS per letter (capped at the next onset), so a 20ms "I"
 * can't snap across.
 *
 * Held notes (with the aligner's letter checkpoints) also key the cursor
 * on those checkpoints, so the letter the singer holds rises slowly,
 * without the final snap of the old per-word fill. Words that aren't sung
 * in order (a backing phrase, or a reply printed after the lead but sung
 * over it) don't join the cursor and keep their own timing.
 *
 * The same cursor drew the wipe in prototype A · Glide.
 * docs/specs/2026-11-08-karaoke-sung-highlight.md
 */
import type { LyricWord } from "./lrc";
import { isHeld } from "./voiceMotion";

/** Floor on how fast the cursor may cross a word, per letter. */
export const MIN_LETTER_MS = 55;
/** A word followed by a silence at least this long ends where its sound
 * ends; a shorter gap is part of the phrase and the cursor runs through. */
export const PHRASE_GAP_MS = 220;

export interface LineCursor {
  /** Left edge of each word along the cursor, in the widths' unit. */
  offsets: readonly number[];
  /** True for words the cursor paints; false for out-of-order words. */
  owns: readonly boolean[];
  /** Song-clock instant the cursor reaches the row's end. */
  end: number;
  /** Cursor position at a song-clock instant (already led). */
  at(clock: number): number;
}

function letters(text: string): number {
  return Math.max(1, [...text.trim()].length);
}

/** Last instant a word's sound lasts, never before its onset. */
function soundEnd(w: LyricWord): number {
  return Math.max(w.end ?? w.t, w.t + 1);
}

export function lineCursor(words: readonly LyricWord[], widths: readonly number[]): LineCursor {
  const offsets: number[] = [];
  let total = 0;
  for (let i = 0; i < words.length; i++) {
    offsets.push(total);
    total += Math.max(0, widths[i] ?? 0);
  }
  // In-order words only: an onset earlier than the last owned word's means
  // another voice, which keeps its own fill.
  const owns = words.map(() => false);
  let lastT = -Infinity;
  words.forEach((w, i) => {
    if (w.timing === "phrase" || w.t < lastT) return;
    owns[i] = true;
    lastT = w.t;
  });
  const seq = words.map((_, i) => i).filter(i => owns[i]);
  const keys: [number, number][] = [];
  seq.forEach((i, k) => {
    const w = words[i];
    const next = k + 1 < seq.length ? words[seq[k + 1]] : undefined;
    const x0 = offsets[i];
    const width = Math.max(0, widths[i] ?? 0);
    const end = soundEnd(w);
    const cap = next ? next.t : Infinity;
    keys.push([w.t, x0]);
    let lastF = 0;
    let lastAt = w.t;
    if (isHeld(w) && w.points && w.points.length >= 2) {
      // The aligner emits a letter for a frame and blanks through its
      // hold, so the letters before a checkpoint fill until the NEXT
      // letter starts: key each level where its plateau ends, the same
      // reading as lrc.ts sampleFill. Then pace, so no step snaps.
      const n = letters(w.text);
      const pts = w.points;
      for (let j = 0; j < pts.length; j++) {
        const f = pts[j].fraction;
        if (!(f > lastF + 1e-6) || f >= 1) continue;
        if (j + 1 < pts.length && pts[j + 1].fraction === f) continue;
        const t = Math.min(cap - 1, Math.max(pts[j].t, lastAt + MIN_LETTER_MS * (f - lastF) * n));
        keys.push([t, x0 + f * width]);
        lastF = f;
        lastAt = t;
      }
    }
    const gap = next ? next.t - end : Infinity;
    if (gap >= PHRASE_GAP_MS) {
      const paced = Math.min(lastAt + MIN_LETTER_MS * (1 - lastF) * letters(w.text), cap);
      keys.push([Math.max(end, paced), x0 + width]);
    }
  });
  if (!keys.length) {
    return { offsets, owns, end: -Infinity, at: () => -Infinity };
  }
  // The last owned word may not be the rightmost (an out-of-order word
  // printed after it): the cursor still finishes its own words only.
  keys.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const pts: [number, number][] = [];
  for (const [t, x] of keys) {
    const prev = pts[pts.length - 1];
    if (prev && t <= prev[0]) {
      // Same instant: keep the farther position, which the earlier key
      // reaches at that instant anyway.
      prev[1] = Math.max(prev[1], x);
      continue;
    }
    pts.push([t, Math.max(x, prev ? prev[1] : -Infinity)]);
  }
  const n = pts.length;
  const slopes = new Array<number>(n).fill(0);
  const secant: number[] = [];
  for (let i = 0; i < n - 1; i++) secant.push((pts[i + 1][1] - pts[i][1]) / (pts[i + 1][0] - pts[i][0]));
  for (let i = 1; i < n - 1; i++) {
    slopes[i] = secant[i - 1] * secant[i] <= 0 ? 0 : (secant[i - 1] + secant[i]) / 2;
  }
  // Rest at both ends: the cursor eases out of the first onset's wait and
  // into the last word's end.
  for (let i = 0; i < n - 1; i++) {
    if (secant[i] === 0) {
      slopes[i] = 0;
      slopes[i + 1] = 0;
      continue;
    }
    const a = slopes[i] / secant[i];
    const b = slopes[i + 1] / secant[i];
    const s = a * a + b * b;
    if (s > 9) {
      const k = 3 / Math.sqrt(s);
      slopes[i] = k * a * secant[i];
      slopes[i + 1] = k * b * secant[i];
    }
  }
  const first = pts[0];
  const last = pts[n - 1];
  return {
    offsets,
    owns,
    end: last[0],
    at(clock) {
      if (clock < first[0]) return -Infinity;
      if (clock >= last[0]) return last[1];
      let lo = 0;
      let hi = n - 1;
      while (hi - lo > 1) {
        const mid = (lo + hi) >> 1;
        if (pts[mid][0] <= clock) lo = mid;
        else hi = mid;
      }
      const [t0, x0] = pts[lo];
      const [t1, x1] = pts[lo + 1];
      const h = t1 - t0;
      const u = (clock - t0) / h;
      const u2 = u * u;
      const u3 = u2 * u;
      return (2 * u3 - 3 * u2 + 1) * x0 + (u3 - 2 * u2 + u) * h * slopes[lo]
        + (-2 * u3 + 3 * u2) * x1 + (u3 - u2) * h * slopes[lo + 1];
    },
  };
}
