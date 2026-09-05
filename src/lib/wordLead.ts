/**
 * The word-wipe lead: how far BEFORE a word's aligned onset its wipe
 * fires, on top of the per-player VOCAL_LEAD_MS the lines use. A karaoke
 * highlight can lead the vocal; Thien preferred a 160ms lead with the
 * 90ms brightness ramp. This is a perceptual preference, not a measured
 * correction for the aligner's absolute error.
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
let listening = false;
let receivedEvent = false;
let initializing: Promise<void> | undefined;
const subs = new Set<(v: number, nudged: boolean) => void>();

function set(v: number, nudged: boolean): void {
  value = v;
  subs.forEach((cb) => cb(v, nudged));
}

/** Register before reading the snapshot; live nudges always win over it.
 * Failed attempts resolve safely and can be retried by the next consumer. */
export function initWordLead(): Promise<void> {
  if (seeded) return Promise.resolve();
  if (initializing) return initializing;
  initializing = (async () => {
    if (!listening) {
      await onWordLead((v) => {
        receivedEvent = true;
        set(v, true);
      });
      listening = true;
    }
    const initial = await commands.wordLead();
    if (!receivedEvent) set(initial, false);
    seeded = true;
  })().catch(() => {
    // Keep the default/latest event and retain any registered listener.
    // Leave seeded false so a later initialization can retry.
  }).finally(() => {
    initializing = undefined;
  });
  return initializing;
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
    void initWordLead();
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
