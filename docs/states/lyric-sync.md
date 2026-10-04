# Lyric sync status

The existing lyric-sync ribbon icon opens **Lyric syncs** directly from the shared top-right controls. It stays the same across sync states; live status appears inside the library. The widget keeps this seat across lyrics, album art, and queue; focus places it beside the queue and exit buttons. The icon follows the existing hover and keyboard reveal and stays visible while its library is open. The library fits inside the fixed widget footprint. Escape dismisses the library before Focus handles Escape and returns focus to the trigger. Reduced motion disables the entrance.

**Settings → Playback → Karaoke lyrics** defaults to on for existing installs. Turning it off renders the original full lyric lines and stops the word-wipe driver in both rooms; line highlighting, auto-follow, instrumental-break dots, seeking, and the lyrics/album toggle remain active. The preference persists through settings.rs, mirrors live between windows, and keeps saved word timing available for re-enabling.

## Control and preference coverage

- [x] The sync icon stays in the same seat across lyrics, art, and queue, including a lyric miss.
- [x] Focus has no control floating beside the lyric column.
- [x] The sync icon and saved library remain within the widget's existing hit rectangle.
- [x] Keyboard open, Escape, outside dismissal, and focus return are covered.
- [x] A failed preference update releases the busy guard and shows a retry message.
- [x] Startup snapshots cannot overwrite a newer live preference event.
- [x] Karaoke off leaves line highlight, auto-follow, and click-to-seek unchanged at both scales.
- [x] Re-enabling uses the existing word timing; disabling does not delete or relearn it.
- [ ] Native cross-window preference propagation and installed-app feel check.

## States

- Waiting: no eligible recording yet, interrupted capture, or incomplete listen. Explain when a fresh start is needed.
- Learning: the current track owns the recorder. A paused player gets paused copy.
- Processing: a captured listen is being aligned. Revisiting the track must not re-arm capture or overwrite its worker status.
- Saved: cached word timing is present or persistence has completed.
- Failed: alignment, coverage, audio, or persistence failed. Text failures explain that replay alone is insufficient.

The backend keeps 64 keyed, in-memory results. The frontend polls its current key every two seconds only while the saved-sync library is open; late replies after a track change are ignored. Results are session status, not durable job history. Cache data remains the authority for saved timing after restart. Worker completion may replace processing with retry guidance, while an interrupted replay may not.

## Numeric lyric compatibility

Single digits receive English spoken targets only in ASCII lines with multiple explicit English context words. The exact English rebus 4sho is also supported. Ambiguous bare digits, multilingual lines, and multi-digit values are not assigned guessed English pronunciations. Display strings stay original. Detail revision 2 invalidates older caches containing digits; unrelated acoustic timings remain readable.

No alignment speed or incremental-save claim is introduced. The icon exposes the existing full-listen pipeline and its outcomes.
