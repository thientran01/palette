/**
 * Fixing one karaoke line by hand (karaoke/fixes.rs keeps the fix).
 *
 * The aligner gets most lines right, but now and then one is plainly wrong
 * and the listener can see it at a glance. From the right-click menu they
 * either tap the line in or have it light whole on its stamp.
 *
 * Tapping: Palette jumps back a little before the line (where the player
 * can seek) and the listener presses as each word starts, holding through
 * long notes: a key or mouse press is a word's start, its release the
 * word's end. One press per word (words are what the line's spaces
 * separate, so a Korean or Japanese line is tapped by phrase, not by
 * syllable). A line too quick to tap takes one long press from its first
 * word to its last, and its words spread over it by their letters.
 *
 * This module holds the state of the one line being tapped in, per window,
 * and the pure arithmetic from presses to word times. LyricsPanel listens
 * for the presses and draws it.
 *
 * docs/specs/2026-11-13-karaoke-line-fixes.md
 */
import type { NowPlaying } from "../types";
import type { LyricLine } from "./lrc";
import { commands } from "./backend";

/** One press on the song's timeline, ms. */
export type Press = { down: number; up: number };
/** A word as tapped, the way karaoke_fix_line takes it. */
export type TappedWord = { t: number; end: number; text: string };
export type SongRef = Pick<NowPlaying, "artist" | "title" | "album" | "duration_ms">;

/** How far before the line's stamp a retime starts playing. Stamps run up
 * to a couple of seconds late on the lines that go wrong
 * (karaoke-bad-lines), and the listener needs a beat to get ready. */
export const PRE_ROLL_MS = 3_000;
/** A pause this long after a release ends the tapping. */
export const SETTLE_MS = 1_200;

/** The line's words as tapped: split on its spaces, each keeping the
 * spaces after it, so they join back into the line's text exactly
 * (attachWords and the letter spans rely on that). */
export function lineWords(text: string): string[] {
  return text.match(/\S+\s*/g) ?? [];
}

/** Letters that take time to sing: spaces and punctuation don't. */
function weight(text: string): number {
  return Math.max([...text.replace(/[\s\p{P}]/gu, "")].length, 1);
}

/** `texts` spread across [from, to] by their letters, end to end. */
function spread(texts: string[], from: number, to: number): TappedWord[] {
  const total = texts.reduce((n, t) => n + weight(t), 0);
  let done = 0;
  return texts.map((text) => {
    const t = from + ((to - from) * done) / total;
    done += weight(text);
    const end = from + ((to - from) * done) / total;
    return { t: Math.round(t), end: Math.round(end), text };
  });
}

/** Word times from the presses, or null when they don't fit the line: one
 * press per word, or one press over the whole line. */
export function tappedWords(text: string, presses: Press[]): TappedWord[] | null {
  const words = lineWords(text);
  if (!words.length || !presses.length) return null;
  if (presses.some((p) => !(p.up >= p.down))) return null;
  if (presses.length === words.length) {
    return words.map((w, i) => ({
      t: Math.round(presses[i].down),
      end: Math.round(presses[i].up),
      text: w,
    }));
  }
  if (presses.length === 1) return spread(words, presses[0].down, presses[0].up);
  return null;
}

/** What the caption under the lyrics says while a line is being fixed. */
export type RetimeNote = "ready" | "count" | "missed" | "saved" | "silent" | "failed";

export type Retime = {
  song: SongRef;
  line: { t: number; text: string };
  words: number;
  presses: Press[];
  /** Song time of the press still held. */
  down: number | null;
  /** The player can seek: a miss replays the line. */
  seekable: boolean;
  /** Bumped on each replay, so the panel restarts its wait. */
  take: number;
};

type State = { retime: Retime | null; note: RetimeNote | null };
let state: State = { retime: null, note: null };
const subs = new Set<() => void>();
let noteTimer: ReturnType<typeof setTimeout> | undefined;

function set(next: State) {
  state = next;
  subs.forEach((cb) => cb());
}

