# Karaoke: line timing for lines the review is sure are wrong

Thien, 2026-11-13: songs that sync well overall still have "one or two
lines that are completely and utterly wrong". The re-look
(`2026-11-13-karaoke-relook.md`) fixes the commonest cause, a wrong LRC
stamp clipping a line's window. This is the backstop for what's left.

## Problem

Word timing that's off by a second or more reads as broken: the fill
races ahead of the singer or lags behind them. The same line lit as a
whole on its LRC stamp reads as plain lyrics, which is what every song
without a sync already shows. For a line that is more likely wrong than
right, the plain line is the better display.

## Which lines

Each saved word carries the review's support (`sync_review.rs`): the
posterior share near the onset the decoder chose. A line's score is the
geometric mean of its words' support, each floored at 0.05 because the
saved figure is rounded to two decimals (as `karaoke::line_score`). One
doubted word in a sure line keeps the score high; a line where most words
are doubted scores low.

JamendoLyrics, 79 songs, mix only, re-look on, 3,382 lines:

| line score | share of lines | words > 300 ms off |
|---|---|---|
| under 0.2 | 1.3% | 46% |
| 0.2 to 0.35 | 2.0% | 46% |
| 0.35 to 0.5 | 1.8% | 34% |
| 0.5 to 0.7 | 4.2% | 23% |
| 0.7 to 0.9 | 12.7% | 9% |
| 0.9 and over | 78.0% | 1% |

With the second ear (separated vocals) the lines under 0.35 were 1.8% of
lines and 50 to 65% of their words were more than 300 ms off. In Thien's
34 newest saves (2026-11-13), 2% of lines score under 0.35, at most six in
one song.

The cut is 0.35 (`UNSURE_LINE_SUPPORT` in `src/lib/lrc.ts`): below it,
nearly half the words are badly off, and above it the share drops quickly.

## Change

`attachWords` gives such a line `alignmentFallback: "unsure"` and no
words, the same path a line with missing or invalid word timing already
takes. Frontend only: the saved sync is unchanged, the line still counts
toward relearning (`karaoke::wants_another_listen`), and a later listen
that hears it better brings its word timing back. Words without support
(older saves, line-only timing) are never judged.

## What's left

- The Lyric syncs panel still counts these lines as word-timed in its
  line map.
- Backing vocals in parentheses are forced into the line's timing and are
  half of the unsure lines in Thien's saves. Not singing them inline is the
  next change, and needs an A/B listen.
