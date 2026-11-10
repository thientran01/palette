# Karaoke: re-decode saved syncs without a re-listen

2026-11-10. Follows `2026-11-10-karaoke-listens-add-up.md`.

## Why

A RECIPE bump relearned every saved sync one full listen at a time, and
recipes 2, 3 and 4 were mostly decoder changes. The model's output for each
line window ("emissions") is everything the decoder reads besides the
lyrics and the time map, so keeping it lets a decoder change redo the whole
library in seconds.

## What is kept (`takes.rs`)

A take is one listen's decode input:

- the recording's length in samples and its time map;
- every line window's emissions on the mix, keyed by the window's exact
  sample range;
- for each line the review doubted, the second ear's emissions for the same
  window. The blend of both ears is recomputed from the two.

Log-probabilities are stored as u16 at 1/1000 nat (`SCALE`). On Thien's five
saved captures (1,380 words, cloud bench with AMX blocked), a re-decode from
the take put every onset and every re-heard line where the live listen did;
one word's end moved one frame and 12 supports changed by 0.01. 8-bit codes,
even with their precision spent near 0, moved 2-3% of words, up to 3 s on
Frank Ocean "Self Control". Sizes: 0.5-1.05 MB a song (2:17-4:09), about
300 MB for a 280-song library. Re-decoding takes 0.04-0.11 s a song.

`karaoke-takes/<key>.ptk` holds one take per listen merged into the sync,
oldest first, plus what they're valid for: `acoustic::EMISSIONS` (model,
separator, input scaling), a hash of the synced lyrics, and the sync's
`listens` when written. Own folder and own 500-file cap, so takes never
evict syncs. A pruned sync's take is removed with it; the sweep removes
takes whose sync is gone.

Not kept: the vocal trial's listens (whole-song separated vocals), the
legacy aligner, and any capture that included the whole device's mix
(audio.rs), whose emissions could spell out voice chat or notifications.
A listen merged into a sync whose earlier listens kept no take drops the
file: a partial history can't rebuild the sync.

## Re-decode (`karaoke/redecode.rs`)

90 s after launch, once, a background thread walks the takes. A sync is due
when its recipe is one RECIPE replaced (`EARLIER_RECIPES`), its take file
matches EMISSIONS, its lyrics and its listens, and it isn't deleted or
flagged to relearn. For each due song, on the sync job slot (ALIGNING, so
a listen that finishes meanwhile queues behind one short decode and runs
right after):

1. `AcousticAligner::decoder_only` (no model or runtime loaded) decodes each
   take with `redecode`: `align_inner` with `Knobs::MIX` and the saved
   emissions in place of the model's. A doubted line whose vocals weren't
   saved keeps the mix's decode, as when the second ear runs out of time.
2. The listens are folded oldest first with the same merge a live listen
   uses.
3. The same gates as a listen (coverage, collapse), then the voice tracks
   are carried over (below), and the sync is rewritten with its listens and
   detail unchanged, if the library didn't change it meanwhile.
4. `karaoke-ready` is sent, so a song on screen updates.

A missing mix window (the decoder moved a window: a new margin, syllable
cap, or an earlier-window retry) fails the song; its take is removed and
the song relearns on its next full listen, as before. Nothing is guessed.

### Voice tracks

The voice pass needs audio. A held word keeps its old track when its line,
place in the line and text match, re-cut to its new onset and length at the
same song times; frames outside what was measured are unvoiced.

## What still needs a listen

- Model, separator or input changes: bump `EMISSIONS`.
- Window changes (above), and detail_v bumps that need more of the second
  ear: the take has no audio to separate.
- Syncs saved before this change (no takes).

## Considered

- **Keeping the audio instead** (an outside review's suggestion): covers any
  change, but about 8 MB a song raw (FLAC would need a new encoder), a
  re-decode reruns the second ear for minutes a song, every recording stays
  on disk, and a faithful second ear would need stereo (46 MB). Put to Thien
  as a decision; emissions recommended.
- **8-bit codes**: see above.
