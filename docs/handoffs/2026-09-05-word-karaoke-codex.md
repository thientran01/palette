# Word karaoke: Codex handoff

Updated 2026-09-05 after review of PR #162 (`feature/word-karaoke`).

**Current result:** fixed prior is selected (`fixed-prior/4`), scoring 204ms
on Blur and 154-157ms on bounded Heart To Heart recovery. The later recovery
section supersedes the missing-map status and song-lead selection below.
Blur still misses the 200ms gate; the goal and PR remain open.

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
| Fixed prior (current) | 204 | 546 | 26% | -54 | 922 |
| Prior + energy | 246 | 652 | 22% | -135 | 922 |
| Prior + flux | 239 | 745 | 27% | -79 | 1515 |
| Song lead (previous) | 215 | 578 | 24% | -113 | 992 |
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
- Store v5 binds exact LRC and recipe `fixed-prior/4`. Old stores re-record.
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

## Superseding evidence recovery and stage selection (2026-09-05)

Thien supplied the original Downloads labels. SHA-256 confirms byte identity
with the Desktop copies:

- Blur `labels.txt`, 343 rows: `a3c7ae3731e6626411da92c658f6cb4b3a31af71201a6fe5166ebab4a29e17fd`.
- Heart To Heart `labels(1).txt`, 118 rows: `10141ed5eb9ae1be1c793cb3a0502849192a683468620f371ba94d9609504ea0`.

A reconstructed Heart To Heart dump was found on the Desktop. It is not the
original dump: words.json is empty and anchors are unavailable. Nevertheless,
its PCM equals the WAV payload exactly, the WAV equals the original tap
page's embedded audio exactly, and its lyric text equals that page's 30 lines.
The original Claude session tool output (b719aa93-c79a-4132-866f-7458a3eafdec,
JSONL row 1636; format in row 1634) independently records intercept 454ms
formatted to zero decimals and slope 0.062483ms/sample to six decimals.
These imply at most 2.191ms map rounding error over the 3,382,560 samples.
This does not bound anchor bias or tapping error, only reconstruction rounding.

Corrected Heart To Heart matrix using those recovered values:

| Stages | Median ms | p90 ms | Within 100ms | Bias ms | Worst line mean ms |
|---|---:|---:|---:|---:|---:|
| Fixed prior | 156 | 445 | 31% | -65 | 757 |
| Prior + energy | 473 | 844 | 14% | -405 | 1377 |
| Prior + flux | 211 | 576 | 23% | 159 | 564 |
| Song lead | 494 | 747 | 6% | -495 | 1187 |
| Song lead + rate | 662 | 950 | 0% | -727 | 1305 |
| Song + energy | 779 | 1140 | 8% | -655 | 1495 |
| Song + energy + end | 823 | 1427 | 2% | -789 | 1605 |

Running the real scorer at all four intercept/slope rounding corners yields
fixed-prior medians 154-157ms and song-lead medians 493-495ms. The choice is
robust to this reconstruction uncertainty. `Stages::shipped()` now selects
the existing fixed prior, recipe `fixed-prior/4`; no constants were fitted.
It improves both available recordings while retaining the captured-audio,
silence and coverage gates. There is still no immediate untimed-word fallback.

The merge accuracy gate remains unmet: Blur is 204ms. The previous section's
missing-map blocker is superseded by this bounded reconstruction for candidate
comparisons, not by a claim that original anchors/live words were recovered.
The 160ms user preference, 90ms attack and line scheduling remain unchanged.

## Offline forced-alignment experiment (not integrated)

`scripts/research/karaoke_mms_probe.py` runs TorchAudio 2.8 MMS_FA, using
full-mix PCM and known LRC only. Per-row windows extend 500ms on each side,
cap at 20s, and use star tokens at the two boundaries. uroman 1.3.1.1
normalizes each source token; original display text and line identity stay
separate. Predictions are completed before labels are read. No constants,
thresholds or alignment paths were fitted to labels.

The app's Rust scorer independently verifies external predictions via
`karaoke_score score-words <dump> <labels> <candidate.json>`. It uses the
same clock correction, source validation and quantiles as native candidates.
The probe's Python diagnostics are provisional; Rust output is authoritative.

| Candidate | Blur median / p90 / worst line | Heart To Heart median / p90 / worst line |
|---|---|---|
| Current fixed prior | 204 / 546 / 922ms | 156 / 445 / 757ms |
| MMS_FA full mix | 152 / 539 / 1224ms | 84 / 262 / 629ms |

MMS share within 100ms: 31% / 59%; biases -138 / -60ms. HtH uses the
bounded reconstruction documented above. All 343 / 118 labelled tokens score.
The candidate clears the numerical acceptance gate on this evidence, but
that is not proof of generalization or an in-app audible improvement.

Measured CPU inference+alignment time: 50.79s Blur, 47.50s HtH with two
Torch threads. One-time model load plus download took 37.84s in this run,
excluding environment setup. Model has 315,466,396 parameters; downloaded
weights occupy 1,262,047,414 bytes (~1.18GiB). Peak RAM was not measured.
This supersedes the earlier speculative several-seconds-per-song estimate.

