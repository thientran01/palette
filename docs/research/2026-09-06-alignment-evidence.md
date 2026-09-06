# Alignment evidence pass — 2026-09-06

Production recipe and timing behavior remain unchanged. This investigation adds an offline margin probe, per-letter acoustic path support, and a clock-aware onset audit. No audio, lyrics, model assets or personal labels are committed.

## Measured onset error

Existing human labels are point annotations: they measure word onsets, not vowel sustain or word endings. Blur labels were explicitly described as less precise by the user. These two fixtures are regression evidence, not a representative quality estimate for all music. Recording-clock labels are converted using each capture time map; label identity/order is checked.

| Song | Window margin | Median absolute ms | P90 absolute ms | Within 100ms |
|---|---:|---:|---:|---:|
| Blur (343 tokens) | 0 | 164 | 544 | 30.6% |
| Blur | 500 (production) | 152 | 539 | 30.9% |
| Blur | 1000 | 155 | 576 | 30.9% |
| Heart To Heart (118 tokens) | 0 | 87 | 244 | 59.3% |
| Heart To Heart | 500 (production) | 84 | 243 | 60.2% |
| Heart To Heart | 1000 | 88 | 242 | 57.6% |

The production margin wins median error on both fixtures. A global window adjustment is rejected. Wider experiments are capped at the existing 336,000-sample memory bound, so long gaps cannot abort an otherwise valid experiment.

Capture-map RMS residuals: Blur 13ms (44 anchors), Heart To Heart 66ms (47). These residuals describe fit consistency, not independent acoustic clock accuracy.

Applying the user's 220ms display lead increases median onset differences to 327ms / 234ms respectively (P90 754ms / 463ms). This does not justify changing their setting automatically: labels carry tap/reaction uncertainty and do not measure preferred visual anticipation. Report raw and displayed errors separately.

## Difficult-section observations

Orchestra's recorded first five rows were replayed from a 25-second PCM prefix with the original time map. Relative to margin 500, margin 0 moved eight of 21 starts by at least 100ms (maximum 1085ms); margin 1000 moved seven (maximum 4065ms). These are stability measurements, NOT accuracy scores: this excerpt has no human truth labels. Boundary changes can push a forced path onto substantially different sounds.

The current decoder consumes each row as one ordered token stream. It cannot represent two simultaneous vocal sequences within a row. Parentheses do not identify whether a vocal is sequential, overlapping, or absent. Merely widening windows or treating all parentheticals as simultaneous is not a justified solution.

CTC letter support is exported for inspection. It is not calibrated confidence in timing; a low value alone must not decide cache replacement or visual fallback. The largest Blur errors include both low and moderate median support.

The user identified Free by Rumi: high notes, exaggerated syllables, and harmonies. Its saved timing cache exists, but no original audio dump was retained. Diagnostic capture was prepared with the existing cache backed up and its words preserved. This is the next fixture needed to distinguish onset errors from premature vowel endings. Restore the prior diagnostic setting after collecting it.

## Reproduction and validation

- Build `cargo build --example karaoke_acoustic --example karaoke_score` under src-tauri.
- Run the acoustic example with dump/model/runtime/output arguments. `PALETTE_PROBE_MARGIN_MS=0|500|1000` changes only the offline example; the app always calls margin 500 without diagnostics.
- `karaoke_score score-words <dump> <labels> <output>` remains the canonical source-identity scorer.
- `python scripts/karaoke_audit.py <output> --meta <meta.json> --labels <labels.txt> --lead-ms 0` reports comparable raw onset errors, plus row acoustic support and distance from window edges. Repeat with `--lead-ms 220` for displayed onset errors.
- Audit metrics reconciled with the Rust scorer on both fixtures. Three Python tests pin audio/song clock conversion, display lead, missing/incorrect identity rejection, and outlier accounting.
- 126 Rust tests pass (4 ignored), Clippy passes. Opt-in real-worker parity test reproduces all 372 Delulu word timings from the pre-investigation baseline; cold and reused results match exactly.

Next experiments should target Free's observed failures and compare acoustic preparation/model behavior against the same audio. Do not promote a candidate on stability or path support alone. Keep timing source improvements separate from presentation smoothing.