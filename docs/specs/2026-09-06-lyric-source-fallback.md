# Source-guided lyric entrances

## Approved behavior

Source line timestamps guide acoustic alignment; they never replace valid word/syllable animation with a plain highlight. This supersedes the earlier plain-highlight fallback after direct user feedback. Invalid or missing word spans retain ordinary line display.

## Implementation

The separated-vocal development trial decodes normally, then retries lines whose first spoken target starts more than 1500ms after the source timestamp. Require a following source row, a positive interval at most 20 seconds, and recording coverage at the source entrance. Exclude parenthesized voices, repeated-la rows, and initial oh/ooh/ah rows covered by the previously approved entry refinement. Normal mixed-audio alignment remains unchanged.

Reuse the same emissions. Charge eight log-score units per second of delay beyond source plus 500ms, once when entering the first spoken target. Do not charge held frames. The audio still determines all later word/spelling spans; no uniform word redistribution or timestamp shifting is performed. Accept only an earlier path whose first-target average posterior and total posterior across its frames each retain at least half the original target's support, with positive support. This relative guard prevents the source prior from selecting a weak early noise frame over a strongly supported late phoneme. Posterior support is not a calibrated timing probability.

The initial 75%-of-line gate missed a retained Baby Flower recording: 3.343s late in a 10.37s source interval. The retry therefore uses the absolute 1500ms discrepancy. These parameters are provisional development-trial guardrails, not universal thresholds established by a large labelled dataset.

## Persistence and scope

Use cache directory karaoke-vocals-preview-v3 and recipe mms-int8/demucs-source-3. Read valid v2, then v1, then ordinary caches while learning v3. Never delete older preview files on source mismatch. Saved-sync listing, deletion and refresh cover all three preview versions. The feature stays within the existing opt-in, debug-only vocal trial; no new model or separation pass is required by the retry.

Baby Flower's approved local four-word reference remains a manual correction. Preserve it separately from automatic comparison output; it must not be described as an automatic alignment result. General durable user corrections are outside this change.

## Verification

Red-first regressions cover preservation of frontend animated spans, a supported earlier CTC entrance, and rejection of an early noise frame that discards a supported hold. Additional checks cover strong contrary audio, good/early entries, short and ambiguous windows, vocalization exclusions, capture coverage and cache fallback.

Native recording comparison uses karaoke_entry_align with PALETTE_SOURCE_GUIDE=1; unconfigured runs retain the existing entry baseline. Outputs are exclusively created. The research report compares token ownership and complete word payloads, and prints timing metadata only. Model assets/audio/lyrics/caches stay local. Source disagreements and listening-ground-truth errors are reported separately.

### Results

Frontend: 65 passing tests. Rust: 145 passing, four local-asset tests ignored (140 passing before this change; five added). Clippy across all targets and the production frontend build passed. Independent review identified weak and strong isolated-noise counterexamples; both failed in the native regression before the support guard and pass after it. The existing independent CTC oracle continues to pass.

Five retained separated recordings contain 207 lines and 1,622 words. Sienna, Free and Happy retain identical complete word/subword payloads (109 lines, 936 words). Baby Flower changes one line's first-word entrance, 167003 to 163901ms (3.102s earlier); the other three boundaries remain unchanged. This matches the approved first entrance but does not solve the remaining held-note boundaries. Bamsopoong changes two repeated-na lines: entrances 59469 to 57944ms and 113664 to 112241ms; interior boundaries also move. Those two changes are source/acoustic-supported candidates, not user-confirmed accuracy improvements. The final mean-plus-mass guard was rerun natively on all three changed rows and retained the candidate payloads. No further vocal separation was needed for these retries.

The local development app was rebuilt and reopened with the trial enabled. Baby Flower's approved four-word timing was carried into the local v3 cache with explicit manual-reference provenance; it remains separate from the automatic experimental output. Model files, recordings, lyric text and local cache contents were not committed.
