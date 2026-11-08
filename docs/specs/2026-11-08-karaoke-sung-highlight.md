# Karaoke: a highlight that sings (2026-11-08)

Thien: the sync is "so good that it makes the words jump by syllables
really fast. There's not much styling or smoothing to conceal the jumps."
Then: "emphasize the singer's voice and the way they sing ... emotion or
dragged notes or vibrato. I want to make the karaoke lyrics feel like the
singer is really singing."

## Why it jumped

The timing wasn't the problem; the drawing was. Each word had its own fill:

- short Latin words flashed in 90 ms (`WORD_ATTACK_MS`);
- held words followed the aligner's letter checkpoints, which crawl
  through a hold and then snap (Laufey "kiss,": the last 40% in 20 ms);
- Korean is one word per syllable, about 100 ms each;
- every filling word lit a white sheen, and every completed word fired a
  260 ms halo (a glow, which the design rules exclude anyway).

On Thien's own syncs the wipe's leading edge moved 6 px or more within a
single 60 fps frame on 2.8% (Laufey "Haunted") and 3.9% (tripleS "Baby
Flower") of frames, up to 24 px at once.

## Prototypes

Rendered next to the shipped wipe with the song audio, from Thien's saved
syncs and karaoke dumps (`/mnt/project-files/karaoke-motion/`, harness in
`prototype/`):

- **A · Glide**: one cursor per row (below). Jumps of 6 px or more fell to
  0.1–0.4% of frames, about 10 px at most.
- **B · Sung**: Glide, and held notes are paced by the voice's letters,
  lift with loudness and sway with the sung pitch (vibrato, slides).
- **C · Rise**: no wipe; letters brighten and float up as they're sung,
  and held letters ride the pitch.

## The row cursor (`src/lib/lineCursor.ts`)

The cursor runs through the row in px, as if its words sat on one line.
Every word onset is a keyframe at that word's left edge, so onsets are as
accurate as before. Between keyframes it is a monotone cubic
(Fritsch-Carlson): continuous velocity, never backward, no overshoot.

- Inside a phrase (the next onset within 220 ms of a word's end) the
  cursor runs straight on from onset to onset, so syllables don't
  stop-start. Before a longer rest, it ends where the word's sound ends
  and waits.
- A word gets at least 55 ms per letter, capped at the next onset.
- Held words (450 ms or more) with letter checkpoints are also keyed on
  them, where each plateau ends (the same reading as `sampleFill`): the
  wipe slows on the held letter, minus the final snap. The paced end can
  finish a held word up to about 100 ms after its sound ends; onsets
  never move.
- Words sung out of order (a backing reply printed after the lead, or a
  phrase-timed backing line) keep their own fill.

The driver draws each word's share of the cursor as `--wipe`, the light's
center in that span's own px, with an 11 px soft edge each side: −12 px
is dark and width + 12 px is fully lit, so a word the cursor has passed
has no half-lit tail while the row waits through a rest.
