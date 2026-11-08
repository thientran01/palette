/**
 * Voice-shaped motion for sung words (B · Sung, 2026-11-08). Thien: "I
 * want to emphasize ... emotion or dragged notes or vibrato ... make the
 * karaoke lyrics feel like the singer is really singing."
 *
 * Paint-only, so a row never rewraps: each word span gets a vertical
 * offset and a scale (index.css --word-y / --word-s).
 *
 * - Every word lifts a little while it is sounding and settles back after,
 *   so fast syllables read as one travelling wave rather than a strobe.
 * - A held note (lineCursor.ts HELD_MS) with a voice track also lifts with
 *   the singer's loudness, swells slightly across the hold, and follows the
 *   sung pitch relative to where the note started: vibrato becomes a gentle
 *   sway, a slide or a dragged note becomes a drift. Short syllables never
 *   follow pitch; that would put the jitter back.
 *
 * The voice track comes from the karaoke worker (voice.rs): pitch and
 * loudness of the separated vocal every 10 ms across each held word.
 * Nothing here moves on its own: no voice, no sway; paused, no motion.
 *
 * docs/specs/2026-11-08-karaoke-sung-highlight.md
 */
import type { LyricWord } from "./lrc";
import { isHeld } from "./lineCursor";

/** Frames of a voice track, from the word's onset. */
export const VOICE_HOP_MS = 10;
/** Unvoiced frame in `voice.c`. */
export const UNVOICED = -32768;

/** How a word sits while it sounds. */
const LIFT_PX = 1; // every word, while sounding
const LOUD_LIFT_PX = 1.2; // extra on a held note at full loudness
const ATTACK_MS = 120;
const SETTLE_MS = 260;
/** After a word's sound ends, before it settles (a breath of release). */
const RELEASE_MS = 120;
/** Pitch sway on a held note: 1px per this many cents, clamped. */
const CENTS_PER_PX = 55;
const MAX_BEND_PX = 2.2;
/** Where a held note's reference pitch is read, past the attack's scoop. */
const REF_MS = 80;
/** Full swell across a held note at full loudness. */
const SWELL = 0.035;

export interface Pose {
  /** Upward offset in px (positive is up). */
  y: number;
  /** Scale, 1 at rest. */
  s: number;
}
export const REST: Pose = { y: 0, s: 1 };

const easeOut3 = (u: number) => 1 - (1 - u) ** 3;
const clamp = (x: number, a: number, b: number) => Math.min(b, Math.max(a, x));

/** Linear read of a 10ms track at `ms` after the onset; unvoiced frames
 * hold the last voiced value. */
function read(track: readonly number[], ms: number, voiced = false): number | undefined {
  if (!track.length) return undefined;
  const x = clamp(ms / VOICE_HOP_MS, 0, track.length - 1);
  const i = Math.floor(x);
  const at = (k: number): number | undefined => {
    if (!voiced) return track[k];
    for (let j = k; j >= 0; j--) if (track[j] !== UNVOICED) return track[j];
    return undefined;
  };
  const a = at(i);
  const b = at(Math.min(i + 1, track.length - 1));
  if (a === undefined) return b;
  if (b === undefined) return a;
  return a + (b - a) * (x - i);
}

/** First voiced pitch at or after `ms` (the note's settled start). */
function reference(c: readonly number[], ms: number): number | undefined {
  for (let k = Math.max(0, Math.round(ms / VOICE_HOP_MS)); k < c.length; k++) {
    if (c[k] !== UNVOICED) return c[k];
  }
  return read(c, ms, true);
}

/** The pose of `w` at a song-clock instant (already led), given the next
 * onset in its row. */
export function voicePose(w: LyricWord, nextT: number | undefined, clock: number): Pose {
  if (clock < w.t - 40) return REST;
  const end = Math.max(w.end ?? w.t, w.t + 60);
  const release = nextT === undefined ? end + RELEASE_MS : Math.min(nextT, end + RELEASE_MS);
  const settle = clock < release ? 1 : Math.max(0, 1 - (clock - release) / SETTLE_MS);
  if (settle <= 0) return REST;
  const attack = easeOut3(clamp((clock - w.t + 40) / ATTACK_MS, 0, 1));
  const sung = Math.min(clock, release) - w.t;
  const voice = isHeld(w) ? w.voice : undefined;
  if (!voice) return { y: attack * settle * LIFT_PX, s: 1 };
  // Held notes come in from HELD_MS on; full expression from ~1s.
  const held = clamp((end - w.t - 350) / 600, 0, 1);
  const loud = clamp((read(voice.l, sung) ?? 0) / 100, 0, 1);
  const ref = reference(voice.c, REF_MS);
  const now = read(voice.c, sung, true);
  const bend = ref === undefined || now === undefined ? 0
    : clamp((now - ref) / CENTS_PER_PX, -MAX_BEND_PX, MAX_BEND_PX);
  const through = clamp(sung / (end - w.t), 0, 1);
  return {
    y: attack * settle * (LIFT_PX + LOUD_LIFT_PX * loud) + held * settle * bend,
    s: 1 + held * settle * through * SWELL * (0.4 + 0.6 * loud),
  };
}
