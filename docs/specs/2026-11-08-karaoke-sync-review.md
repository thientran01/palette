# Karaoke: a second look at every line (2026-11-08)

Thien: word sync is "so close", and each word on its own is good enough,
so what's missing is something that checks the work and fixes it, "like
one more person looked at it closely."

Scored on JamendoLyrics (`examples/karaoke_bench.rs`, mix pipeline, 79
songs, 21,580 words, stamps on time), run in a Linux cloud container that
matched Thien's PC baseline (37 / 125 ms median / p90, 95% within 300 ms).

## Check: how sure is the model of each word?

The decoder keeps the single best CTC path. Where two placements of a word
score almost alike it commits to one of them on a hair. Replaying Thien's
own dumps on another CPU moved 2–20 words per song by up to 3 s, all on
adlibs in parentheses, repeated phrases ("끝없는 이 heartache (끝없는 이
heartache)") and vocables.

`sync_review.rs` runs CTC forward-backward over the emissions the path
came from. A word's **support** is the posterior probability that its
first letter starts within 150 ms of the frame the path chose. It costs
about as much as the Viterbi pass and no extra model run.

| support | share of words | more than 300 ms off |
|---|---|---|
| < 0.1 | 1.3% | 64% |
| 0.1–0.3 | 2.5% | 52% |
| 0.3–0.5 | 2.7% | 33% |
| 0.5–0.7 | 3.6% | 21% |
| 0.7–0.9 | 6.0% | 11% |
| ≥ 0.9 | 84% | 0.8% |

A line with any word under 0.5 is **doubted**: 18% of lines, holding 78%
of the words that are more than 300 ms off (846 of 1,091).

## Fix that didn't work: the posterior median

Moving doubted lines to each letter's posterior-median start (the
least-expected-error placement, and much steadier under small emission
noise) made the set slightly worse: p90 125 → 133 ms, late bias +11 → +24
ms. Doubt doesn't say where the word really is.

## Fix that works: a second ear on doubted lines

A doubted line is heard again on separated vocals (Kim Vocal 2, the
separator the vocal trial uses) for just that stretch of the song
(`VocalSeparator::vocals_window`, with a second of context each side).
That gives three decodes of the line's window: the mix, the vocals, and
both ears' emissions mixed frame by frame (`mix_ears`: a letter either
ear heard keeps at least half its probability, so the path can take each
part of the line from whichever ear heard it). The decode with the highest
geometric-mean support keeps the line (`surest`); one lost word counts
against a line more than a plain mean allows.

On the 596 doubted lines (3,919 words), with no limit on separation:

| rule | lines replaced | more than 300 ms off |
|---|---|---|
| keep the mix | 0 | 21.6% |
| always the vocals | 596 | 14.5% |
| always the blend | 596 | 14.7% |
| vocals if their mean support is higher | 464 | 14.0% |
| …geometric mean | 452 | 13.7% |
| …and the mix's sure words (≥ 0.9) stay within 300 ms | 435 | 13.6% |
| **surest of mix, vocals and blend (shipped)** | 502 | **12.7%** |
| …with the sure-words check | 492 | 12.9% |
| …with a product blend instead (both ears must hear) | 475 | 14.0% |
| the best of the three every time (oracle) | 250 | 7.8% |

The blend beat the two-way pick on 18 songs and lost on 6 (bootstrap over
songs: 6 to 58 fewer bad words, 90% interval). The sure-words check was a
wash: it blocked as many right fixes (a mix sure of the wrong copy of
"wrong concept wrong concept") as wrong ones, so it went.

### Stereo or mono

The recorder can keep the capture's stereo for the separator. On 16 songs
(227 doubted lines, 1,663 words, the two-way rule), more than 300 ms off:
keep the mix 22.6%, mono second ear 17.8%, stereo second ear 14.1%. Stereo
fixes 1.75 times as many words, so the recorder keeps it whenever the
separator is installed (about 46 MB for four minutes at 48 kHz, dropped
after the align).

## Budget: fourteen separator passes

The job has to finish while the next song plays, or `ALIGNING` skips
learning that song. Kim Vocal 2 works in passes of 5.75 s of audio, about
6.4 s each on Thien's PC (pulse.log, 2026-10-31: whole songs of 177–243 s
in 197–266 s); the align itself takes 10–41 s.

Doubted lines are a quarter of the audio, but separated one window at a
time each pays its padding and a part-filled pass: a median song would
cost 31% of a whole-song separation (p90 79%). `plan_second_look` lets
windows that overlap, or that cost no more heard as one, share a stretch
(median 27%, p90 58%), and spends a fixed number of passes on the least
sure lines first. Bad words fixed by the shipped rule (simulated on the
per-line results above; the measured 10-pass run fixed 247 against 250
simulated):