Artifacts: `%TEMP%/pulse-karaoke-mms-lab/fullmix-results/` contains JSON
predictions; `models/hub/checkpoints/model.pt` is the isolated model cache.
The running app still uses `fixed-prior/4`. No audio was uploaded.

Reproduce using an isolated Python environment with CPU torch==2.8.0,
torchaudio==2.8.0, uroman==1.3.1.1 and numpy, set TORCH_HOME to an isolated
model cache, then:

```powershell
python scripts/research/karaoke_mms_probe.py --repo . --out <output-dir> <blur-dump> <hth-dump>
cargo run --manifest-path src-tauri/Cargo.toml --example karaoke_score -- score-words <dump> <labels> <output-dir/song.json>
```

Integration requirements still unresolved:

- Model download, packaging and runtime: this was an isolated Python CPU
  probe, not a production dependency or committed model artifact. MMS model
  uses [CC-BY-NC 4.0](https://docs.pytorch.org/audio/main/generated/torchaudio.pipelines.MMS_FA.html).
- Early word timing needs correct rendering without moving text between
  lines. Current row-only activation blocks the requested 160ms lead on 12
  Blur tokens (up to 620ms) and 2 HtH tokens (up to 298ms) in a schedule
  simulation. Eight total cross-row end overlaps also need an explicit
  rendering policy. Do not blindly clamp and claim the offline scores hold.
- Vocal separation has not yet been benchmarked; it might improve noisy
  line starts, at added runtime and model cost.
- Live smoothness, seeks, pause/resume, cache migration and CPU contention
  must be verified after integration. Preserve user word lead preferences.


## 2026-09-05 — Quantized ONNX and renderer follow-up

The int8 MMS_FA export is 356,235,747 bytes (~340 MiB), versus
1,262,351,625 bytes for float ONNX. On this machine, using two CPU threads,
full-mix alignment took 22.40s for Blur and 24.15s for Heart To Heart
(model load separately 0.95s). Rust `score-words` reports median/p90
152/539ms for Blur and 84/243ms for Heart To Heart. Float results were
152/539 and 84/262ms. This is only two recordings; no generalization claim.
The original Heart To Heart map reconstruction caveat still applies.
`scripts/research/karaoke_mms_export.py` reproduces the export, float
bundle parity checks at three audio lengths, and quantization. The probe
accepts `--onnx`. Models remain outside Git and are NOT integrated into
the app. Runtime packaging, romanization and CTC parity remain open work.

The renderer now keeps timed spans mounted in their original rows with
stable font weight. One frame driver permits early onsets and overlapping
sung tails without changing line stamps, marker ownership or scroll time.
It cancels animation immediately on pause and avoids unchanged span writes.
Frontend tests: 40 pass (previously 34); three fail when the old current-row
gate is substituted. The browser preview exposed an Illegal invocation
from unbound requestAnimationFrame; the call site now wraps both frame APIs.
Browser DOM/CSS checks show distinct 32px mock rows and correct gradients.
This validates browser layout, not native audible synchronization.


## 2026-09-05 — Native acoustic integration, validation in progress

`acoustic.rs` now runs ONNX Runtime 1.22 through ort rc10, with the Rust
uroman 0.7 romanizer and a bounded CTC decoder. All 461 word onsets/endings
across both recordings match the Python int8 predictions exactly. Native
alignment took 23.53s Blur / 23.16s Heart To Heart, with about 1s model load
before checksum validation was added. These are development CPU timings.
64 deterministic synthetic CTC oracle cases come from torchaudio 2.8;
no song audio or model weights are committed. Rust checks passed before
live worker integration; rerun after integration/review changes.

The background `commit_sync` worker selects the model when the installed
app-data `karaoke-model/enabled` marker exists at startup. Selection stays
fixed until restart. Model/DLL checksums are verified on load; caches and
dumps use `mms-int8/1`, not the fixed-prior recipe. Failed inference retains
diagnostic audio when enabled and stays retryable; it never caches a prior
under the model recipe. Session/model memory drops after each alignment.
`scripts/install_karaoke_model.py` installs already-local verified assets,
with the enable marker last. No model distribution or audio upload occurs.
MMS weights remain CC-BY-NC 4.0; the script documents the license.

Local assets have been installed to `%APPDATA%/com.thien.pulse/karaoke-model`.
The task dev app is restarting with these changes. PR #162 remains open;
new native changes require review and live-worker verification before merge.
Native example: `karaoke_acoustic <dump-dir> <model.onnx> <onnxruntime.dll> <output.json>`.


Native follow-up verification: installed-directory replay (no Python DLL
search path) still matches all 461 starts/ends/source identities. Including
asset checksum verification, load was 6.10/6.26s and alignment 23.16/23.27s.
Three-agent review (+failure/recurring-family scope) confirmed punctuation
abort and loader panic defects; both fixed, reviewed again, and pinned by
two assertions that fail under the old behavior. Standalone quoted Korean
is also covered; internal apostrophes in contractions remain acoustic targets.
Full Rust suite: 101 pass, 2 pre-existing ignored, plus 5 scorer tests;
clippy all-targets passes. The native app started at 14:52 PT with
`karaoke: acoustic model enabled (mms-int8/1)`. Win32 enumeration verified
a visible Palette window (PID 31472, HWND 593558). A listening-feedback
question is pending; native audible feel has not been signed off.
