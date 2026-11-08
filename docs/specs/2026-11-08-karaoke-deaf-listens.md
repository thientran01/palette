# Karaoke: don't save a listen that heard no vocals (2026-11-08)

Thien reported word karaoke that "goes through each line really fast with
no rhythm" on Baby Flower (tripleS), AMERICAN GIRL (Tiffany Day) and WATCH
ME (K/DA), whatever the word-lead setting.

## What the saved files showed

Every line's words sat in one or two 20ms CTC frames each, so a seven-word
line lasted about 0.3–0.8s, parked at an edge of its search window (stamp
−480ms, stamp +500ms, or just before the next stamp). That is the shape
the decoder produces when the emissions hold no voice at all: every letter
takes its minimum frames and blank takes the rest.

All three were learned between 2026-11-04 22:57 and 2026-11-05 00:35.
pulse.log shows Spotify's process loopback "never delivered" for that
whole stretch, so capture used the device-mix fallback, which heard near
silence. The `silence on …` guard checks only the recording's peak
(`PEAK_ABORT`, 1e-3), so one stray sound let a deaf listen through to the
aligner. Every one of the 21 syncs saved in that window collapsed, and
every sync saved before or after it is fine. So it is not the 1.0.11
decoder (#200, #202) or the clock (#216).

## Signature

For each LRC line with at least 3 spoken words: (last word end − first
onset) / words. Take the median over lines; judge only songs with at least
8 such lines.

On Thien's 292 saved syncs: 27 collapsed at 57–87ms (21 from that window, 6
older `mms-int8/1` files from about 2026-10-29). Every real sync is at
least 183ms (a fast rap verse). The threshold is 120ms.

## What changed (`karaoke::collapsed`)

- `commit_recording` refuses to save a collapsed alignment and says the
  listen didn't pick up the vocals. It is not recorded as a miss, so the
  next listen retries (the capture failed, not the song).
- `read_file` treats a collapsed saved sync like a stale one: the song
  shows line karaoke, the file is pruned, and `has_file` lets the next
  full listen relearn it. The saved-syncs library skips it too.

## Not fixed here

Why Spotify's process capture stops delivering. It recurred on 2026-11-06
and 11-07, where the peak guard caught it as silence. A capture-side check
(how much of the recording is audible, not its single loudest sample)
would stop these listens before the aligner runs, but needs recorded PCM
from a failing session to set its threshold.
