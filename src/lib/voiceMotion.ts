/**
 * Held notes ride the singer's pitch (C · Rise, 2026-11-08). Thien: "I
 * want to emphasize ... emotion or dragged notes or vibrato ... make the
 * karaoke lyrics feel like the singer is really singing."
 *
 * On a held note (HELD_MS or longer) with a voice track, each letter that
 * has risen moves up and down with the sung pitch, relative to where the
 * note started: vibrato becomes a gentle sway, a slide or a dragged note a
 * drift. Later letters read the pitch a little earlier, so the sway runs
 * through the word as a small wave. Short syllables never follow pitch:
 * that would bring back the syllable-rate jitter this work removes.
 *
 * The voice track comes from the karaoke worker (src-tauri/src/voice.rs):
 * the separated vocal's pitch every 10 ms across each held word. Nothing
 * here moves on its own: no voice, no sway; paused, no motion.
 *
 * docs/specs/2026-11-08-karaoke-sung-highlight.md
 */
import type { LyricWord } from "./lrc";

/** Sung at least this long: a held note. Must match voice.rs HELD_MS. */
export const HELD_MS = 450;
/** Frames of a voice track, from the word's onset. */
export const VOICE_HOP_MS = 10;
/** Unvoiced frame in `voice.c`. */
export const UNVOICED = -32768;

/** After a word's sound ends, before its letters settle back. */
const RELEASE_MS = 120;
const SETTLE_MS = 260;
/** 1px per this many cents away from the note's start, clamped. */
const CENTS_PER_PX = 55;
const MAX_BEND_PX = 2.2;
/** Where a held note's reference pitch is read, past the attack's scoop. */
const REF_MS = 80;
/** Each later letter reads the pitch this much earlier: the wave. */
const LETTER_LAG_MS = 18;

const clamp = (x: number, a: number, b: number) => Math.min(b, Math.max(a, x));

export function isHeld(w: LyricWord): boolean {
  return w.timing !== "phrase" && w.end !== undefined && w.end - w.t >= HELD_MS;
}

/** Pitch at `ms` after the onset, linear between frames; unvoiced frames
 * hold the last voiced value. */
function pitchAt(c: readonly number[], ms: number): number | undefined {
  if (!c.length) return undefined;
  const x = clamp(ms / VOICE_HOP_MS, 0, c.length - 1);
  const i = Math.floor(x);
  const voiced = (k: number): number | undefined => {
    for (let j = k; j >= 0; j--) if (c[j] !== UNVOICED) return c[j];
    return undefined;
  };
  const a = voiced(i);
  const b = voiced(Math.min(i + 1, c.length - 1));
  if (a === undefined) return b;
  if (b === undefined) return a;
  return a + (b - a) * (x - i);
}

/** First voiced pitch at or after `ms` (the note's settled start). */
function reference(c: readonly number[], ms: number): number | undefined {
  for (let k = Math.max(0, Math.round(ms / VOICE_HOP_MS)); k < c.length; k++) {
    if (c[k] !== UNVOICED) return c[k];
  }
  return pitchAt(c, ms);
}

/** How far letter `k` of held word `w` sits above its line, in px, at a
 * song-clock instant (already led), given the next onset in its row.
 * Zero for anything that isn't a voiced held note. */
export function heldBend(w: LyricWord, nextT: number | undefined, clock: number, k = 0): number {
  if (!w.voice || !isHeld(w) || clock < w.t) return 0;
  const end = w.end ?? w.t;
  const release = nextT === undefined ? end + RELEASE_MS : Math.min(nextT, end + RELEASE_MS);
  const settle = clock < release ? 1 : Math.max(0, 1 - (clock - release) / SETTLE_MS);
  if (settle <= 0) return 0;
  // Held notes come in from HELD_MS on; full sway from about a second.
  const held = clamp((end - w.t - 350) / 600, 0, 1);
  const sung = Math.min(clock, release) - w.t;
  const ref = reference(w.voice.c, REF_MS);
  const now = pitchAt(w.voice.c, Math.max(0, sung - k * LETTER_LAG_MS));
  if (ref === undefined || now === undefined) return 0;
  return held * settle * clamp((now - ref) / CENTS_PER_PX, -MAX_BEND_PX, MAX_BEND_PX);
}
