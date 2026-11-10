# Karaoke: a second listen for every doubted line

2026-11-10. Follows `2026-11-08-karaoke-sync-review.md`.

## Why

The review's second ear was capped at 14 separator passes a song. The cap
existed to keep one song's sync job shorter than the next song: karaoke
runs one job at a time, and a listen that finished while a job was running
was dropped and had to be learned again on a later play.

On JamendoLyrics that cap kept 89% of the gain (781 words more than 300 ms
off at 14 passes, 744 with no limit). Thien's K-pop is different: it doubts
44% of lines instead of 18%, every one of the five replayed songs used all
14 passes, and the ear reached 67 of the 104 doubted lines. Pressing
Relearn didn't help, because a relearn runs the same capped pipeline.

## What changed

- **Queue.** A listen that finishes while another song's sync is running
  waits in a queue (`karaoke::QUEUE_MAX` = 2) and is aligned after it, on
  the same job thread. ALIGNING is set and cleared only under the queue's
  lock, so a listen queued as a job ends is never stranded. A job thread
  lost to a panic or a failed spawn gives up the queue; those songs learn
  on their next play. The Lyric syncs panel lists queued songs as
  finishing.
- **No pass cap.** `REVIEW_PASSES` is 0 (no limit). Neighbouring stretches
  merge whenever that costs no more passes, so a song costs about one
  whole-song separation at most: about 43 passes for four minutes, about
  275 s on Thien's PC. `EAR_DEADLINE` went from 120 s to 360 s and still
  stops a busy PC; lines left keep the mix's timing. The voice pass's
  deadline now counts from when the voice pass starts (90 s), not from the
  align, so a long review doesn't starve it.
- **Targeted relearn.** New saves write `detail_v` 5. A detail-4 save
  relearns on its next full listen only when it has a line the review
  doubted (a word under 0.5) that the second ear never re-heard, which is
  what the cap left behind. Other saves stay. `RECIPE` is unchanged, since
  the decoder and every line the old cap reached are the same.

## Cost

Background CPU on songs with many doubted lines, on the same two separator
threads. A queued listen holds its recording in memory (about 60 MB for
four minutes with stereo), at most two of them.

## Checking it

`karaoke: model reused=…, second ear on Xs of Ys (N passes)` in pulse.log
gives each song's passes; `align busy — … queued` and `align queue full`
say when the queue was used.
