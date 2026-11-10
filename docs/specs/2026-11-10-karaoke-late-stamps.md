# Karaoke: lyrics whose timestamps run late

2026-11-10. Item 2 of the list in `2026-11-10-karaoke-review-fixes.md`.

## The problem

A line is looked for in a window from 500 ms before its stamp to 500 ms
after the next one (`recorded_window_margin`). The late-entrance retry
handles a singer who comes in well after the stamp. Nothing handled one who
comes in before the window opens, which is what a late LRC produces: the
whole file stamped behind the singing.

On JamendoLyrics with every stamp moved late (`karaoke_bench --lead-ms`,
mix only), words more than 300 ms off:

| stamps         | before | now    | songs better / worse |
| -------------- | ------ | ------ | -------------------- |
| on time        | 5.02%  | 5.02%  | never triggers       |
| 300 ms late    | 5.64%  | 5.24%  | 24 / 13              |
| 600 ms late    | 11.11% | 5.05%  | 62 / 1               |
| 900 ms late \* | 47.97% | 13.25% | 12 / 0               |

79 songs, 21,580 words; \* 14 songs, 3,644 words, which score 9.6% on
time. "Better / worse" counts songs that moved by more than two words. At
300 ms most lines still fit their windows, so there is little to win and
the scatter between songs is as large as the gain (the worst lost 20 words
of 1,818, a rap song that happened to score well unmoved).

With stamps 600 ms late, 70% of first words sit within 100 ms of the
window's first frame, most of them with high support: the review can't see
it, because inside that window the edge really is the best place. The
damage isn't only first words. The window's end is late too, so a line's
last words drift into the next line.

## What was tried

- **An earlier window for a line whose first word is pinned at the edge**
  (decode again from up to a second earlier, keep the surer decode, or the
  earlier one when its first word isn't doubted, or always): 19.5% to 18.3%
  at best on 14 songs with stamps 600 ms late, with more first words wrong
  than before. A wider window invites the previous line's tail, and fixing
  the start of the window does nothing for its end.
- **The same, only when the line is doubted** (the outside review's
  suggestion): worse than doing nothing, 19.9%. Pinned first words are
  mostly not doubted.
- **Moving the whole song** (shipped): 19.5% to 9.9% on the same 14 songs,
  where on-time stamps score 9.6%.

## What shipped

`acoustic::late_stamps`: after the first pass, take each line's first word
minus its stamp, over lines whose first word the review doesn't doubt. When
there are at least 8 and their median is 250 ms or more before the stamp
(`RECENTRE_MS`, half the window's reach), the song's stamps run late.
`align_inner` then runs the first pass again with every stamp moved by that
median, which moves both ends of every window. A first move can only see as
far as the old windows reached, so a second round may follow
(`RECENTRE_ROUNDS`). The second ear and everything after work on the moved
lines; the words still carry the stamps the lyrics came with (`line_t`),
which is what the page keys on. pulse.log: `karaoke: lines start Xms before
their stamps; looking again Yms earlier`.

On-time stamps never trigger it: a line's first word sits 0.1 to 0.4 s
after its stamp on Thien's five dumps (medians +141, +125, -118, +21, +262
ms), and exactly on it in the benchmark. So nothing changes for those
songs, `RECIPE` stays, and the page already shows a line from its first
word when that comes before the stamp (`wordTimedStarts`).

Cost: one more pass of the aligner over the song (not the separator), only
for a song that triggers it.

## Saved syncs

New saves write `detail_v` 6. An older save (4 or 5) whose own words say
the stamps run late (`karaoke::stuck_late`, the same rule on the saved
first words) relearns on its next full listen, and that listen replaces it
instead of merging: its lines sit sure at the edge of the old windows and
would outvote the fix. Installs without the second ear write 3 as before
and don't get this relearn.

## Not done

None of the five dumps has stamps this late, so this is a guard for a kind
of lyrics file, not a fix for a song Thien has reported. A single line
stamped late in an otherwise on-time song (4 of 182 lines on the dumps sit
at the window's edge) is still clipped.
