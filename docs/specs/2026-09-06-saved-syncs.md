# Saved syncs

Click the lyric sync icon to open a compact library popover. Keep Palette's existing typography and artwork treatment. Show the current song first, then recently saved timing. Each row offers Refresh and Delete through authored hover/focus icons.

Refresh schedules learning on the next full listen; existing timing remains available until the replacement saves. Refresh remains available for explicit retry after a failed attempt. Delete hides all timing variants for that song and immediately reloads line lyrics in open views. Undo restores the hidden timing while the panel remains open and no later action supersedes it. A future full listen can learn deleted timing again.

The native library scans current, previous vocal-trial, and baseline caches without pruning files. Rows must pass the same version, recipe and exact-lyrics validation as playback. Song identity/art comes from local history; unmatched entries have a neutral fallback. This is local-device storage.

Policy is persisted atomically in sync-library.json. Per-song revisions prevent a worker started before Refresh/Delete/Undo from publishing over that action. Failed-listen suppression is revision-scoped. Sync completion and policy mutation serialize only the final save, never model inference. The frontend reloads on library changes and successful alignment, with request generations guarding track changes and late responses.

Validation: native policy and non-destructive cache-list tests cover stale jobs, delete/Undo, refresh preservation and source mismatch. Browser mock interaction checks cover refresh, delete, Undo and Escape. Full frontend tests, Rust unit tests, build and Clippy run before opening the PR. This PR is stacked on the vocal-entry trial; it does not change the alignment algorithm.
