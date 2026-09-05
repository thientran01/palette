# Word karaoke data plumbing: anchors, dump, scorer

**Date:** 2026-09-04 · **Approved by:** Thien (brainstorm session) · **Scope:** one PR (commits on `feature/word-karaoke`, PR #162)

## Problem

Word timing in the local karaoke aligner has only ever been judged by ear on
one track. The 2026-09-04 aligner rewrite passed nine synthetic-tone unit
tests and regressed badly live (beat-onset aliasing, pre-stamp words merged
onto the previous line); it was reverted the same day. Two data problems make
every aligner change a guess:

1. **No ground truth.** Nothing measures how far a word lands from its sung
   onset, so no candidate aligner can be compared to the current one.
2. **The time base is one number.** The recording is anchored to a single
   origin taken when recording arms, and every later sample trusts the
   sample counter. The media loop sees a fresh `(position_ms,
   position_at_ms)` pair every few seconds (Spotify ms-precise, Apple Music
   floored to whole seconds) and throws all but the first away. A
   mid-recording seek is not detected at all and silently corrupts the
   word times.

Vault context: the even-pace karaoke sweep was removed 2026-07-06 on Thien's
"only okay is not perfect" call, so the bar is perfect-or-nothing; the
position pipeline itself was fixed in 2026-07 by capturing ~2,265 real
position payloads and replaying them offline (PRs 15–23). This PR brings that
method to word timing.

## Design (approved)

### 1. Anchors instead of an origin (`karaoke.rs`, `align.rs`)

- While a recording is armed, every `observe` call with a fresh pair
  (`position_at_ms` differs from the last accepted anchor's) records an
  anchor `(sample_index, position_ms)`:
  `sample_index = received_input_frames − staleness_ms × rate_in / 1000`
  converted to the 16kHz output grid (`× TARGET_HZ / rate_in`), where
  `staleness_ms = clamp(unix_ms() − position_at_ms, 0, STALE_CAP_MS)`.
  Pairs with `position_at_ms == 0` (never stamped) are not anchors.
- At commit, `TimeMap::fit(&anchors, nominal_ms_per_sample)` runs a
  least-squares line `position_ms = intercept + slope × sample_index`.
  The slope is clamped to nominal ± 0.5% (shared-mode WASAPI shares the
  player's audio engine clock, so real drift is ~0; a slope outside the
  clamp means bad anchors, and the map falls back to nominal slope with the
  intercept refit). Fewer than two anchors → `TimeMap::from_origin`
  (today's behaviour: projected origin at nominal rate). `meta.json`
  records which path ran.
- Robustness: after the first fit, anchors with residual > `SEEK_RESIDUAL_MS`
  (2500, beyond Apple Music's 2s jitter band) are treated as a seek, see
  below; anchors with residual > 1200ms but below that are dropped as
  jitter outliers and the line is refit once.
- `align::align` takes `&TimeMap` in place of `origin_ms`. `TimeMap` lives
  in `align.rs` (`pub`) with `pos_ms(sample_index) -> i64` and
  `sample_at(position_ms) -> usize`; the aligner's per-line windows use
  `sample_at`, and word times come from `pos_ms`. Piecewise interpolation
  was rejected: it would bake Apple Music's 1s sawtooth into the map.
- **Seek detection at observe time.** Once ≥ 2 anchors exist, a new pair
  whose position differs from the running map's prediction by more than
  `SEEK_RESIDUAL_MS` aborts the recording (`RECORDING` off, slot cleared)
  WITHOUT inserting the key into the miss set, so the track re-records on
  the next clean listen. Logged at info.

### 2. Dump (`karaoke.rs`)

- Opt-in: `settings.json` key `"karaokeDump": true`, read via
  `settings::get_bool` at commit time (installed builds have no env vars).
- Written on the align thread after the fit and BEFORE the word store, so
  a failed align still leaves the evidence. Directory
  `app-data/karaoke-dumps/<key>/`:
  - `pcm.i16` — 16kHz mono little-endian i16 (~8MB for four minutes).
  - `lyrics.lrc` — the cached synced LRC verbatim.
  - `words.json` — the words the live aligner produced (same shape as the
    store file), or `[]` when it produced none.
  - `meta.json` — artist/title/album/duration_ms, `rate_in`, `origin_ms`
    (projected), `anchors` `[[sample_index, position_ms], …]`,
    `fit` `{intercept, slope, nominal_slope, clamped, residual_rms_ms,
    n_anchors, from_origin}`, `dropped_outliers`.
- Cap: 5 dumps, oldest by mtime evicted (same pattern as `evict_old`).
- Write failures log a warn and never block the word store.

### 3. Scorer (`src-tauri/examples/karaoke_score.rs`)

Run from `src-tauri`: `cargo run --example karaoke_score -- <cmd> <dump-dir> [labels.txt]`.

- `template <dump-dir>` — re-runs `align::align` on the dump and writes
  `<dump-dir>/labels.template.txt` in Audacity label format
  (`start\tend\ttext`, one row per aligner token, `start = end = word.t / 1000`,
  text = the token). Thien imports it into Audacity over `pcm.i16` (File →
  Import → Raw Data, 16-bit signed, mono, 16000Hz), drags each marker onto
  the sung onset, exports labels. Matching is by row order, never by text,
  so tokenization never has to be reconciled.
- `score <dump-dir> <labels.txt>` — re-runs the current aligner, pairs
  token i with label row i (row count mismatch = hard error naming the
  first divergent line), and prints: median |Δ|, p90 |Δ|, share within
  100ms, mean signed Δ (early/late bias), the five worst lines with their
  text, and whether the replay reproduces the dump's live `words.json`
  exactly (dump-fidelity check). Also prints the fit residual RMS from
  `meta.json` so clock error and aligner error stay separately visible.
- `lib.rs`: `mod align` becomes `pub mod align` so the example can link
  against `pulse_lib`. No other visibility changes.

### Data flow

media loop pair → `observe` → anchor (or seek abort) → commit → `TimeMap::fit`
→ dump (if enabled) → `align::align(&samples, &map)` → coverage gate → word
store → `karaoke-ready`.

### Error handling

- < 2 anchors: origin fallback, recorded in meta.
- Slope outside clamp: nominal slope, intercept refit, `clamped: true`.
- Seek: abort recording, no miss-set entry.
- Dump write failure: warn, continue.
- Scorer: label/token count mismatch → exit 1 with the offending line.

## Tests

- `align::TimeMap`: fit recovers a known intercept and slope from anchors
  with ±20ms noise; anchors floored to whole seconds average to within
  50ms of truth; slope outside the clamp falls back to nominal; a single
  anchor yields `from_origin`.
- `karaoke`: a pair 3s off the running map aborts the recording and leaves
  the miss set untouched; a 1.5s outlier is dropped and the fit survives.
- Existing aligner tests updated to pass a `TimeMap::from_origin(0, …)`.

## Live checklist

- Enable `karaokeDump` in settings.json, listen to two or three Spotify
  tracks on this PC from the start WITHOUT pausing (the recorder ends at
  the first pause) past the 55% mark; confirm dump dirs appear.
- `template`, mark labels in Audacity, `score`: the printed median/p90 is
  the current aligner's baseline. Record it in the vault project note.
- Seek mid-track during a recording: log shows the abort, no karaoke file,
  the track records cleanly on the next full listen.

## Out of scope

Any aligner change. Word-level lyric fetching. Vocal separation / CTC
alignment (costed 2026-09-04: ~350MB models on demand, 15–60s CPU per song,
1–2GB transient RAM, ~5 PRs; MMS aligner is CC-BY-NC).
