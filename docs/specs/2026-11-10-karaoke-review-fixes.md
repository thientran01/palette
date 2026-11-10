# Karaoke: three fixes from an outside code review

2026-11-10. Thien had another model review the sync pipeline at 1b8b699.
These are the findings in this area that held up against the code and his
log, and what changed. The rest went to other threads (partial listens,
Korean spelling) or are listed at the end.

## The second ear heard mono (H1)

`2026-11-08-karaoke-sync-review.md` chose stereo for the second ear: on 16
songs, more than 300 ms off on doubted lines was 22.6% keeping the mix,
17.8% with a mono ear and 14.1% with a stereo one. The recorder was changed
to keep stereo whenever the separator is installed (`Rec::stereo`), but the
capture thread still offered stereo only under `vocal_preview::enabled()`
(a debug build with an env var), and the first packet without it clears
`Rec::stereo`. So every release build separated the 16 kHz mono copy
upsampled to 44.1 kHz: no stereo image, nothing above 8 kHz.

`loopback.rs` now offers stereo on every packet and the recorder decides
per recording. `Rec::stereo` reserves four minutes up front, as `samples`
does, so the capture thread never stops to move a growing buffer. The job
summary in pulse.log ends `(N passes, stereo)` or `(N passes, mono)`. The
device-mix fallback (`audio.rs`) is still mono.

Saved syncs are not relearned for this. A song with a line still doubted
already learns again (`2026-11-10-karaoke-listens-add-up.md`) and gets the
stereo ear then; Relearn does the same for any other song.

## Short seeks and pauses bent the clock (H3)

A pair more than `align::SEEK_RESIDUAL_MS` (2.5 s) off the fit is a seek,
and the fit drops pairs more than 1.2 s off as jitter. Both were sized for
Apple Music, which reports whole seconds. A scrub of under 2.5 s, or a
pause that short while the capture kept delivering, passed: the fit either
split the difference or dropped one side, and the song saved with lines up
to a second or two off. Nothing gates on the fit's residual.

Thien's log has no case of it (96 saved fits, rms 3 to 26 ms, where one
would show as hundreds), so this closes a hole rather than fixing a known
bad sync. `karaoke::seek_band`: once a recording has four pairs and its fit
is within 50 ms rms with nothing dropped, the band is 500 ms. Spotify
always qualifies; the worst single pair in five dumps sat 41 ms off.
Otherwise the band stays 2.5 s. The two-strike rule is unchanged. What a seek does
changed in `2026-11-10-karaoke-partial-listens.md`: the audio before it is
kept as a partial listen.

## A merge margin smaller than rounding (M7)

`MERGE_MARGIN` was 0.01 in mean log support, but support is saved to two
decimals: 0.05 and 0.06 are one rounding step and 0.18 apart in log, and
0.00 and 0.01 are 2.3 apart under the old 0.001 floor. A line could change
hands, and a listen count as an improvement, on rounding alone. The floor
is now 0.05 (`SUPPORT_FLOOR`) and the margin 0.2, about a fifth more
support, which no single rounding step reaches.

## Late stamps (the review's notes on fix 2)

`2026-11-10-karaoke-late-stamps.md`. The review's version (an earlier
window only for a line that is doubted and pinned at the window's edge)
lost to doing nothing on the benchmark, and moving the whole song won.

## Checked and left

- H2 (a line stamped before the recording starts is decoded on clipped
  audio): real. 90% of recordings start within 0.5 s of the song's start,
  so it needs a first line stamped earlier than that. Fixed with the
  partial-listens edge rule (`2026-11-10-karaoke-partial-listens.md`).
- Offset plus slope when lining listens up: the fits above are tight enough
  that drift between two listens is a few milliseconds. A constant offset
  stays.
- An energy-envelope cross-correlation instead of sure words: not needed at
  this precision; the median over a dozen or more sure words is already
  within a frame.