| passes per song | 4 | 6 | 8 | 10 | 12 | 14 | 16 | no limit |
|---|---|---|---|---|---|---|---|---|
| fixed | 123 | 163 | 232 | 250 | 275 | 306 | 320 | 342 |
| songs at the cap | | | | 36 | 31 | 19 | 18 | |
| longest job on Thien's PC | | | | 1:53 | 2:06 | 2:19 | 2:31 | |

Fourteen passes (about 90 s on Thien's PC) keep 90% of the gain (89%
measured, below). The longest job (the slowest align seen, 41 s, the
extra decodes and 14 passes) is shorter than every song in the set (2:28
or longer). A busy
PC can run several times slower (one whole-song separation took 527 s
instead of about 210 s), so the worker's ear starts no stretch more than
two minutes into the align, and stretches go least sure first.

Tried here and dropped: packing every stretch into one separator input
(simulated 281 fixed at 10 passes: a pass's worth of padding and slack
saved per song, for a new batch API and joins inside a pass).

## Result

The whole set (79 songs, 21,580 words), the mix pipeline before and after:

| | median / p90 | within 100 ms | within 300 ms | more than 300 ms off |
|---|---|---|---|---|
| before (`mms-int8/2`) | 37 / 125 ms | 87.2% | 94.9% | 1,091 (5.1%) |
| 10 passes | 36 / 110 ms | 88.5% | 96.1% | 844 (3.9%) |
| **14 passes (shipped)** | **36 / 107 ms** | **88.9%** | **96.4%** | **781 (3.6%)** |
| no limit | 36 / 107 ms | 89.0% | 96.6% | 744 (3.4%) |

At fourteen passes the second ear heard 502 of the 596 doubted lines and
replaced 436 of them; words more than 300 ms off on doubted lines went
21.6% → 13.7%. That is 310 words fixed, 89% of the 347 that no limit
fixed. The median song used 10 passes, 22 used all 14 and 5 had nothing
to doubt.

50 songs have fewer words off, 19 the same and 10 more: the same ten, by
the same counts, as at 10 passes. The worst two have 4 more each, where a
mix that was right but unsure ("no no no no no", "es tarde") lost to a
surer vocals decode that put the line a second late. By the bench's own
rule (median 10 ms or p90 50 ms worse) one song is worse: "Pluie d'entre
deux saisons", p90 585 → 1,061 ms, from its closing "ah ahha ah ah",
which every version misses (19 → 20 words off, as at 10 passes and with
no limit).

## Where it runs

- `Knobs::MIX`: `review_below` 0.5, `review_blend`, `review_passes` 14.
  Recipe `mms-int8/3`; `mms-int8/2` syncs stay on screen until the song's
  next full listen relearns them.
- The second ear runs when `Kim_Vocal_2.onnx` sits beside the aligner's
  model (`acoustic::second_ear_installed`). Without it the check still
  runs and changes nothing.
- `karaoke: model reused=…, second ear on Xs of Ys (N passes)` in
  pulse.log says how much of each song the second ear heard.

## Thien's K-pop dumps

Five of Thien's own captures (`karaoke-dumps`, mono, since a dump doesn't
keep stereo) replayed with the second ear at 14 passes. There is no
ground truth, so this shows how much changes, not whether it's right.

| song | lines | doubted | heard | replaced | words moved > 300 ms |
|---|---|---|---|---|---|
| NMIXX – SPINNIN′ ON IT | 56 | 30 | 14 | 9 | 35 of 601 |
| EXO – Let Me In | 42 | 14 | 9 | 5 | 34 of 512 |
| wave to earth – love. | 24 | 13 | 7 | 6 | 17 of 198 |
| KISS OF LIFE – Nobody Knows | 44 | 16 | 16 | 11 | 24 of 337 |
| tripleS – Baby Flower | 69 | 31 | 21 | 12 | 32 of 423 |

K-pop doubts far more than the test set: 44% of lines (104 of 235)
against 18%. Every song used all fourteen passes, so the ear reached 67
of the 104. That is where a bigger budget would pay off next.

## Tried and rejected

- Posterior-median placement (above), and a flatter posterior
  (temperature 1.5: p90 125 → 208 ms).
- Doubt at 0.3 or 0.7 for the median fix: all worse than the shipped path.
- Keeping the mix's sure words in place, a product blend, skipping
  vocable-only lines (15.1%), and judging lines by their least sure word
  (13.5%) or median word (13.9%) instead of the geometric mean.
- Letting every decode's posterior judge every decode (on the 406 lines
  heard at 10 passes: 13.9% against 14.4% for each decode's own support,
  inside the noise between songs), and judging by the blend's posterior
  alone (also 13.9%).
- Separating each doubted line on its own (padding and part-filled
  passes, above).
