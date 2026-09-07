# One-song research recording

The lyric-sync panel exposes Record for research, including for songs with saved timing. This replaces manual settings/cache intervention when collecting examples of timing errors.

## Behavior

- One explicit song request at a time. Retain an existing recorder for the same song; otherwise arm until playback is within the existing eight-second start window. The panel instructs the user to replay from the beginning and keep Palette visible.
- Reuse the music-app recorder, clock anchors, sample conversion and safety checks. Saved-cache and previous-miss eligibility cannot suppress an explicit request. No recording of unrelated tracks; remote playback and unsupported sources retain the normal restrictions.
- Show armed, recording (captured time and progress), saving, saved/partial, and interrupted states. Stop & save retains a partial capture; Cancel drops the research request and its active buffer. Track change/capture stop also saves the research buffer.
- A research recording does not run alignment, modify saved timing, or alter the persistent karaokeDump setting. Normal automatic learning continues on subsequent eligible listens.
- Save under app data/karaoke-research/<timestamp>-<song-key>. Include audio.wav, pcm.i16, lyrics.lrc, words.json, meta.json and research.json. The word snapshot follows playback's enabled cache source and deletion state. The manifest identifies the selected timing cache and whether the recording reaches the track end; it never presents old words as a new alignment.
- Keep the latest five recording folders. Open folder opens this fixed app-owned directory; never accepts a caller-provided path. No uploads or model downloads.
- Requests are memory-only. Restart disarms recording; captured files remain available via Open folder.

## Boundaries and review

The existing recorder owns audio and its lock; the research module owns explicit request identity/status and asynchronous file output. Lock ordering is recorder then research status. Unique request identities prevent a detached old capture from saving after cancellation or completing a newer request for the same song. The writer must claim the current request before writing. Saving blocks replacement/cancel until its result is known.

The frontend polls only while mounted/visible, rejects status responses overtaken by a user action, and prevents overlapping actions. Existing sync activity remains separate from research status.

## Validation

Research regression tests cover target-song eligibility, busy/repeated starts, cancellation/rearm identity, stale completion, playable WAV/PCM parity, partial recording labels, playback-cache selection and byte-for-byte preservation of saved syncs. An isolated browser flow opens the panel, arms the request, verifies replay guidance and cancels back to idle without page errors. Full Rust tests, all-target Clippy, frontend build/tests and independent review are required. A real song listen remains the native audio acceptance check.
