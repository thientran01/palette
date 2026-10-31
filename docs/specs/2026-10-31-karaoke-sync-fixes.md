# Karaoke sync fixes: tail drift, repeated phrases, vocables (2026-10-31)

Scored, not felt. `src-tauri/examples/karaoke_bench.rs` replays the MMS
aligner on JamendoLyrics MultiLang (CC audio with human word onsets; EN, FR,
DE, ES, no Korean), with LRC-style stamps at each line's first onset, and
again with every stamp 200 ms early (`--lead-ms 200`), the way LRCLIB
stamps usually lead the voice. 71 songs, 19,240 words. A change was kept
only if overall median and p90 held and no song got clearly worse
(median +10 ms or p90 +50 ms), at both leads, on both pipelines.

## What failed

The first scoreboard's five worst songs shared three patterns:

1. **Tail drift.** The last line, and any line before a long instrumental
   break, searched up to 20 s of audio, and its words drifted seconds late.
2. **Repeated phrases.** A line such as "i've waited i've waited" skipped a
   faint first copy and matched a louder later one. The source-entry retry
   (`EntryMode::Guide`) found the right entrance, and the posterior-support
   guard then rejected it: a faint first phoneme scores low next to the
   loud repeat the free path took.
3. **Vocables.** "ooh ooh ooh ooh" gives CTC no consonants to hold. It
   compressed a 26 s phrase into 2 s, or slid a lone "oh" to the end of
   its gap.

## What changed (`acoustic::Knobs`)

- **Connected entry retry.** A retried entrance is also trusted when the
  line's next target follows it within 400 ms and both sit before the
  original entry. The noise shape the support guard exists for (a lone
  early blip, with the rest of the line still at the original place) still
  fails, and its test still passes.
- **Guide for the last line** (bounded by what was recorded), with the
  lateness gate lowered from 1500 to 700 ms.
- **Syllable window cap.** A line's window stops at stamp + 3 s + 400 ms per
  sung syllable when the next stamp is farther. Counting per syllable, not
  per word, matters: a held "intentandoooo" takes four seconds of a two-word
  line. Vocable-only lines are exempt, because a held "ooh" has no budget.
- **Vocable fallback.** A vocable-only line (not the last line) whose onsets
  stray more than 1.5 s from an even spread over 85% of its stamp-to-stamp
  interval takes the spread. A path that agrees keeps its own timing. The
  85% figure: the last vocable onset sits at a median 74% of the interval.

## Results (median / p90 ms, share within 300 ms)

| | lead 0 before | lead 0 after | lead 200 before | lead 200 after |
|---|---|---|---|---|
| mix (shipped path) | 37 / 141, 94% | 36 / 128, 95% | 36 / 141, 94% | 36 / 133, 95% |
| vocals (trial) | 36 / 106, 96% | 35 / 104, 97% | 35 / 107, 96% | 35 / 106, 97% |

Mean bias: mix +75 → +16 ms (lead 0), +47 → +2 ms (lead 200).

By class, p90 before → after, lead 0:
- **Tail** (last lines, and lines with more than 8 s before the next stamp): mix 5442 → 360, vocals 315 → 191.
- **Repeats** (lines with a repeated phrase): mix 662 → 533, vocals 479 → 409.
- **Vocables:** mix 2179 → 1115, vocals 1880 → 1115.

## Tried and rejected

- **Inter-onset gap prior.** A soft cost on words that trail the previous
  onset, through onset tracking in the Viterbi. It fixed some mid-line
  skips but broke held words and vocables.
- **Per-word window cap.** Broke held words ("intentando destacar").
- **Ungated or vowel-weighted vocable spread, and the spread on last lines.**
  Each regressed some song.
- **A 1000 ms vocable tolerance.** Better at lead 0, but it replaced a
  correct path on Intentando at lead 200.
- **Lateness gate 500 ms.** No gain over 700.

## Limits

- No Korean in the set. On ten unlabelled K-pop dumps, the change moved
  0.4–5.6% of words by more than 300 ms (`karaoke_bench --dumps`), nearly
  all of them late first words pulled back toward their own stamp.
- Still wrong: words in the middle of a line that skip to a later
  repetition, on separated vocals.
