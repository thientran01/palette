# Saved syncs

Click the lyric sync icon to open a compact library popover. Keep Palette's existing typography and artwork treatment. Show the current song first, then recently saved timing. Each row offers Refresh and Delete through authored hover/focus icons.

Refresh schedules learning on the next full listen; existing timing remains available until the replacement saves. Refresh remains available for explicit retry after a failed attempt. Delete hides all timing variants for that song and immediately reloads line lyrics in open views. Undo restores the hidden timing while the panel remains open and no later action supersedes it. A future full listen can learn deleted timing again.

The native library scans current, previous vocal-trial, and baseline caches without pruning files. Rows must pass the same version, recipe and exact-lyrics validation as playback. Song identity/art comes from local history; unmatched entries have a neutral fallback. This is local-device storage.

Policy is persisted atomically in sync-library.json. Per-song revisions prevent a worker started before Refresh/Delete/Undo from publishing over that action. Failed-listen suppression is revision-scoped. Sync completion and policy mutation serialize only the final save, never model inference. The frontend reloads on library changes and successful alignment, with request generations guarding track changes and late responses.

Validation: native policy and non-destructive cache-list tests cover stale jobs, delete/Undo, refresh preservation and source mismatch. Browser mock interaction checks cover refresh, delete, Undo and Escape. Full frontend tests, Rust unit tests, build and Clippy run before opening the PR. This PR is stacked on the vocal-entry trial; it does not change the alignment algorithm.

## Activity and polish

The top of the library shows currently captured and processing songs. Listening progress is measured from captured audio relative to the remaining song duration at recording start; it does not advance on a wall-clock timer. Finishing is a named stage with no percentage because the alignment worker does not expose fractional progress. Completed or failed workers leave the active section through the alignment guard, including spawn failures. This is an activity view of the existing capture/worker pipeline, not a new deferred-job scheduler.

Activity polling reads memory once per second only while the popover is mounted and the document is visible. Saved cache enumeration remains event-driven. Row controls use 18px authored glyphs with stronger strokes, hover/focus visibility and press feedback. The inset scrollbar adopts the Settings rounded neutral thumb and removes the native arrow track.

## Unified sync control

The trigger's hover/focus label now names its action, Lyric syncs. The full state explanation lives inside the library under the current song, with its title and artist. Opening the library suppresses the label; the label is bound to the trigger rather than the entire popover wrapper. Expanded styling makes the active button recognizable.

All five states use an authored ribbon family with stable left/signal/right correspondence. Every state retains three visible two-cubic paths, so transitions reshape the same strokes without adding or removing parts. The same glyphs appear in the trigger, current-song explanation and active jobs. The existing 260ms in-out morph and reduced-motion behavior remain shared with Palette's icon system; there is no idle animation.

## Sync check (2026-11-09)

The panel says how good each saved sync is, from the review's own numbers (`docs/specs/2026-11-08-karaoke-sync-review.md`). Those were computed during the align and then thrown away. Now each word keeps its `support`, rounded to two places, plus `reheard` when the second ear listened to its line again. A file without them (`detail_v` below 3, or below 4 where the second ear also writes held-note voice data) still displays, and the next full listen relearns it, as the mms-int8/4 recipe already does.

