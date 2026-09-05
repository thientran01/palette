/**
 * The word-wipe lead: how far BEFORE a word's aligned onset its wipe
 * fires, on top of the per-player VOCAL_LEAD_MS the lines use. A karaoke
 * highlight is expected to lead the vocal; the aligner is unbiased against
 * tap truth (±70ms) yet Thien read words as "a little late" — the wipe
 * lands ON the onset and ramps 90ms, so full brightness trails the sound.
 *
 * Rust owns the value (settings.json "wordLeadMs"; karaoke.rs nudges it
 * from the Ctrl+Alt+[ / ] hotkeys and emits "word-lead"). This module
 * mirrors it live for the panel. docs/specs/2026-09-04-word-lead-nudge.md
 */
import { useEffect, useState } from "react";
import { commands, onWordLead } from "./backend";

export const WORD_LEAD_DEFAULT_MS = 160;

let value = WORD_LEAD_DEFAULT_MS;
let seeded = false;
const subs = new Set<(v: number, nudged: boolean) => void>();

function set(v: number, nudged: boolean): void {
  value = v;
  subs.forEach((cb) => cb(v, nudged));
}

function seed(): void {
  if (seeded) return;
  seeded = true;
  void commands.wordLead().then((v) => set(v, false));
  onWordLead((v) => set(v, true));
}

/** Current lead in ms (positive = earlier). */
export function wordLeadMs(): number {
  return value;
}

/** Live lead + a counter that bumps on every NUDGE (not on the seed), so a
 * consumer can show a caption for user-initiated changes only. */
export function useWordLead(): { leadMs: number; nudges: number } {
  const [state, setState] = useState({ leadMs: value, nudges: 0 });
  useEffect(() => {
    seed();
    setState((s) => (s.leadMs === value ? s : { ...s, leadMs: value }));
    const cb = (v: number, nudged: boolean) =>
      setState((s) => ({ leadMs: v, nudges: nudged ? s.nudges + 1 : s.nudges }));
    subs.add(cb);
    return () => {
      subs.delete(cb);
    };
  }, []);
  return state;
}

/** "Words 140 ms early" / "Words on time" / "Words 60 ms late". */
export function describeWordLead(ms: number): string {
  if (ms === 0) return "Words on time";
  return `Words ${Math.abs(ms)} ms ${ms > 0 ? "early" : "late"}`;
}
