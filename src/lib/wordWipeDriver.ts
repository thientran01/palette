import { wordWipe, WORD_ATTACK_MS, type LyricWord } from "./lrc";

type Style = Pick<CSSStyleDeclaration, "setProperty" | "removeProperty">;
export interface WordRow {
  index: number;
  words: LyricWord[];
  style: Style;
  spans: Style[];
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

/** One driver for the panel. Word onsets choose which rows paint; the
 * separate lyric-line clock continues to own scrolling and the marker.
 * Only active rows visit spans. Inactive rows inherit their normal tone. */
export function driveWordRows(
  rows: WordRow[], currentLine: number, leadMs: number,
  clock: WipeClock, frames: FrameScheduler,
): () => void {
  const tracks = rows.filter(r => r.words.length > 0 && r.spans.length === r.words.length)
    .map(row => ({ row, active: false, last: new Array<number>(row.words.length).fill(-1) }));
  if (!tracks.length) return () => {};
  let raf = 0;
  let disposed = false;
  const write = () => {
    const pos = clock.now();
    const p = pos + leadMs;
    for (const track of tracks) {
      const { row } = track;
      const lastWord = row.words[row.words.length - 1];
      const end = Math.max(lastWord.end ?? lastWord.t + WORD_ATTACK_MS, lastWord.t + 1);
      const active = row.index === currentLine || (p >= row.words[0].t && p < end);
      if (active !== track.active) {
        if (active) row.style.setProperty("--word-bright", "rgb(var(--fg))");
        else row.style.removeProperty("--word-bright");
        track.active = active;
      }
      if (!active) continue;
      const wipe = wordWipe(row.words, pos, leadMs);
      const cur = wipe?.index ?? -1;
      for (let i = 0; i < row.spans.length; i++) {
        const frac = i < cur ? 1 : i === cur ? (wipe?.frac ?? 0) : 0;
        if (frac === track.last[i]) continue;
        track.last[i] = frac;
        row.spans[i].setProperty("--wipe", `${(frac * 100).toFixed(1)}%`);
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
    for (const track of tracks) if (track.active) track.row.style.removeProperty("--word-bright");
  };
}