- **Words on time** is an estimate: the average, over the song's checked words, of each word's chance of landing within 300 ms of the singing. The whole percent shown never reaches 100. Revised 2026-11-09 (below); the first version mapped support alone to a fixed share (30% under 0.1 up to 99.3% at 0.9 or more) and read high on the hardest songs.
- **Line map**: one tick per lyric line (blank stamps and repeated stamps skipped). Sure lines (every word at 0.9 or more) are short and faint, loose lines are mid, and unsure lines (a word under 0.5, what the review doubts) stand full height in full ink. Lines with no word timing are hollow. The ink is neutral, nothing on it moves, and it has no playhead.
- **Second listen**: lines re-heard on isolated vocals, and how many came back sure enough not to be doubted.
- **Least sure** sorts by the estimate, with unchecked songs last.
- The current song sits on a soft card that keeps its shape in every state. Its strip of line ticks fills in as the listen is captured (with "% heard" in the number's place), and then rises into the line map once the check lands. That is a one-shot, staggered height change. The status line remains while a song is waiting, learning or failed. Relearn sits under the summary.
- Other songs being synced are listed under "Also syncing". The current song's own job is shown on its card.
- Saved rows carry a 40 px version of the map, folded into 14 stretches, each drawn as its least sure line. The list header counts how many songs are checked.
- Record for research and Folder sit in a footer, apart from the song's story.
- The panel reaches the window's bottom edge (at most 560 px in focus mode) instead of 65vh. It covers the transport while open. The mode cluster hides under it, because the panel sits in the text plane's stacking context below the cluster's z-20.

## Words on time on hard songs (2026-11-09)

The first estimate was right on average (96.6% estimated, 96.5% measured over JamendoLyrics) and within 1.7 points per song, but the songs it scored lowest were worse than it said: the nine it put under 93% averaged 87.8% estimated and 85.8% measured, and Embers read 81% against 72%. Refitting the table, or cutting it finer, changed nothing. Two things support alone doesn't see did:

- **How hard the song is.** Where the first pass doubts many lines, words at every support level are on time less often. As the app aligns, with under 10% of a song's words on doubted lines, words at 0.9–0.99 were on time 99% of the time and words at 0.5–0.9 92%; with 30% or more, 95% and 83% (words under 0.5: 68% and 45%). A line counts as doubted by the first pass if the second ear heard it again or it still holds a word under 0.5, so the share is the same with and without the second ear, and the fit carries from one to the other.
- **A line that starts late.** A line whose first word lands well after its stamp has often found something else. Words after the first were on time 97% of the time on lines that started where the song's lines usually do, 77% on lines 300–700 ms late, 60% on lines 0.7–1.5 s late, and 1 in 19 on the five lines later than that. The words the check is sure of slip less (99%, 91%, 89%), so this term mostly lowers the loose words on those lines. Lateness is measured against the song's median line, because a lyric file leads or trails the vocal by a song-wide offset (Thien's saved songs: medians from -480 to +540 ms).

Each word's chance is a logistic of three terms (`sync_library::on_time`): its support bucket, minus 2.59 × the share of words on doubted lines (counted up to 0.6, about the hardest song in the set), minus 3.29 per second its line starts late beyond 300 ms of slack (up to 2 s).

| support | no line doubted | 30% of words on doubted lines | 60% |
|---|---|---|---|
| < 0.1 | 58% | 39% | 23% |
| 0.1–0.3 | 73% | 55% | 36% |
| 0.3–0.5 | 85% | 72% | 54% |
| 0.5–0.7 | 91% | 82% | 67% |
| 0.7–0.9 | 94.6% | 89% | 79% |
| ≥ 0.9 | 99.5% | 98.9% | 97.6% |

Measured on JamendoLyrics (79 songs, 21,580 words) aligned three ways: as the app does (second ear, stamps on time), without the second ear, and with every stamp 200 ms early. With the shipped constants:

| | error per song, before → after | within 3 points | songs scored under 93%: estimated / measured |
|---|---|---|---|
| as the app aligns | 1.69 → 1.42 | 67 → 68 of 79 | 87.8 / 85.8 → 84.7 / 85.8 |
| no second ear | 1.97 → 1.70 | 66 → 67 | 87.3 / 86.5 → 85.5 / 85.7 |
| stamps 200 ms early | 1.68 → 1.40 | 70 → 67 | 89.2 / 88.0 → 87.8 / 88.4 |

Easy songs got closer too (songs scored 97% or more: 0.99 → 0.69 points off). Embers reads 72% (measured 72%) and The Leader 73% (76%). Counting only words after each line's first, whose truth the benchmark's stamps don't give away, the errors are 1.93 → 1.58, 2.17 → 1.85 and 1.75 → 1.49. Without the late-line term the first row is 1.47 and the fifteen hardest songs still read 3.3 points high, against 1.9 with it.

**Corrected 2026-11-10.** The constants were fitted on an earlier set of these three runs, made on a cloud machine with Intel AMX. ONNX Runtime 1.22's AMX int8 kernel miscomputes one frame in 32 there, and far worse when the CPU is oversubscribed, which that early-stamp run was: it showed whole lines landing seconds late and early stamps costing 1.5 points of words on time. Neither is real. On the AVX512-VNNI kernel Thien's PC uses (the cloud bench now blocks AMX), early stamps cost nothing (96.5% on time either way). The table above is from clean runs and does not refit anything. A leave-one-song-out refit on the clean runs gives the same errors (1.43, 1.75, 1.37) and constants close to the shipped ones (2.2 for the song, 3.8 for lateness), so the shipped ones stay.

What it still can't see: an echo placed with full support on the wrong repeat (Mes Larmes, "les larmes" at the end of every chorus line: 95% estimated, 85% measured), and a song whose many loose words are loose but near (Christmas avec toi: 85% estimated, 92% measured). Tried and dropped on the first runs, each within the noise on top of the two terms: finer support buckets, the line's least sure word, neighbouring words' support, word and previous-word length, repeated words in a line, vocables, lines before a long gap, and whether the word's own line was reheard.

The fit reads the `support` and `reheard` the bench now writes beside each word's error (`--mix-only --ear stereo`, `--mix-only`, and `--lead-ms 200`).
