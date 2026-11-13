# Karaoke: fixing a bad line by hand

Thien, 2026-11-13: the re-look and the unsure-line fallback help, but
"often times i can just see a horrible line that i can so easily fix, but
theres just no way for me to do so". He picked "tap it in, plus plain" on
the decision card over a nudge-only control and a plain-only switch, and
asked for it to be fast, easy and simple, with plain, direct copy.

## What the listener does

Right-click a lyric line (the widget's lyrics or the focus room). On a song
with a word sync the menu starts with the line's own items:

- **Tap in this line.** Palette jumps 3 s before the line's stamp (where
  the player can seek; Apple Music can't, so it waits for the line to play).
  The row draws plain with every word dim, and a caption says "Tap each word
  as it's sung". Each press of Space, or of the mouse anywhere over the
  lyrics, is a word's start and its release the word's end, so a long note
  is a long hold. Each tapped word turns to full ink and the caption counts
  ("3 of 7"). The last word's release saves the line ("Line fixed").
  - A line too fast to tap takes one long press from its first word to its
    last; its words spread over the press by their letters.
  - Any other count is a miss: "Missed a word. Try again", and the line
    plays again. Where the player can't seek, the tapping stops instead.
  - Tapping ends when it pauses: 1.2 s after the last release, and not
    before the next line's stamp, so a long rest between two words isn't
    read as the end.
  - No press by 4 s past the next line's stamp: "No taps heard", stopped.
  - Esc stops at any time (in the focus room, without closing the room).
- **Show this line plain.** The line lights whole on its stamp, like a line
  the review is sure is wrong (`2026-11-13-karaoke-unsure-lines.md`).
- **Undo fix**, on a fixed line: the aligner's timing shows again.

Words are what the line's spaces separate, so a Korean or Japanese line is
tapped by phrase, not by syllable (the aligner times those a syllable at a
time, about ten a second, far too fast to tap). A tapped phrase fills
evenly through its press.

A tap is placed on the song clock as the line highlight reads it
(`posClock.now()` plus the player's line lead). The word lead (how early
a word lights) is not added: it applies to tapped words exactly as to the
aligner's, so a fixed line lights as early as its neighbours.

## Where a fix lives

`karaoke-fixes/<key>.json` in the app's data folder, one file per song:
each fixed line's stamp and text, and its tapped words (none for plain).
The aligner's sync file is never touched.

Every time a song's words are read for display (`karaoke::load`, a listen
saving, a background re-decode, the Lyric syncs panel) the fixes are laid
over them (`karaoke::fixes::apply`): the aligner's words for a fixed line
are dropped and the fix's put in their place, marked `fixed`. So:

- Relearns, extra listens, partial listens and re-decodes never see a fix
  and can't undo it. It lays over every later sync of the same lyrics.
- A fix applies only while its line is in the song's timed lyrics with the
  same stamp and text. Lyrics that change leave it dormant, never
  misplaced.
- A song with no word sync (never learned, or deleted in the library)
  shows no fix.

Fixed words carry no review support, so they don't move the panel's
"words on time" estimate. The panel's line map draws a fixed line as `f`
(short like a sure line, darker ink) and its summary adds "You fixed N
lines by hand."
