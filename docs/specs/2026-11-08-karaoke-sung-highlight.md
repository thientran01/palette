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

- **A · Glide**: one smooth wipe cursor per row. Jumps of 6 px or more
  fell to 0.1–0.4% of frames, about 10 px at most.
- **B · Sung**: Glide, and held notes are paced by the voice's letters,
  lift with loudness and sway with the sung pitch.
- **C · Rise**: no wipe; letters brighten and float up as they're sung,
  and held letters ride the pitch.

Thien picked C.

## Rise (`src/lib/letterRise.ts`, `src/lib/wordWipeDriver.ts`)

Every word is split into letters (graphemes, so a Hangul block or an
accented letter is one) and each letter is its own inline-block span.

- A letter's onset is exactly as measured: where the aligner's letter
  checkpoints first reach its share of the word, or, without checkpoints,
  spread evenly across the word's displayed fill (`wordFillEnd`: the 90 ms
  attack for short Latin words, the sung span for syllable scripts and
  long words).
- Its rise starts 30 ms before the onset and lasts until the next letter's
  onset, but never less than 260 ms, with an ease-out. Quick syllables
  therefore overlap into one wave instead of strobing, and a held letter
  rises as slowly as it is held. Only how long a letter takes to settle is
  smoothed; when it starts is never moved.
- The driver writes `--lit` (0–1) per letter. The color mixes from the
  row's tone to `--word-bright` by `--lit`, and the letter sits
  `--drop` (1.75 px) × (1 − `--lit`) below its place.
- `--drop` applies only while the row is being sung (`data-singing`) and
  transitions with the line's own 220 ms scale-up, so unsung letters ease
  down as the line arrives instead of jumping.
- Plain colored text, no clipped gradient: the lyrics keep ClearType. No
  sheen, no halo (the design rules exclude glow).
- Backing voices printed after the lead keep their own onsets, so a reply
  rises over the lead without lighting future lead words.
- Reduced motion keeps the brightening and drops all movement.

Tuning (2026-11-09): the first build floated letters up 2.5 px and swayed
held notes ±2.2 px with the letters running as a wave. On his PC Thien
found it "moves the characters too much ... i want palette to feel pretty
minimal", briefly picked Glide, then asked for Rise toned down. A 1 px
float with a sub-pixel sway (word moving as one) was too little; he asked
for "in between", so the float is 1.75 px, the sway up to ±1.5 px and the
letter wave 9 ms.

Letters as inline-blocks lose kerning across letter pairs; the line's
layout never changes while it is sung.

## The voice (`src-tauri/src/voice.rs`, `src/lib/voiceMotion.ts`)

Held words also carry a voice track, made by the karaoke worker right
after the alignment and the second ear:

- The stretches around held notes (a word's onset to 120 ms past its end;
  neighbours within 2 s share one stretch) are separated by the same Kim
  Vocal 2 ear, the most held singing per pass first, within 10 passes
  (about 64 s on Thien's PC) and no stretch started 150 s after the align
  began.
- Every 10 ms: YIN pitch on the separated vocal (70–1000 Hz), and RMS
  loudness over 40 ms. Octave slips are folded back onto the note's
  median. Pitch is saved in cents from A440 with a 30 ms mean; loudness is
  scaled 0–100 against the song's held notes (20th–95th percentile) with a
  quick attack and slow release.
- Pitch from the mix instead of the separated vocal agreed on 62% of
  voiced frames for Laufey "Haunted" and 12% for tripleS "Baby Flower".

On the whole of "Haunted" (69 held notes) the budget voiced 29 in 9
passes; analysis took 0.4 s of CPU, so the separator is the whole cost.

On screen, a held note's risen letters move 1 px per 75 cents away from
the pitch 80 ms into the note (clamped to ±1.5 px, and eased in for notes
under about a second): vibrato is a gentle sway and a slide or a dragged
note a drift. Each later letter reads the pitch 9 ms earlier, a slight
wave through the word. Unvoiced frames hold
the last pitch, and the letters settle back over 260 ms after the note's
release. Short syllables never follow pitch, which would bring back the
syllable-rate jitter. Loudness is saved with the track but not drawn.

Saved syncs carry `detail_v` 3 once their held notes were listened to.
With the separator installed, an older sync relearns on its next full
listen (its words stay on screen meanwhile).
