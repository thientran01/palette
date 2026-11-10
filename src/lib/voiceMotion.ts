/**
 * Held notes. A word sung at least HELD_MS is a held note: the row cursor
 * (lineCursor.ts) paces it on the aligner's letter checkpoints, and the
 * karaoke worker (src-tauri/src/voice.rs) stores its voice track.
 *
 * Held letters used to sway with the singer's pitch (C · Rise,
 * 2026-11-08). Thien removed it on 2026-11-10: "remove the singing
 * effects and just have the rise".
 *
 * docs/specs/2026-11-08-karaoke-sung-highlight.md
 */
import type { LyricWord } from "./lrc";

/** Sung at least this long: a held note. Must match voice.rs HELD_MS. */
export const HELD_MS = 450;

export function isHeld(w: LyricWord): boolean {
  return w.timing !== "phrase" && w.end !== undefined && w.end - w.t >= HELD_MS;
}
