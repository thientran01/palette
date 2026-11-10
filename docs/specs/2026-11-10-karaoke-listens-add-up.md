# Karaoke: listens add up

2026-11-10. Follows `2026-11-10-karaoke-full-second-ear.md`.

## Why

Relearn threw the saved sync away and kept whatever the next listen produced,
so a worse capture could replace a better one, and a line one listen got
right was lost if the next got it wrong. Thien asked how to get close to
100% syncs. A "try harder" button has nothing left to switch on once every
doubted line gets the second ear (#248), so instead every listen now adds
to what the song already has.

## How

- **Merge** (`karaoke::merge_listens`). A new listen of the same lyrics
  under the same recipe (and, with the second ear installed, a saved
  detail of at least 4) merges into the saved sync line by line, keyed by
  the line's LRC stamp. A line's score is the mean log support of its
  words, as `LineDecode::score` ranks decodes. The saved line stays unless
  the new one scores more than 0.01 higher; a line only one listen placed
  comes from that listen; an unchecked line yields to a checked one. A song
  deleted in the library starts fresh.
- **Extra listens** (`wants_another_listen`). A saved sync with any word
  under the review's 0.5 is not settled until it has `MAX_LISTENS` (3)
  listens, so its next full listens learn it again. A listen that improves
  no line sets the count to the limit, ending the extra listens early. The
  count is the store's `listens` (missing in older files, read as 1). The
  Lyric syncs panel shows these songs as "relearns next listen".
- Relearn still forces one more listen at any count, and merges too.

## What it can and can't fix

The same track decodes nearly the same way on every listen. What differs is
the capture (noise, a glitch, Spotify's loopback dropping out), the time map
(a few frames), and the separator's stretch boundaries, which can tip a
borderline line between the mix, the vocals and the blend. So this repairs
lines a bad listen got wrong, and keeps a good listen from being replaced by
a bad one. It doesn't fix a line that every listen gets wrong the same way:
ad-libs, echoes, a phrase repeated within one line.

## Cost

Up to two more sync jobs for a song with an unsure line, only on full
listens that start near the beginning, and fewer once a listen adds
nothing. pulse.log says `karaoke: listen N of <title> improved K lines`
for each merge.
