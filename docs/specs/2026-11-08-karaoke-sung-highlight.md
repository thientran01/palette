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

- Letter onsets come from one cursor per row (`src/lib/lineCursor.ts`,
  the cursor prototype A drew its wipe with), in letters. It passes every
  word onset exactly; between onsets it is a monotone cubic
  (Fritsch-Carlson): continuous speed, never backward, no overshoot.
  Inside a phrase (next onset within 220 ms of a word's end) it runs
  straight on; before a longer rest it ends where the word's sound ends.
  A word gets at least 55 ms per letter. Held words with letter
  checkpoints are keyed where each checkpoint's plateau ends (the same
  reading as `sampleFill`), so the held letter rises slowly. Words sung out
  of order (a backing reply) keep their own fill.
- A rise starts 80 ms before its onset and takes 200 ms on a smoothstep:
  no jolt when a letter starts moving or lands, half up 20 ms after it is
  sung and fully up 120 ms after. Neighbouring rises overlap into one
  wave. A held letter rises as quickly as any other; the hold shows as the
  next letter waiting, and as the pitch sway.
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

Smoothness (2026-11-09): Thien then found it "too bumpy" between
syllables and letters. Each word had timed its own letters (short words
packed into the 90 ms attack, then a pause; held words stepping on
checkpoints) and each rise was an ease-out that started at full speed.
The row cursor and the smoothstep cut the frame-to-frame change in how
fast the line lights (mean |Δv| / mean |v| of the summed `--lit`) from
0.24–0.31 to 0.08–0.12 on Thien's five saved syncs.

Speed (2026-11-09): with rises that lasted the whole gap to the next
letter (a held letter's whole hold), Thien found the rise "doesn't happen
fast enough and it looks weird relative to the beat or the actual
singing". Rises are now a fixed 200 ms, half up 20 ms after the onset
(it was 60 ms for quick letters and far later for held ones). The same
measure is 0.18: still under the 0.24–0.31 of the first build, and the
cursor keeps letters evenly spaced.

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
