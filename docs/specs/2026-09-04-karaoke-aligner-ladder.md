# Word karaoke aligner: prior + refinement ladder

**Date:** 2026-09-04 · **Approved by:** Thien (brainstorm session) · **Scope:** one PR (commits on `feature/word-karaoke`, PR #162), follows `2026-09-04-karaoke-data-plumbing.md`

## Problem

The shipped aligner (`align.rs`: vocal-band-ish energy envelope, rises out of
silence, cumulative-energy spread as the fallback) measures **median 498ms,
p90 1874ms, 10% within 100ms, bias +174ms** against Thien's tap-labelled
truth for TAEYEON "Blur" (343 tokens; scorer:
`cargo run --example karaoke_score`). The clock is not the problem (time map
residual 13ms). Decomposing the error on the truth set:

| oracle / model                                   | median | p90  | ≤100ms |
|--------------------------------------------------|-------:|-----:|-------:|
| current aligner                                  | 498    | 1868 | 10%    |
| shift each line so its first word is right       | 285    | 1750 | 28%    |
| true line span, aligner's inner order            | 127    | 753  | 46%    |
| true line span, uniform by syllable weight       | **44** | 413  | 61%    |
| no audio: stamp + 330ms, 3.1 syl/s, 0.9×window   | 181    | 530  | 30%    |

So: (1) inside a line, a flat syllable-weighted spread beats the audio
envelope; (2) the whole error is where the line STARTS and ENDS; (3) a
no-audio prior already beats the current aligner ~3×. Truth statistics:
first sung word is a median 330ms after the stamp (p10 −71, p90 +536); the
sung span fills a median 74% of the stamp window (p10 0.29, p90 0.96);
syllable rate 1.7–4.4/s (median 3.1), too variable for a fixed rate alone.
Worst class: a line followed by a long gap has its tokens spread to the next
stamp (9.7s on "이 모든 날 제대로 봐").

Decisions already made: the prior stays INTERNAL (Thien, this session) — word
wipes remain gated on a recorded listen; a silent recording (exclusive-mode
Apple Music) stays line-only. 2026-07-06 bar still applies: perfect or
nothing, so nothing here changes the frontend gating.

## Design (approved)

Three stages, each individually switchable so the scorer prints what every
rung buys. The shipped default is the highest rung that wins on BOTH truth
songs.

### Stage 0 — prior (no audio)

Per line with window `W = next_t − line.t` (last line: `W = min(8000,
pcm_end − line.t)`), syllable weights `w_i` from `token_weight`, total
`S = Σ w_i − w_last` (the last token's start is what's placed, not its end):

```
start = line.t + LEAD_MS                       LEAD_MS = 330
span  = min(S / RATE_SYL_S × 1000,             RATE_SYL_S = 3.1
            FILL × W − LEAD_MS)                FILL = 0.9
span  = max(span, MIN_SPAN_MS)                 MIN_SPAN_MS = 80 (existing)
t_i   = start + span × (Σ_{j<i} w_j) / S       (i = 0 → start)
end_i = t_{i+1}; end_last = start + span + LAST_HOLD_MS (250), capped at next_t
```

`start` and `span` are the `LinePrior` the refinements adjust. This stage
alone kills the gap-spreading class (span is capped by the window, never
stretched to the next stamp).

### Stage 0b — song calibration (added after the second song)

Heart To Heart (Mac DeMarco, 118 tokens) showed the fixed prior failing
acceptance: **374ms median, +359ms late bias** — its stamps sit a median
114ms before the vocal where Blur's sit 330ms. The lead is per song. The
per-line start detector is noisy (it lost to the prior on both songs) but
its MEDIAN across lines is unbiased on both (+5 / −24ms), so:

- `song_lead`: run `EnergyRise` on every line; `lead = median(start −
  stamp)` over lines where it fired, clamped to [−100, 800]ms; fewer than
  `SONG_MIN_SAMPLES` (6) firings keeps `LEAD_MS`. The flat prior then runs
  per line with that lead.
- `song_rate`: same idea with the end detector (`rate = median(S / span)`,
  clamped [1.5, 6.0]/s, spans < 300ms ignored). Measured and REJECTED for
  shipping: the end detector under-measures spans, so rates come out high
  and words land early (bias −236 / −338).

Matrix on both songs (2026-09-04):

| stages             | Blur median | HtH median | HtH bias | ships |
|--------------------|------------:|-----------:|---------:|:-----:|
| prior (fixed lead) | 183         | 374        | +359     |       |
| prior + energy     | 204         | 314        | −24      |       |
| prior + flux       | 254         | 615        | +584     |       |
| **song lead**      | **177**     | **160**    | −71      | **✓** |
| song lead + rate   | 208         | 271        | −338     |       |
| song + energy      | 224         | 402        | −310     |       |
| song + energy + end| 354         | 415        | −419     |       |

Worst lines under song lead: 851ms (Blur) / 722ms (HtH). Both songs meet
median ≤ 200ms and no line > 1.5s. `Stages::shipped()` = `song_lead` only.

### Stage 1 — start refinement (audio)

Search `[line.t − START_PRE_MS (300), line.t + START_POST_MS (900)]` for the
vocal onset. Two detectors are implemented and scored against each other:

- `EnergyRise`: the existing 10ms HP-RMS envelope; first frame whose value
  exceeds `VOICE_REL × window peak` after being below it, within the search
  range.
- `Flux`: the spectral-flux onset function from the 2026-09-04 rewrite (log-
  magnitude, 150–5000Hz, lag 2, ±1 bin max filter, −54dBFS floor), computed
  over the search range only; the strongest peak whose normalized strength
  ≥ `START_MIN_STRENGTH` (0.35).

A detection is accepted only if found inside the range; then `start` moves
to it and `span` is recomputed from the prior formula with the new start.
Otherwise the line keeps the prior. Whichever detector wins on both songs
ships; the loser stays behind a `Stages` flag for the scorer.

### Stage 2 — end refinement (audio)

Around the prior end `e = start + span`, search `[e − 0.4×span, e + 0.4×span]`
for the last frame of the vocal-band envelope above `VOICE_REL × peak`
followed by `END_QUIET_FRAMES` (3) quiet frames. Accept if found, else keep
the prior end. Words are re-spread over the refined span.

### Interfaces

`align.rs`:

```rust
pub struct Stages { pub start: Option<StartDetector>, pub end: bool }
pub enum StartDetector { EnergyRise, Flux }
impl Stages { pub const PRIOR_ONLY: Stages; pub fn shipped() -> Stages }
pub fn align(pcm, sample_rate, lines, map) -> Vec<Word>            // = align_with(.., &Stages::shipped())
pub fn align_with(pcm, sample_rate, lines, map, stages: &Stages) -> Vec<Word>
```

`examples/karaoke_score.rs` gains `matrix <dump-dir> <labels.txt>`: prints one
row per stage set (`prior`, `prior+energy`, `prior+flux`, `prior+energy+end`,
`prior+flux+end`) with median / p90 / ≤100ms / bias / worst line, plus the
`shipped()` row marked.

### Store

`STORE_V` → 3: the 21 existing v2 files were produced by the 498ms aligner
and re-record on the next full listen. (v3 was used briefly on 2026-09-04
and reverted; those files are already gone from disk via the v≠STORE_V
delete-on-read, so reusing the number is safe.)

### Error handling

- Silence (`PEAK_ABORT`) still aborts before alignment → line-only.
- A refinement that fails its threshold degrades to the prior for THAT line.
- Coverage gate stays; with the prior every line has words, so it only fires
  on the empty-lyrics path.

## Tests

- Prior: span capped by window, span floored at MIN_SPAN_MS, single-token
  line, weights spread (a 3-token line lands tokens at 0, w0/S, (w0+w1)/S).
- Start refinement: a tone burst 500ms after the stamp moves `start` there;
  a burst outside the search range leaves the prior; silence leaves the prior.
- End refinement: a burst ending at 60% of the prior span pulls the end in;
  no decay leaves the prior.
- Existing aligner tests updated to the new placement (bursts still land
  within ±40ms when the detectors fire).

## Acceptance (measured, not felt)

Both truth songs (Blur + one English track tapped by Thien with the same
tap.html flow): **median ≤ 200ms and no line with mean |Δ| > 1500ms** for the
shipped stage set; each shipped stage must not worsen either song's median.
Tap noise is ~±100ms, so no claim below ~80ms is made.

## Live checklist

- Tap a second song (English), copy labels.txt beside its dump.
- `matrix` on both dumps; pick the shipped stage set; record both rows in the
  vault project note.
- Re-listen to one track end-to-end: the wipe lands on syllables, no
  gap-spreading on lines before a break.

## Out of scope

Shipping the prior as a visible fallback (Thien: internal only). Vocal
separation / CTC alignment. Frontend changes.
