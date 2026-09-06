# Lyric sync status

The lyric header owns a neutral 15px authored morph icon. Its three cubic strokes morph only on state changes; it never runs a second ambient animation beside the waveform. Hover, focus, or click reveals the explanatory chip. Escape dismisses it before Focus handles Escape. Reduced motion disables icon and chip transitions.

## States

- Waiting: no eligible recording yet, interrupted capture, or incomplete listen. Explain when a fresh start is needed.
- Learning: the current track owns the recorder. A paused player gets paused copy.
- Processing: a captured listen is being aligned. Revisiting the track must not re-arm capture or overwrite its worker status.
- Saved: cached word timing is present or persistence has completed.
- Failed: alignment, coverage, audio, or persistence failed. Text failures explain that replay alone is insufficient.

The backend keeps 64 keyed, in-memory results. The frontend polls its current key every two seconds only while the lyric surface is visible; late replies after a track change are ignored. Results are session status, not durable job history. Cache data remains the authority for saved timing after restart. Worker completion may replace processing with retry guidance, while an interrupted replay may not.

## Numeric lyric compatibility

Single digits receive English spoken targets only in ASCII lines with multiple explicit English context words. The exact English rebus 4sho is also supported. Ambiguous bare digits, multilingual lines, and multi-digit values are not assigned guessed English pronunciations. Display strings stay original. Detail revision 2 invalidates older caches containing digits; unrelated acoustic timings remain readable.

No alignment speed or incremental-save claim is introduced. The icon exposes the existing full-listen pipeline and its outcomes.