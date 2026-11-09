# Karaoke sync: memory and wasted separator work (2026-11-09)

Thien asked for Palette to be faster and more efficient everywhere. For
word sync the brief was: faster, lighter on CPU or memory, or quicker to
finish, with no loss in accuracy. The PC showed ~3.6 GB while the second
ear ran (2026-11-08).

Measured in a Linux cloud container (4 cores) with Thien's own
`mms-fa-int8.onnx` and `Kim_Vocal_2.onnx`, replaying the worker's whole
job per song (mix align, the review's second ear on stereo, then the voice
pass), on JamendoLyrics. glibc was run with a fixed 1 MB mmap threshold so
freed large blocks go back to the OS, as they do on Windows.

## Where a song's job goes

| part | time (HILA, 3:54) | share |
|---|---|---|
| MMS on the mix | 19 s | 17% |
| Kim Vocal 2, 14 second-ear passes | ~52 s | 46% |
| Kim Vocal 2, 10 voice passes | ~37 s | 33% |
| everything else | ~5 s | 4% |

A separator pass is 3.9 s here (6.4 s on Thien's PC), and 98% of it is
the model (STFT, iSTFT and resampling ~0.1 s). The model's input is fixed
at 256 frames, so a pass can't be made shorter; only 17% of separator
input is zero padding, and shrinking the one-second context to fit fewer
passes would have saved 1 pass in 40.

## Changes

1. **No ONNX Runtime memory pattern** (both sessions). For the separator it
   planned one block for the whole pass on top of the per-tensor
   allocations: peak 3.37 GB → 2.19 GB on three windows, output identical
   bit for bit, same speed. MMS gets the same setting because every line
   window is a different length (peak −50 MB, identical words).
2. **The separator loads per job and goes with it** (~0.2 s per song with
   the checksum). Its arena held ~1.2 GB until the five-minute idle
   release, through the next song's mix pass too. The aligner still stays
   loaded between songs.
3. **One `VocalEar` for the whole job.** It keeps every stretch the review
   separated (~2.5 MB for 14 passes), and `voice::listen` hears a held note
   inside one for free. The review's own stretches never overlap, so its
   words can't change. A reused note's vocals had a second of real context
   each side, the same as a fresh pass gives.

## Results, 12 songs (every 7th of the set)

| | before | after |
|---|---|---|
| peak memory per job | 3.40–3.68 GB | 2.22–2.40 GB |
| memory after the job | ~3.5 GB (until idle release) | ~1.0 GB |
| words | 3,456 | 3,456, all identical |
| words > 300 ms off | 156 | 156 |
| separator passes (review / voice) | 109 / 116 | 109 / 119 |
| held notes with a voice track | 284 | 435 (+53%) |
| job time | 1,098 s | 1,119 s (noise) |

On the 270 notes voiced both ways the pitch track differs by 0 cents at
the median and p90, 8 cents at p99; frames agree on voiced/unvoiced 99%.

## Not done

- **4 separator threads**: output is identical and a pass is ~1.8x faster
  (30 s → 17.5 s on three windows), for ~15% more CPU in total. Thien chose
  to keep 2 threads (2026-11-09).
- **Spending the freed voice passes as time**: with reuse a budget of 5
  voices as many notes over the 12 songs as 10 did, but songs where the
  review heard little (JASON MILLER, Vente) would voice fewer notes. Kept
  at 10, so every song voices at least as many as before.
- **int8 Kim Vocal 2**: dynamic quantization needs ConvInteger, which ONNX
  Runtime's CPU provider lacks; static QDQ would need calibration and a
  new model file. Conv is 57% of a pass, MatMul 15%.
- Graph optimization levels 1 and 2 were slower and lower memory than the
  default (level 3) but changed the output.
