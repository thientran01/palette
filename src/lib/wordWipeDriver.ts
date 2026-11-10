import type { LyricWord } from "./lrc";
import { riseCurve, rowLetterOnsets, RISE_EARLY_MS, RISE_MS } from "./letterRise";
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
   * which lowers its unsung letters so they can rise (index.css), then
   * `data-settling` while that drop eases back out (settleRows). */
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
  /** letterLit, or letterSnap for a word that rises by syllable. */
  rise: (onset: number, clock: number) => number;
  lit: string;
  bend: string;
}

/** A running driver: call it to stop. `setCurrentLine` hands it the next
 * line without rebuilding (a rebuild re-queried every letter span and
 * re-derived every row's onsets on each line change: a 5-12ms hitch on
 * real songs, landing on the frame the line transitions start). */
export type WordDriver = (() => void) & { setCurrentLine(line: number): void };

/** Letter onsets per row, by the row's words array: a pure function of it,
 * and the arrays live as long as their lyrics (attachWords makes new ones
 * when a sync lands), so a driver rebuilt for a word-lead nudge reuses them. */
const onsetCache = new WeakMap<readonly LyricWord[], number[][]>();
function cachedOnsets(words: readonly LyricWord[]): number[][] {
  let onsets = onsetCache.get(words);
  if (!onsets) onsetCache.set(words, onsets = rowLetterOnsets(words));
  return onsets;
}

/** One driver for the panel. Letter onsets choose which rows sing; the
 * separate lyric-line clock continues to own scrolling and the highlight.
 * Only active rows visit letters. Inactive rows inherit their normal tone.
 * Every letter keeps its own timing, so a backing voice printed after the
 * lead but sung over it rises on its own. */
export function driveWordRows(
  rows: WordRow[], currentLine: number, leadMs: number,
  clock: WipeClock, frames: FrameScheduler,
): WordDriver {
  const tracks = rows.filter(r => r.words.length > 0 && r.letters.length === r.words.length)
    .map(row => {
      const letters: Letter[] = [];
      const rowOnsets = cachedOnsets(row.words);
      row.words.forEach((w, wi) => {
        const onsets = rowOnsets[wi];
        const rise = riseCurve(w);
        const spans = row.letters[wi];
        if (spans.length !== onsets.length) return;
        spans.forEach((style, k) => letters.push({ style, word: wi, k, onset: onsets[k],
          rise, lit: "", bend: "" }));
      });
      return { row, letters, active: false,
        start: Math.min(...row.words.map(w => w.t)) - RISE_EARLY_MS,
        end: Math.max(...letters.map(l => l.onset - RISE_EARLY_MS + RISE_MS),
          ...row.words.map(w => (w.end ?? w.t) + 1)) + TAIL_MS,
      };
    });
  if (!tracks.length) return Object.assign(() => {}, { setCurrentLine: () => {} });
  let raf = 0;
  let disposed = false;
  let current = currentLine;
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
      const active = row.index === current || (p >= track.start && p < track.end);
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
        if (!active) row.el?.toggleAttribute("data-settling", true);
        track.active = active;
      }
      if (!active) continue;
      for (const letter of track.letters) {
        // Compare the values CSS receives, not unrounded progress: only
        // redundant writes vanish.
        const e = letter.rise(letter.onset, p);
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
  const stop = () => {
    disposed = true;
    unsubscribe();
    if (raf) frames.cancel(raf);
    for (const track of tracks) if (track.active) {
      for (const prop of ROW_PROPS) track.row.style.removeProperty(prop);
      track.letters.forEach(clear);
      track.row.el?.toggleAttribute("data-singing", false);
      track.row.el?.toggleAttribute("data-settling", true);
    }
  };
  return Object.assign(stop, {
    setCurrentLine(line: number) {
      if (line === current) return;
      current = line;
      kick();
    },
  });
}

/** Letters carry their rise transform only while their row sings and while
 * its drop eases back out afterwards (index.css). A transform on every
 * letter of every row, even an identity one, gave each letter its own paint
 * chunk: thousands per song, all re-layerized on every frame a karaoke row
 * repainted. When a row's --drop transition ends (its letters are back at
 * rest, identity transforms), this lifts `data-settling` so the letters
 * paint as plain text again. Lives outside driveWordRows because a drop
 * can outlast a driver (one is rebuilt when the lyrics or word lead change). */
export function settleRows(root: HTMLElement): () => void {
  const done = (e: Event) => {
    if ((e as TransitionEvent).propertyName !== "--drop") return;
    const ink = e.target as Element;
    const row = ink.closest?.("[data-settling]");
    if (!row) return;
    // A row that sang again mid-ease starts a fresh drop: wait for that one.
    const running = ink.getAnimations?.().some(a =>
      (a as CSSTransition).transitionProperty === "--drop" && a.playState === "running");
    if (!running) row.removeAttribute("data-settling");
  };
  root.addEventListener("transitionend", done);
  root.addEventListener("transitioncancel", done);
  return () => {
    root.removeEventListener("transitionend", done);
    root.removeEventListener("transitioncancel", done);
  };
}
