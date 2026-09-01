/*
 * Shared audio-reactive plumbing: one refcounted band subscription for the
 * whole app, reduced-motion gating (which also stops backend audio capture),
 * and the envelope/shaping helpers every reactive surface draws through.
 */
import {
  commands,
  onAudioBands,
  onSettingsChanged,
  SPECTRUM_BINS,
  type AudioBands,
} from "./backend";

const ZERO_BANDS: AudioBands = {
  bass: 0,
  mid: 0,
  high: 0,
  level: 0,
  spectrum: new Array<number>(SPECTRUM_BINS).fill(0),
};

type BandsCb = (b: AudioBands) => void;

const subscribers = new Set<BandsCb>();
let unsub: (() => void) | null = null;
let initialized = false;
const mq = window.matchMedia("(prefers-reduced-motion: reduce)");

/** The two facts a realm vote is computed from. The pref is unknown until
 * prefs_seed (or a settings-changed) lands. A defaulted true was the bug
 * that let Search vote capture on against a persisted off. */
type ReactiveVoteFacts = {
  prefEnabled: boolean;
  reducedMotion: boolean;
};

function voteFromFacts({ prefEnabled, reducedMotion }: ReactiveVoteFacts): boolean {
  return prefEnabled && !reducedMotion;
}

type Pref = { status: "unseeded" } | { status: "seeded"; enabled: boolean };
let pref: Pref = { status: "unseeded" };

/** The separator may bloom only when reduced motion is off AND the user hasn't
 * turned it off in prefs. Drives both the visual subscription and the backend
 * capture gate (no audio work for a suppressed separator). Exported for the
 * Waveform's settle-window choice: the single ZERO_BANDS payload apply() sends
 * on shutoff must settle on the prompt window, not the in-song-silence grace.
 * Unseeded returns false so a Waveform cannot bloom before the pref is known;
 * apply() still withholds the backend vote until seeded. */
export function reactiveOn(): boolean {
  if (pref.status !== "seeded") return false;
  return voteFromFacts({ prefEnabled: pref.enabled, reducedMotion: mq.matches });
}

function apply(): void {
  if (pref.status !== "seeded") return;
  const on = reactiveOn();
  // Also stops backend capture — no audio work for suppressed visuals.
  commands.setReactiveEnabled(on);
  if (!on) {
    unsub?.();
    unsub = null;
    // One final zero payload so subscribers decay to rest instead of freezing.
    for (const cb of subscribers) cb(ZERO_BANDS);
  } else if (!unsub && subscribers.size > 0) {
    unsub = onAudioBands((b) => {
      for (const cb of subscribers) cb(b);
    });
  }
}

/** Apply the persisted "reactive_separator" preference. Idempotent — a
 * no-change call is a cheap early return, so re-seeding on mount / echoing a
 * settings-changed event costs nothing. First call is what seeds the pref
 * and unlocks the first vote. */
export function setReactiveEnabledSetting(on: boolean): void {
  if (pref.status === "seeded" && pref.enabled === on) return;
  pref = { status: "seeded", enabled: on };
  apply();
}

/** Seed the pref (prefs_seed), listen for settings-changed, and vote only
 * once both facts are known. Pill mode / nothing playing must still stop
 * capture, so this runs even before any Waveform mounts. */
export function initReactive(): void {
  if (initialized) return;
  initialized = true;
  mq.addEventListener("change", apply);
  void commands.prefsSeed().then((s) => {
    setReactiveEnabledSetting(s.reactive_separator);
  });
  onSettingsChanged(({ key, value }) => {
    if (key === "reactive_separator") setReactiveEnabledSetting(Boolean(value));
  });
}

export function subscribeBands(cb: BandsCb): () => void {
  initReactive();
  subscribers.add(cb);
  apply();
  return () => {
    subscribers.delete(cb);
    if (subscribers.size === 0) {
      unsub?.();
      unsub = null;
    }
  };
}

/**
 * Asymmetric exponential smoother: light blooms with a hit (fast attack) and
 * breathes out (slow release). Stacks on top of the backend's own smoothing.
 */
export class Envelope {
  private value = 0;

  constructor(
    private attackMs = 45,
    private releaseMs = 550,
  ) {}

  step(target: number, dtMs: number): number {
    const tau = target > this.value ? this.attackMs : this.releaseMs;
    this.value += (target - this.value) * (1 - Math.exp(-dtMs / tau));
    return this.value;
  }

  get current(): number {
    return this.value;
  }
}
