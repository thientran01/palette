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

The panel says how good each saved sync is, from the review's own numbers (`docs/specs/2026-11-08-karaoke-sync-review.md`). Those were computed during the align and then thrown away. Now each word keeps its `support`, rounded to two places, plus `reheard` when the second ear listened to its line again. A file without them (`detail_v` below 3) still displays, and the next full listen relearns it, as the mms-int8/4 recipe already does.

- **Words on time** is an estimate: each word's support maps to the share of JamendoLyrics words at that support that landed within 300 ms (0.36 under 0.1 up to 0.992 at 0.9 or more), averaged over the song. It is shown as a whole percent and never reaches 100.
- **Line map**: one tick per lyric line (blank stamps and repeated stamps skipped). Sure lines (every word at 0.9 or more) are short and faint, loose lines are mid, and unsure lines (a word under 0.5, what the review doubts) stand full height in full ink. Lines with no word timing are hollow. The ink is neutral, nothing on it moves, and it has no playhead.
- **Second listen**: lines re-heard on isolated vocals, and how many came back sure enough not to be doubted.
- **Least sure** sorts by the estimate, with unchecked songs last.
- The current song's card replaces the old status block. The status line remains while a song is waiting, learning or failed. Relearn sits beside Record for research.
- The panel reaches the window's bottom edge (at most 560 px in focus mode) instead of 65vh. It covers the transport while open. The mode cluster hides under it, because the panel sits in the text plane's stacking context below the cluster's z-20.
