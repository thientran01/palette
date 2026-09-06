# Queue play-now boundary fix

User report: playing a song from Pulse history/Search can skip through Spotify playlist continuation and re-add those tracks as explicit queue entries. Spotify UI shows separate "Next in queue" and "Next from" sections; GET /me/player/queue does not expose that boundary.

## Implemented

- A target already first in Spotify's upcoming list needs one observed skip, without an append.
- Other targets are explicitly appended. Compare pre/post queue snapshots, with unchanged current track, to locate a unique insertion. Never use the original target's position in the mixed list as a skip plan.
- Reject unchanged/delayed snapshots, ambiguous adjacent duplicates, tail-only replacement and other queue edits. An accepted but unconfirmed append returns `queued`; it cannot trigger Next's fallback skip. A matching managed front is marked pending so the late feeder cannot append it again.
- Confirm each playback transition and known remaining queue before another skip. Stop on unexpected playback. Restore only intermediates whose departure was confirmed. On success, one prior explicit target copy is replaced by the played appended copy; other duplicates are preserved.
- The automatic feeder and fed-marker reconciliation share a gate with all play-now callers, including Search. On exit, suppressed observations are rearmed even for a gate timeout or already-playing no-op. Accepted-but-unobserved handoffs are persisted. After a 30-second settling grace, two fresh absences (checked at 5-second intervals) release a lost handoff without re-posting it, covering playback during app downtime. Jump exit restores the original track/position evidence so a no-op cannot manufacture a repeated-track arrival.
- Queue membership remains usable when positional data is incomplete; only positional jumps reject such snapshots.
- Diagnostic logs record planned steps and unconfirmed stops without logging tokens or song metadata.

## Evidence and limits

118 Rust library tests pass (16 new cases, 3 existing ignored); 47 frontend tests, frontend production build, Rust formatting and clippy pass. Cases are synthetic scenarios matching the reported behavior, not captured Spotify responses. No live playback success is claimed yet.

The public API cannot reliably label external explicit entries versus continuation. Insertion matching is conservative evidence, not a source-provided boundary. An ambiguous/late accepted append remains in Spotify's queue; the UI reports this rather than silently trying again. Concurrent edits by Spotify or another client remain possible; no cross-service transaction exists.

This PR addresses play-now traversal. It does not redesign the managed queue's URI-based row identity, remove-already-fed limitation, or Next semantics for a repeat of the currently playing track. Those audit findings remain separate follow-ups.

## Native check

With one explicit Spotify queue item and a selected song farther down "Next from" (not its immediate first entry), play the selected song from Pulse Search/history. Expected: only the explicit prefix is traversed/restored; playlist continuation is not promoted. Also try a target already first, a pre-existing explicit target, and a delayed/offline response. Confirm the `queued` message does not cause a second automatic feed. Keep this as a draft until native behavior is observed.
