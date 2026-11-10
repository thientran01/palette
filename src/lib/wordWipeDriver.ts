import type { LyricWord } from "./lrc";
import { FILL_LEAD_MS, letterFill, rowLetterSpans, type LetterSpan } from "./letterFill";

/** How long a row stays sung after its last letter fills. */
const TAIL_MS = 400;

type Style = Pick<CSSStyleDeclaration, "setProperty" | "removeProperty">;
export interface WordRow {
  index: number;
  words: LyricWord[];
  style: Style;
  /** Each word's letter spans, in order (letterFill.ts letterSegments). */
  letters: Style[][];
  /** The row element: carries `data-singing` while the row is active. */
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
  span: LetterSpan;
  lit: string;
}

/** A running driver: call it to stop. `setCurrentLine` hands it the next
 * line without rebuilding (a rebuild re-queried every letter span and
 * re-derived every row's spans on each line change: a 5-12ms hitch on
 * real songs, landing on the frame the line transitions start). */
export type WordDriver = (() => void) & { setCurrentLine(line: number): void };

/** Letter spans per row, by the row's words array: a pure function of it,
 * and the arrays live as long as their lyrics (attachWords makes new ones
 * when a sync lands), so a driver rebuilt for a word-lead nudge reuses them. */
const spanCache = new WeakMap<readonly LyricWord[], LetterSpan[][]>();
function cachedSpans(words: readonly LyricWord[]): LetterSpan[][] {
  let spans = spanCache.get(words);
  if (!spans) spanCache.set(words, spans = rowLetterSpans(words));
  return spans;
}

/** One driver for the panel. Letter spans choose which rows sing; the
 * separate lyric-line clock continues to own scrolling and the highlight.
 * Only active rows visit letters. Inactive rows inherit their normal tone.
 * Every letter keeps its own timing, so a backing voice printed after the
 * lead but sung over it fills on its own. */
export function driveWordRows(
  rows: WordRow[], currentLine: number, leadMs: number,
  clock: WipeClock, frames: FrameScheduler,
): WordDriver {
  const tracks = rows.filter(r => r.words.length > 0 && r.letters.length === r.words.length)
    .map(row => {
      const letters: Letter[] = [];
      const rowSpans = cachedSpans(row.words);
      row.words.forEach((_, wi) => {
        const spans = rowSpans[wi];
        const styles = row.letters[wi];
        if (styles.length !== spans.length) return;
        styles.forEach((style, k) => letters.push({ style, span: spans[k], lit: "" }));
      });
      return { row, letters, active: false,
        start: Math.min(...row.words.map(w => w.t)) - FILL_LEAD_MS,
        end: Math.max(...letters.map(l => l.span.off),
          ...row.words.map(w => (w.end ?? w.t) + 1)) + TAIL_MS,
      };
    });
  if (!tracks.length) return Object.assign(() => {}, { setCurrentLine: () => {} });
  let raf = 0;
  let disposed = false;
  let current = currentLine;
  const clear = (letter: Letter) => {
    if (letter.lit) letter.style.removeProperty("--lit");
    letter.lit = "";
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
        track.active = active;
      }
      if (!active) continue;
      for (const letter of track.letters) {
        // Compare the values CSS receives, not unrounded progress: only
        // redundant writes vanish.
        const e = letterFill(letter.span, p);
        const lit = e.toFixed(3);
        if (lit !== letter.lit) {
          letter.lit = lit;
          letter.style.setProperty("--lit", lit);
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
