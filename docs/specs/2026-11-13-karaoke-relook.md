# Karaoke: looking again for lines a stamp clipped

2026-11-13. Thien: songs with a high sync % "oftentimes" have one or two
lines that are "completely and utterly wrong".

## The problem

Each line is decoded only in its own window, from 500 ms before its stamp to
500 ms after the next line's stamp (`recorded_window_margin`, capped by
`Knobs::window_next`). One stamp a second or two off in the lyrics leaves
part of the singing outside the window, and CTC still has to place every
letter, so it squeezes the line into what is left. `late_stamps` handles a
whole file stamped late; nothing handled a single line.

JamendoLyrics, mix only, every sixth line given one fault (the rest on time).
"Horrible" = the line's median word more than 700 ms off, or half its words
more than 1 s off:

| fault on the line            | horrible lines | of those, doubted |
| ---------------------------- | -------------- | ----------------- |
| none                         | 7 of 544       | 7                 |
| stamp 700 ms late            | 31             | 23                |
| stamp 1.5 s late             | 332 (61%)      | 213               |
| stamp 2.5 s early            | 66             | 54                |
| (line before that early one) | 103 of 1,076   | 41                |
| line missing from the LRC    | 27 (line before it) | 21           |

An early stamp hurts the line before it most: that line's window now ends
500 ms after the early stamp, in the middle of its own singing.

The review can't see most of this. Support (`sync_review.rs`) is the
posterior share near the chosen frame, so it measures whether a word has
another plausible place *inside the window*, not whether the window holds
the line. Squeezed words with nowhere else to go are often sure. With the
1.5 s fault, 119 of the 332 horrible lines had no word under 0.5, so no
second ear and no extra listen ever touched them, and the sync % barely
moves: two bad lines of 45 cost it at most 4 points.

Thien's 34 newest saves (staged 2026-11-13): 43 of 1,443 lines (3%) have
their first word within 60 ms of the window's start or their last word
within 60 ms of its end, which is this signature. Only 18 of the 43 hold a
word under 0.5.

## The change

`AcousticAligner::relook`, between the first pass (and `late_stamps`) and
the second ear. A line is looked at again when the first pass doubted it
or it is jammed: its first spoken word starts, or its last ends, within
`JAMMED_MS` (60 ms) of its window's edge. Its new window runs from 200 ms
(`RELOOK_MS`) before the end of the line ahead to 200 ms after the start of
the line behind, when those neighbours are sure, wherever the stamps put
them. A side with no sure neighbour stays put, or reaches
`RELOOK_REACH_MS` (2.5 s) further when the line is jammed on that side.
The line keeps the new decode only if the model is surer of it
(`LineDecode::score`, the same judge as the second ear's). A window that
moved less than 200 ms in all is skipped. The second ear then hears
whichever window won.

Cost: one more model run per doubted or jammed line whose window changed,
about a tenth of a second each. Re-decoding a saved take (`redecode`)
skips a window the take doesn't hold and keeps the line as it was; a live
listen records the new window in its take.

## Results

JamendoLyrics, 79 songs, mix only (no second ear), words more than 300 ms off:

| stamps                     | before | after  | horrible lines | songs better / worse |
| -------------------------- | ------ | ------ | -------------- | -------------------- |
| on time                    | 5.02%  | 4.55%  | 49 → 47        | 19 / 5               |
| one in six 700 ms late     | 6.45%  | 5.51%  | 87 → 70        | 26 / 4               |
| one in six 1.5 s late      | 16.23% | 9.02%  | 396 → 201      | 75 / 0               |
| one in six 2.5 s early     | 13.79% | 8.90%  | 201 → 131      | 62 / 0               |
| one in six missing         | 5.77%  | 5.50%  | 68 → 69        | 15 / 5               |

"Better / worse" counts songs that moved by more than two words. The worst
song with on-time stamps lost 8 words (Embers, 189 words), the best gained
14.

Tried and dropped: taking the new decode whenever it was looked for
(on-time stamps 4.64%, 10 songs worse, a missing line 5.92%).

## Saved songs

`detail_v` 7 (written with the second ear installed, like 4 to 6). A save
below it with a jammed line (`karaoke::jammed_save`,
the same 60 ms test against its stamps) relearns once on its next full
listen, and that listen replaces it instead of merging, because a jammed
line is often sure and a merge never trades a sure line. Saves with no
jammed line stay as they are.

## Left

- A line whose stamp is off and whose neighbours are both doubted only
  reaches 2.5 s, and only on a jammed side. Half the 1.5 s-late lines that
  were horrible still are.
- Text that isn't sung where the line is stamped (a wrong lyric) comes out
  sure 13% of the time (a swap test: 70 of 544). A line-level highlight
  for hopeless lines is the next change.
- Backing vocals in parentheses: 7% of Thien's lines, 53% of them still
  unsure after all listens, against 17% of other lines.