export function getRetime(): State {
  return state;
}
export function subscribeRetime(cb: () => void): () => void {
  subs.add(cb);
  return () => {
    subs.delete(cb);
  };
}

/** A note that clears itself, with no line being tapped. */
function finish(note: RetimeNote | null) {
  clearTimeout(noteTimer);
  set({ retime: null, note });
  if (note) noteTimer = setTimeout(() => set({ ...state, note: null }), 1_800);
}

function rewind(r: Retime) {
  if (r.seekable) commands.seekAbs(Math.max(r.line.t - PRE_ROLL_MS, 0));
}

/** Start tapping `line` in. */
export function startRetime(song: SongRef, line: LyricLine, seekable: boolean) {
  clearTimeout(noteTimer);
  const r: Retime = {
    song,
    line: { t: line.t, text: line.text },
    words: lineWords(line.text).length,
    presses: [],
    down: null,
    seekable,
    take: 0,
  };
  set({ retime: r, note: "ready" });
  rewind(r);
}

export function cancelRetime() {
  if (state.retime) finish(null);
}

export function pressDown(at: number) {
  const r = state.retime;
  if (!r || r.down !== null) return;
  set({ retime: { ...r, down: at }, note: "count" });
}

export function pressUp(at: number) {
  const r = state.retime;
  if (!r || r.down === null) return;
  const presses = [...r.presses, { down: r.down, up: Math.max(at, r.down) }];
  const next = { ...r, presses, down: null };
  set({ retime: next, note: "count" });
  if (presses.length === r.words) void commit(next);
}

/** The pause after the last release: one long press spreads the line, a
 * press short of the words is a miss and replays. */
export function settle() {
  const r = state.retime;
  if (!r || r.down !== null || !r.presses.length) return;
  if (r.presses.length === 1 || r.presses.length === r.words) {
    void commit(r);
    return;
  }
  replay("missed");
}

/** No press at all by the time the line was long gone. */
export function nothingHeard() {
  const r = state.retime;
  if (!r || r.presses.length || r.down !== null) return;
  finish("silent");
}

function replay(note: RetimeNote) {
  const r = state.retime;
  if (!r) return;
  if (!r.seekable) {
    finish(note);
    return;
  }
  const next = { ...r, presses: [], down: null, take: r.take + 1 };
  set({ retime: next, note });
  rewind(next);
}

async function commit(r: Retime) {
  const words = tappedWords(r.line.text, r.presses);
  if (!words) {
    replay("missed");
    return;
  }
  // The line is let go at once: the saved fix arrives as a karaoke-ready.
  finish("saved");
  try {
    await commands.karaokeFixLine(r.song, r.line.t, r.line.text, words);
  } catch {
    finish("failed");
  }
}

/** Light the line whole on its stamp. */
export async function plainLine(song: SongRef, line: LyricLine) {
  cancelRetime();
  try {
    await commands.karaokeFixLine(song, line.t, line.text, []);
  } catch {
    finish("failed");
  }
}

/** Back to the aligner's timing for the line. */
export async function undoFix(song: SongRef, line: LyricLine) {
  cancelRetime();
  try {
    await commands.karaokeUnfixLine(song, line.t, line.text);
  } catch {
    finish("failed");
  }
}

export function retimeCaption(note: RetimeNote, r: Retime | null): string {
  switch (note) {
    case "ready":
      return r && !r.seekable
        ? "Tap each word when the line plays"
        : "Tap each word as it's sung";
    case "count":
      return r ? `${Math.min(r.presses.length + (r.down !== null ? 1 : 0), r.words)} of ${r.words}` : "";
    case "missed":
      return r ? "Missed a word. Try again" : "Missed a word";
    case "saved":
      return "Line fixed";
    case "silent":
      return "No taps heard";
    case "failed":
      return "Couldn't save the fix";
  }
}

/** The song has a word sync to lay a fix over. */
export function hasWordSync(lines: LyricLine[]): boolean {
  return lines.some((l) => l.words?.length || l.alignmentFallback || l.fixed);
}
