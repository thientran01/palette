import { wordWipeFraction, WORD_ATTACK_MS, type LyricWord } from "./lrc";

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
    .map(row => ({ row, active: false, last: new Array<number>(row.words.length).fill(-1),
      halo: new Array<number>(row.words.length).fill(-1),
      start: row.words.reduce((t, w) => Math.min(t, w.t), Infinity),
      end: row.words.reduce((t, w) => Math.max(t, w.end ?? w.t + WORD_ATTACK_MS, w.t + 1), -Infinity),
    }));
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
          row.style.setProperty("--word-peak", "rgb(var(--lyric-peak))");
          row.style.setProperty("--word-halo", "rgb(var(--lyric-peak) / 0.28)");
          row.style.setProperty("--lyric-scale", "1");
          row.style.setProperty("--lyric-blur", "0px");
        } else {
          row.style.removeProperty("--word-bright");
          row.style.removeProperty("--word-peak");
          row.style.removeProperty("--word-halo");
          row.style.removeProperty("--lyric-scale");
          row.style.removeProperty("--lyric-blur");
        }
        track.active = active;
      }
      if (!active) continue;
      for (let i = 0; i < row.spans.length; i++) {
        const frac = wordWipeFraction(row.words[i], pos, leadMs, row.words[i + 1]?.t);
        // A small onset bloom releases over DUR5 (260ms), using the same
        // playback clock and RAF. It freezes on pause and rewinds on seek.
        const elapsed = p - row.words[i].t;
        const halo = elapsed >= 0 && elapsed < 260 ? (1 - elapsed / 260) ** 2 : 0;
        if (halo !== track.halo[i]) {
          track.halo[i] = halo;
          row.spans[i].setProperty("--word-bloom", halo > 0
            ? `0 0 ${(12 * halo).toFixed(2)}px var(--word-halo, transparent)` : "none");
        }
        if (frac === track.last[i]) continue;
        track.last[i] = frac;
        row.spans[i].setProperty("--wipe", `${(frac * 100).toFixed(1)}%`);
        row.spans[i].setProperty("--word-sheen", frac > 0 && frac < 1
          ? "var(--word-peak, currentColor)" : "var(--word-bright, currentColor)");
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
      track.row.style.removeProperty("--word-bright");
      track.row.style.removeProperty("--word-peak");
      track.row.style.removeProperty("--word-halo");
      track.row.style.removeProperty("--lyric-scale");
      track.row.style.removeProperty("--lyric-blur");
    }
  };
}
