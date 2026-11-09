import { WORD_ATTACK_MS, type LyricWord } from "./lrc";
import { letterLit, rowLetterOnsets, RISE_EARLY_MS, RISE_MS } from "./letterRise";
import { heldBend } from "./voiceMotion";

/** How long a row stays sung after its last letter lands: long enough for a
 * held note's sway to settle (voiceMotion.ts release + settle). */
const TAIL_MS = 400;

type Style = Pick<CSSStyleDeclaration, "setProperty" | "removeProperty">;
export interface WordRow {
  index: number;
  words: LyricWord[];
  style: Style;
  /** Each word's letter spans, in order (letterRise.ts letterSegments). */
  letters: Style[][];
  /** The row element: carries `data-singing` while the row is active,
   * which lowers its unsung letters so they can rise (index.css). */
  el?: Pick<Element, "toggleAttribute">;
}
export interface WipeClock {
  now(): number;
  isPlaying(): boolean;
  subscribe(cb: () => void): () => void;
}
export interface FrameScheduler {
  request(cb: () => void): number;
  cancel(id: number): void;
}

const ROW_PROPS = ["--word-bright", "--lyric-scale", "--lyric-opacity", "--lyric-filter"] as const;

interface Letter {
  style: Style;
  word: number;
  /** Index within its word, for the held note's slight wave. */
  k: number;
  onset: number;
  next: number;
  lit: string;
  bend: string;
}

/** One driver for the panel. Letter onsets choose which rows sing; the
 * separate lyric-line clock continues to own scrolling and the highlight.
 * Only active rows visit letters. Inactive rows inherit their normal tone.
 * Every letter keeps its own timing, so a backing voice printed after the
 * lead but sung over it rises on its own. */
export function driveWordRows(
  rows: WordRow[], currentLine: number, leadMs: number,
  clock: WipeClock, frames: FrameScheduler,
): () => void {
  const tracks = rows.filter(r => r.words.length > 0 && r.letters.length === r.words.length)
    .map(row => {
      const letters: Letter[] = [];
      const rowOnsets = rowLetterOnsets(row.words);
      row.words.forEach((w, wi) => {
        const onsets = rowOnsets[wi];
        const spans = row.letters[wi];
        if (spans.length !== onsets.length) return;
        // The last letter takes the rest of the word's sound.
        const end = Math.max(w.end ?? w.t + WORD_ATTACK_MS, onsets[onsets.length - 1] ?? w.t);
        spans.forEach((style, k) => letters.push({ style, word: wi, k, onset: onsets[k],
          next: onsets[k + 1] ?? end, lit: "", bend: "" }));
      });
      return { row, letters, active: false,
        start: Math.min(...row.words.map(w => w.t)) - RISE_EARLY_MS,
        end: Math.max(...letters.map(l => l.onset - RISE_EARLY_MS + Math.max(RISE_MS, l.next - l.onset)),
          ...row.words.map(w => (w.end ?? w.t) + 1)) + TAIL_MS,
      };
    });
  if (!tracks.length) return () => {};
  let raf = 0;
  let disposed = false;
  const clear = (letter: Letter) => {
    if (letter.lit) letter.style.removeProperty("--lit");
    if (letter.bend) letter.style.removeProperty("--bend");
    letter.lit = "";
    letter.bend = "";
  };
  const write = () => {
    const pos = clock.now();
    const p = pos + leadMs;
    for (const track of tracks) {
      const { row } = track;
      const active = row.index === currentLine || (p >= track.start && p < track.end);
      if (active !== track.active) {
        if (active) {
          row.style.setProperty("--word-bright", "rgb(var(--fg))");
          row.style.setProperty("--lyric-scale", "1");
          row.style.setProperty("--lyric-opacity", "1");
          row.style.setProperty("--lyric-filter", "none");
        } else {
          for (const prop of ROW_PROPS) row.style.removeProperty(prop);
          track.letters.forEach(clear);
        }
        row.el?.toggleAttribute("data-singing", active);
        track.active = active;
      }
      if (!active) continue;
      for (const letter of track.letters) {
        // Compare the values CSS receives, not unrounded progress: only
        // redundant writes vanish.
        const e = letterLit(letter.onset, letter.next, p);
        const lit = e.toFixed(3);
        if (lit !== letter.lit) {
          letter.lit = lit;
          letter.style.setProperty("--lit", lit);
        }
        // A held note's letters ride the pitch once they've risen.
        const word = row.words[letter.word];
        const px = e * heldBend(word, row.words[letter.word + 1]?.t, p, letter.k);
        const bend = px === 0 ? "" : `${(-px).toFixed(2)}px`;
        if (bend !== letter.bend) {
          if (bend) letter.style.setProperty("--bend", bend);
          else letter.style.removeProperty("--bend");
          letter.bend = bend;
        }
      }
    }
  };
  const tick = () => {
    raf = 0;
    if (disposed) return;
    write();
    if (clock.isPlaying()) raf = frames.request(tick);
  };
  const kick = () => {
    if (disposed) return;
    write();
    if (clock.isPlaying()) {
      if (!raf) raf = frames.request(tick);
    } else if (raf) {
      frames.cancel(raf);
      raf = 0;
    }
  };
  kick();
  const unsubscribe = clock.subscribe(kick);
  return () => {
    disposed = true;
    unsubscribe();
    if (raf) frames.cancel(raf);
    for (const track of tracks) if (track.active) {
      for (const prop of ROW_PROPS) track.row.style.removeProperty(prop);
      track.letters.forEach(clear);
      track.row.el?.toggleAttribute("data-singing", false);
    }
  };
}
