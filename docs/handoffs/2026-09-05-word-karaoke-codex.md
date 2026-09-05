# Word karaoke: Codex handoff

Updated 2026-09-05 after review of PR #162 (`feature/word-karaoke`).

## User intent and live baseline

Improve word accuracy and smoothness without disturbing the line highlight.
Thien reports the current timing is very synced, but words sometimes stack
onto the wrong lines. Keep the 160ms word lead preference and 90ms wipe
attack; do not retune them from a synthetic test. Codex now owns the branch;
Claude is stopped. Review fixes are in this PR; merging still needs the
accuracy gate below resolved.

## Measurement correction — read before tuning

The original tap page exported audio-relative seconds. The original scorer
compared those directly to song-relative predictions. Capture does not start
at song zero. The old 177ms Blur / 160ms Heart To Heart results, bias claims,
oracle comparisons and fitted constants are therefore historical and must
not be used to claim acceptance or pick a model.

The corrected scorer maps legacy/Audacity labels through the dump's exact
TimeMap. New tap downloads declare `# clock: song` and are already mapped.
A low anchor-fit residual measures internal consistency, not absolute
synchronization with the singer or speaker output. Tap uncertainty has not
been independently quantified.

Corrected Blur matrix (343 labels, preserved Desktop evidence, current code):

| Stages | Median ms | p90 ms | Within 100ms | Bias ms | Worst line mean ms |
|---|---:|---:|---:|---:|---:|
| Fixed prior | 204 | 546 | 26% | -54 | 922 |
| Prior + energy | 246 | 652 | 22% | -135 | 922 |
| Prior + flux | 239 | 745 | 27% | -79 | 1515 |
| Song lead (current) | 215 | 578 | 24% | -113 | 992 |
| Song lead + rate | 316 | 872 | 15% | -376 | 1163 |
| Song + energy | 336 | 938 | 15% | -428 | 1163 |
| Song + energy + end | 489 | 1336 | 12% | -565 | 1163 |

The documented gate is median <=200ms, no line mean absolute error >1500ms
on both songs, and no worsening on either song. It is **not satisfied**.
Do not tune on this one noisy recording just to cross the threshold.

## Evidence and reproduction

Complete Blur dump: `C:/Users/Thien/OneDrive/Desktop/karaoke-blur/`.
Contains `pcm.i16`, `lyrics.lrc`, `words.json`, `meta.json`, `labels.txt`.

Heart To Heart: `C:/Users/Thien/OneDrive/Desktop/karaoke-heart-to-heart/`
contains labels, WAV and old tap page, but **no original TimeMap metadata**.
`%APPDATA%/com.thien.pulse/karaoke-dumps` was absent at review time. Do not
invent a map from rounded historical output or reuse the old score. Recover
the original complete dump before claiming a corrected second-song result.

From the PR worktree's `src-tauri` directory:

```powershell
$env:CARGO_TARGET_DIR = 'C:/Users/Thien/Projects/pulse/src-tauri/target'
cargo run --offline --example karaoke_score -- matrix C:/Users/Thien/OneDrive/Desktop/karaoke-blur C:/Users/Thien/OneDrive/Desktop/karaoke-blur/labels.txt
```

For new evidence: enable `karaokeDump`, listen from near the start without
seeking, then change tracks. `karaoke_score template <dump>` writes an
Audacity audio-clock text template and a song-clock JSON sidecar with line
identity. `python scripts/karaoke_tap.py <dump> <output>` copies all four
evidence files alongside its self-contained page so dump eviction cannot
remove the scoring metadata. Preserve original labels. Partial label prefixes
score; a candidate missing a labelled word is rejected.

## Reviewed fixes

- Words retain source `line_t`; membership never follows calibrated onset.
- Short/dense lines reserve room for remaining words; impossible lines use
  line-only highlighting. Empty LRC stamps bound instrumental breaks.
  Predictions never extend beyond captured audio.
- Store v5 binds exact LRC and recipe `song-lead/3`. Old stores re-record.
- Diagnostic dumps allow 55% listens, but persistent caches require mapped
  audio to reach duration minus 1500ms. Partial attempts remain retryable.
- Unresolved seek strikes reject finalization. Delivery deficits over 400ms
  discard ambiguous recordings instead of inserting fabricated silence.
  Failed worker spawning releases the alignment flag. Cache/LRC reads are
  skipped outside the near-start recording window.
- Word-lead events are registered before seeding, and stale seeds cannot
  overwrite nudges. Bracket hotkeys can be rebound. Nudge captions avoid
  the upper return button.
- Tap playback and seeks share the dump map; autosave resumes near the next
  token and retains calibration. Calibration cannot add lyric taps.

The previous Who Knows end-clamp explanation was a hypothesis, not a
confirmed reconstruction. Conservative recording checks are warranted without
claiming that specific incident is proved.

## Architecture and remaining work

`align.rs` owns measured mapping and placement; `karaoke.rs` owns capture,
evidence and persistence; `karaoke_score.rs` compares to labels. `posClock`
owns display time; `LyricsPanel.tsx` updates CSS wipes in one rAF without
per-word React rerenders. Preserve existing line scheduling and neutral text.

Resolve the evidence gap and accuracy gate before merging under the current
spec. Vocal separation/forced alignment remains a future experiment: model
size, CPU time, licensing and accuracy require actual model-specific checks;
previous 50-100ms/seconds-per-song estimates were not benchmarks.

Run `npm run build`, `npm test`, `cargo test --offline --all-targets`,
`cargo fmt --check`, `cargo clippy --offline --all-targets -- -D warnings`,
and `python -m unittest scripts/test_karaoke_tap.py`. Real playback and caption
placement still need live verification; unit tests cannot certify audible sync.

Local review validation: production frontend build passed; 34 frontend tests,
96 Rust library tests plus 4 scorer tests passed (2 live probes ignored),
16 tap-tool tests passed; Rust formatting and clippy with warnings denied
passed. The real Blur template-to-tap-page flow also generated successfully
in a temporary copy without modifying the original evidence.
