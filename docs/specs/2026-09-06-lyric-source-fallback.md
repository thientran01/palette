# Source-timed fallback for missed entrances

Approved direction: retain accurate acoustic word timing, but use the original line timestamp when the alignment misses the line. This is a display guard and applies to existing saved caches without another listen.

## Evidence and scope

A read-only scan of 11 current vocal-trial caches (660 nonempty matched line windows) found 18 first-word delays over 1.5 seconds and half the source interval, plus five entire word spans outside their source window. These are timing disagreements, not independently verified vocal onsets. An eight-token row at 136770ms starts at 139718ms, only 262ms before the next source row. Baby Flower's held-note row at 163660ms starts at 171808ms. The user reported missed lines and has previously confirmed this Baby Flower entrance is much earlier.

Use a stricter initial threshold than the exploratory scan: onset must be more than both 1500ms and 75% of the source interval late. Source windows must be positive, finite and at most 20 seconds. Do not reject early entrances or quick delivery merely because a line finishes well before the next timestamp. Exclude parentheses/backing phrases to preserve simultaneous voices. Do not guess a final-line boundary.

## Behavior

1. Attach words to their existing source line identity.
2. Reject nonfinite/reversed word spans. Clear stale attached words if a replacement contains no words for this line.
3. For severe late entrances, omit word spans and render the existing source-timed line highlight. Keep the original source text, timestamp, scroll clock and neighboring rows unchanged.
4. Expose a reason on the rendered line for the read-only audit; do not rewrite saved timings or invent intra-line timing.

This does not repair word boundaries within otherwise plausible lines, and a source timestamp can itself be inaccurate. Duration compression alone is deferred: no such case appeared in the initial scan, and fast delivery followed by a rest is legitimate.

## Validation

Regression tests first reproduce the late burst, stale replacement and malformed-span failures. Preserve early vocals, fast delivery, overlapping adlibs and ambiguous windows. Run the production parser/fallback against local caches with scripts/audit-source-fallback.mjs; only timing metadata may be published, not local lyrics or audio. Inspect the affected cohort before publishing. Ship as a separate PR above the current library branch.

### Results

The three failing regression cases passed after implementation; the full suite passed 65 tests across nine files (59 before this change), and the production build passed. A comparison fixture using the actual LyricsPanel renderer confirmed the late saved alignment stays dim at the source entrance while the fallback highlights the full line on time.

The cache audit suppressed word spans on 24 of 4,284 lines across 79 source-matching cache records: eight of 685 current vocal-preview lines, zero of 723 older preview lines, and 16 of 2,876 regular cache lines. Cache variants are counted independently, not as unique songs. Sixteen missing-word rows were counted separately because they already use whole-line display. These counts measure the scope of the guard, not verified listening accuracy.

Audit limitation: cache files do not provide verified track duration to the parser. The audit uses duration zero, so duration-dependent outro boundaries are omitted and final-line fallback counts may be understated compared with the running app. No affected example was found in the current cohort during review.
