# Karaoke: partial listens save the lines they heard

2026-11-10. Thien approved splitting this off the second-listen thread. A
listen used to count only if it was armed in the first 8 s
(`ARM_NEAR_START_MS`) and reached 1.5 s before the end (`cache_complete`).
Everything else was thrown away, and a skip after 55% was even aligned in
full (up to about 7 minutes of separation and voice passes) before
`commit_recording` discarded it. Thien's pulse.log for 2026-11-10 has 392
recordings: 11 aligned and discarded as incomplete, 13 dropped at a seek, 1
at a delivery gap. Plays that started mid-song never recorded at all.

Now a listen that starts late, is skipped, or is cut by a seek, a pause or
a source change saves the lines it held whole. A song can be learned in
pieces, and the pieces add up through `combine_listens` like any listens.

## Which lines a listen holds whole

`acoustic::edge_cut`, in `first_pass`, on the window the aligner actually
uses (after the late-stamp shift and the syllable cap):

- **Start, every listen.** A line is left if the recording began less than
  250 ms before its stamp (`START_SLACK_MS`). Before this, a window that
  began before the recording was clamped to the first sample and the line
  decoded on whatever was left: a listen armed at 0:05 squeezed a line
  stamped 0:04 into the leftover audio and saved it (the outside review's
  H2). Plays from the top start recording 8 to 834 ms in (95% of the 392),
  so lines stamped from about a second on are heard on such a play.
- **End, partial listens only** (`whole_windows`). A line is left if its
  window, the next stamp plus 500 ms or the stamp plus 20 s, runs past the
  recording. A whole listen keeps decoding its last lines as before: the
  recording reached the song's end.
- A line stamped past the recording's end is left, as it always was.

The aligner reports the lines it left (`EdgeCut`, by the stamps the lyrics
carry) and they cost no model time and no second-ear passes. A partial
listen's take records `whole_windows`, so a re-decode leaves the same
lines.

## When a partial listen records and saves

| Event | Before | Now |
| --- | --- | --- |
| Play starts after 8 s | never recorded | records if the song has no sync, or has unheard lines and a sync it can merge into, and 8+ lines of 3+ words are still ahead |
| Skip or stop early | dropped under 55%, aligned and discarded over | saved as partial if worth it (below) |
| Seek (`karaoke::seek_band`) | recording dropped | cut at the last pair that agreed with the fit, committed as partial; the next beat may record again from the new position once its job is done |
| Lone seek strike at the end | dropped | cut the same way |
| Pause or stop | the process capture sends no packets, so the 400 ms gap check dropped the recording on resume | cut and committed when the status changes, unless the recording already reached the end (the last song of a queue) |
| Source switches away from the music app | dropped | cut and committed |

A partial listen is worth a job (`partial_worth`, checked before any audio
is decoded) when the acoustic aligner is installed, the time map has 3+
pairs, and the lyrics put 8+ lines of 3+ words inside the recording. After
the align it must pass, on the lines it held whole:

- `MIN_LINE_COVERAGE`, measured over those lines only, and
- `collapse_judged`: 8+ judged lines, so the deaf-capture guard
  (`collapsed`, which ignores songs with fewer) can speak.

A partial listen that fails either is not saved and is never a miss: it
heard too little to judge the song. A silent partial listen is dropped the
same way. It also never replaces a sync it can't merge into (an older
recipe), never saves on a song deleted in the library or waiting on
Relearn, and doesn't record mid-song on a song with no lines missing, so
unsure lines still wait for whole listens (most songs keep an unsure word
until their third, and re-hearing them on every mid-song play would start
a sync job almost every time).

A mid-song play of a song with nothing to learn is remembered (`DECLINED`)
until another track plays, a library action, or a save, so later beats skip
the disk reads.

## Bookkeeping

- `StoreFile::unheard`: stamps of lines no listen has held whole. Empty and
  left out of the file for anything a whole listen made, so every existing
  file reads as complete. Merging keeps only lines both the saved sync and
  the new listen missed. A whole listen's own unheard lines are only those
  cut at the start.
- `settled` (and so `has_file` and the Lyric syncs panel's "relearns")
  requires no unheard line stamped from 1.5 s on (`REACHABLE_FROM_MS`). An
  earlier one may never be heard whole and would relearn the song forever.
- `listens`: a partial listen leaves the count where it was (a song it
  starts gets 1) and never ends the extra listens when it improves
  nothing, because it judged only part of the song (`fold_listen`).
- On screen nothing changes: an unheard line has no words, and the
  frontend already falls back to line karaoke per line.

## Left for later

- The seek band itself is the second-listen thread's
  (`2026-11-10-karaoke-review-fixes.md`); this only turns what it catches
  into cuts.
- Partial listens merging into a sync saved under an older recipe (its
  lines could carry over marked unheard).
