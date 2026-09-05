# Word lead: perceptual offset, nudge hotkeys, and the scorer's `fit`

**Date:** 2026-09-04 · **Approved by:** Thien ("go ahead build layer one") · **Scope:** one PR (commits on `feature/word-karaoke`, PR #162), follows `2026-09-04-karaoke-aligner-ladder.md`

## Problem

With the song-lead aligner shipped (177 / 160ms median vs tap truth on
Blur / Heart To Heart, biases +27 / −71ms) Thien's live verdict was still
"the words come in a little late". Two things hide in that:

1. **Perception.** The wipe fires ON the aligned onset and ramps over
   `WORD_ATTACK_MS` (90ms), so full brightness lands ~90ms after the sound.
   Karaoke highlights are expected to LEAD the vocal; the line highlight
   already does (`VOCAL_LEAD_MS`, 50ms on Spotify, instant). Words therefore
   read ~100–150ms late even when the data is right.
2. **Nothing learns from Thien.** The only feedback signal that measures his
   perception (rather than the audio) is him saying "late" — and there was
   no way to act on it short of a code change.

## Design (approved)

### Layer one: a word lead Thien can nudge

- `WORD_LEAD_DEFAULT_MS = 120`: the wipe for a word fires this far before
  its aligned onset, on top of the per-player `VOCAL_LEAD_MS`. Persisted in
  settings.json as `"wordLeadMs"`, clamped to [−200, 400].
- Two new rebindable global hotkeys on the existing `hotkey_defs` table
  (so they show in Preferences → Hotkeys and rebind like the rest):
  - `wordsearlier` — **Ctrl+Alt+[** — lead += 20ms ("words earlier")
  - `wordslater` — **Ctrl+Alt+]** — lead −= 20ms ("words later")
- Rust owns the value (`karaoke::word_lead_ms`, `karaoke::nudge_word_lead`):
  a nudge reads settings, steps, clamps, writes, and emits `"word-lead"`
  with the new value. `word_lead` is the seed command the frontend calls
  at mount.
- Frontend: `src/lib/wordLead.ts` holds the live value (seed + event) and a
  `useWordLead()` hook. `LyricsPanel` runs the wipe at
  `leadMs + wordLeadMs` and, on a nudge (never on mount), shows a 1.6s
  caption in the lyrics viewport — "Words 140 ms early" / "on time" /
  "60 ms late" — neutral chrome, `aria-live="polite"`, the same chip
  grammar as the return-to-now button. Browser mock: value in
  `localStorage["pulse.wordLeadMs"]`, `window.__nudgeWordLead(±20)` drives
  the toast in preview.
- The line highlight is untouched: Thien called the lines "perfect" in
  July; this offset is words-only.

### The scorer's `fit`

`cargo run --example karaoke_score -- fit <dump-dir>...` (each dir holds
`labels.txt`): grid-searches the prior's constants across ALL truth songs
and prints the best set beside the current defaults.

- `align::PriorParams { lead_ms, rate_syl_s, fill }` (defaults =
  `LEAD_MS`, `RATE_SYL_S`, `FILL`) threaded through `align_with_params`;
  `align_with` keeps the defaults.
- Grid: `rate_syl_s` 1.5..6.0 step 0.1 × `fill` 0.50..1.00 step 0.05 under
  `Stages::shipped()` (song lead calibrated per song, so `lead_ms` only
  matters as the fallback); objective = mean of per-song medians.
  Separately a `lead_ms` sweep −100..600 step 10 under `PRIOR_ONLY` for
  the fallback default.
- Output: per-song median at defaults and at the best set, and the best
  constants. Updating the constants stays a human commit — `fit` reports,
  it never writes.

### Out of scope

Per-song offsets (the song-lead calibration already adapts per recording);
a learned model (revisit at ≥10 tapped songs); any change to line timing.

## Tests

- `karaoke`: nudge clamps at both ends and steps by 20 (pure helper).
- `lrc`/panel: unchanged wipe math; the offset is additive at the call
  site.
- `fit` runs on the two truth dirs and reports.

## Live checklist

- Ctrl+Alt+[ during a word-timed track: caption "Words 140 ms early", wipe
  visibly earlier; Ctrl+Alt+] back. Value survives relaunch (settings.json).
- Preferences → Hotkeys lists both, rebindable.
- Default 120ms: Thien's "a little late" verdict re-checked on the same
  songs.
