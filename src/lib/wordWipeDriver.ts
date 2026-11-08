import { wordFillEnd, wordWipeFraction, WORD_ATTACK_MS, type LyricWord } from "./lrc";
import { lineCursor } from "./lineCursor";

/** How long a row stays painted after its last word (DUR5). */
const TAIL_MS = 260;
/** Half the soft edge of the wipe, in px. Wider than the old 5px so a
 * change of pace reads as one moving light, not a stepping edge. Must
 * match WIPE_FEATHER in LyricsPanel. */
export const WIPE_FEATHER_PX = 11;

type Style = Pick<CSSStyleDeclaration, "setProperty" | "removeProperty">;
export interface WordRow {
  index: number;
  words: LyricWord[];
  style: Style;
  spans: Style[];
  /** Each span's laid-out width in px (offsetWidth, so the ink's resting
   * scale doesn't shrink it). Missing in tests: estimated from letters. */
  widths?: number[];
  /** The row element: carries `data-wiping` while the row is active, which
   * is the only time its words are drawn as a clipped gradient (index.css).
   * A resting row is plain text, so it keeps ClearType on Windows. */
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

/** One driver for the panel. Word onsets choose which rows paint; the
 * separate lyric-line clock continues to own scrolling and the highlight.
 * Only active rows visit spans. Inactive rows inherit their normal tone.
 * In-order words share one row cursor (lineCursor.ts); another voice's
 * words keep their own fill. */
export function driveWordRows(
  rows: WordRow[], currentLine: number, leadMs: number,
  clock: WipeClock, frames: FrameScheduler,
): () => void {
  const tracks = rows.filter(r => r.words.length > 0 && r.spans.length === r.words.length)
    .map(row => {
      const widths = row.words.map((w, i) => row.widths?.[i] ?? 10 * Math.max(1, [...w.text].length));
      const cursor = lineCursor(row.words, widths);
      const ends = row.words.map((word, i) => cursor.owns[i] ? cursor.end
        : Math.max(word.end ?? word.t + WORD_ATTACK_MS, wordFillEnd(word, row.words[i + 1]?.t)));
      return { row, widths, cursor, active: false, last: new Array<string>(row.words.length).fill(""),
      start: row.words.reduce((t, w) => Math.min(t, w.t), Infinity),
      end: row.words.reduce((t, w, i) => Math.max(t, ends[i] + TAIL_MS, w.t + 1), -Infinity),
      };
    });
  if (!tracks.length) return () => {};
  let raf = 0;
  let disposed = false;
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
        }
        row.el?.toggleAttribute("data-wiping", active);
        track.active = active;
      }
      if (!active) continue;
      const x = track.cursor.at(p);
      for (let i = 0; i < row.spans.length; i++) {
        // Compare the values CSS receives, not unrounded floating-point
        // progress: only redundant writes vanish.
        // Each word's share of the light, then --wipe as the light's
        // center in the span's own px: -edge is dark, width + edge fully
        // lit, so a word the cursor has passed has no half-lit tail while
        // the row waits through a rest.
        const width = track.widths[i];
        const frac = track.cursor.owns[i]
          ? (width > 0 ? Math.min(1, Math.max(0, (x - track.cursor.offsets[i]) / width)) : x >= track.cursor.offsets[i] ? 1 : 0)
          : wordWipeFraction(row.words[i], pos, leadMs, row.words[i + 1]?.t);
        const edge = WIPE_FEATHER_PX + 1;
        const wipe = `${(-edge + frac * (width + 2 * edge)).toFixed(1)}px`;
        if (wipe !== track.last[i]) {
          track.last[i] = wipe;
          row.spans[i].setProperty("--wipe", wipe);
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
      track.row.el?.toggleAttribute("data-wiping", false);
    }
  };
}
