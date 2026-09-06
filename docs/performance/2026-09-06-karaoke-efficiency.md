# Karaoke efficiency — measured 2026-09-06

## Scope and findings

Normal development playback measured about 4.7% of a 16-thread CPU over 30 seconds. A later per-process sample attributed most CPU to the WebView processes. These are observational samples, not a controlled before/after playback benchmark.

The lyric driver now suppresses writes when the exact formatted CSS value is unchanged. A 180-second Haunted replay at 60fps reduced setter calls from 10,830 to 6,842 (36.8%). All 6,842 effective changes matched the baseline, including a per-frame ordered SHA-256 digest. Frame rate, timing interpolation, shimmer, overlap, and seek behavior are unchanged. This is a measured write reduction, not a claim that whole-app CPU fell by 37%.

## Acoustic measurements

A 145.74-second local Delulu capture, 372 aligned words, development build:

| Configuration | Load seconds | Align seconds | CPU seconds | Peak working set MiB |
|---|---:|---:|---:|---:|
| 2 threads, spin enabled (existing) | 7.77 | 29.21 | 59.98 | 763 |
| 1 thread, spin disabled | 7.58 | 44.44 | 49.45 | 770 |
| 2 threads, spin disabled | 12.19 | 37.40 | 59.55 | 765 |
| 4 threads, spin disabled | 13.22 | 29.32 | 70.13 | 767 |

Single-run measurements on a live development machine vary with concurrent load. No alternative demonstrated a convincing efficiency/speed win. One/four threads also changed some alignment output; two threads without spinning preserved output but showed no benefit. Production remains TWO threads with spinning enabled.

A separate reused-model run: cold load 7.29s, alignment 22.32s; warm alignment 23.20s, with exactly equal word objects. Warm reuse removes a measured model load; it does not make the inference itself faster.

The actual worker integration also passed: 32.29s cold and 30.57s reused, with all 372 words unchanged. These separate runs illustrate timing variability; avoiding reload is the reliable benefit.

## Shipped behavior

One bounded background worker owns one model. Consecutive songs reuse it; five minutes without a job drops it. Errors discard the model, thread startup can retry, and stale worker failures cannot invalidate a replacement connection. Asset verification still occurs on each cold load. No model weights, pronunciation, decoder, frame windows, cache recipe, or source timing changes.

This trades temporary model residency for faster consecutive syncs. The model is not loaded at app startup, and there is no periodic inference/prewarming while idle. Mid-song incremental alignment is deferred; it would reduce the final wait but introduce scheduling and partial-result complexity rather than inherently reduce CPU.

## Reproduction

- `scripts/perf/lyric-writes.cjs <baseline-ref> <word-cache.json>` replays both drivers against the same data and reports actual writes plus effective-output hashes. Run from the repo with Node.
- `karaoke_acoustic` accepts the existing dump/model/runtime/output arguments. Optional benchmark-only environment variables: `PALETTE_BENCH_THREADS` (1–8), `PALETTE_BENCH_SPIN` (0 disables), `PALETTE_BENCH_REPEAT` (second alignment with exact equality assertion). These do not configure the app.
- Ignored Rust test `local_worker_preserves_cold_and_warm_results` exercises the actual worker using `PALETTE_WORKER_DUMP`, `PALETTE_WORKER_MODEL`, and `PALETTE_WORKER_BASELINE`. Baseline JSON fractions allow only 1e-12 serialization roundoff; word/checkpoint timestamps and cold-vs-warm results must match exactly.