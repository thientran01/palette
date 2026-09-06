# Orchestra opening: adlib investigation

Status: diagnostic capture fixed; no alignment behavior change justified yet.

Evidence: 30.84 seconds of a local Red Velvet Orchestra replay. Eight timing anchors, zero dropped anchors, 16.50ms map residual RMS, origin/intercept 187ms. No unresolved seek strikes. Local evidence is copied outside the rotating app dumps at `%TEMP%/pulse-karaoke-mms-lab/orchestra/capture`. Audio/model files are private local artifacts, not committed.

The first full replay was rejected after a near-zero Spotify position arrived with the old track identity. A short diagnostic replay was then lost because evidence saving incorrectly shared ALIGNING with a Delulu inference job. Diagnostic PCM now uses a separate bounded writer and separate retention root, including empty synced text when unavailable. Rejected evidence never updates word caches or emits karaoke-ready. Review caught and fixed eager then_some guard destruction, shared eviction, and tests racing the ALIGNING static.

## Experiments

The existing whole-row CTC constraint forces parenthesized vocals after the lead. Independent lead/backing decoding over identical emissions improved the model match for lead words, but left Elegant/elegant/dangerous and Slow/slow essentially unchanged. This does not establish exact vocal onset truth: no syllable/adlib human labels exist for this capture.

A local TorchAudio 2.8 HDEMUCS_HIGH_MUSDB_PLUS vocal-separation experiment on the first 17 seconds (6-second chunks, 1-second overlap, 2 CPU threads) did not solve it. Total initial run including model download/load was 30.57s; separation ran approximately 12s. It moved Slow/slow to about 15.4s (near the following Ad astra line) with weaker CTC confidence. Do not deploy this result or call separation an accuracy improvement on this evidence.

Before separation, independent backing estimates: Elegant 6205-7675ms, elegant 7735-9789ms, dangerous 9789-10917ms; Slow 10722-10843ms, slow 10883-11205ms. After separation: Elegant 6064-7051ms, elegant 7051-7252ms, dangerous 7272-8118ms; Slow 15369-15449ms, slow 15469-15550ms. These are model estimates, not annotations. Model confidence cannot be treated as calibrated word correctness.

Scripts/results remain in `%TEMP%/pulse-karaoke-mms-lab`: adlib_probe.py and orchestra/{opening-comparison.json,vocals-comparison.json,separate.py,separate.log}. The separated experiment reused opening emission filenames; JSON comparisons and original captured PCM remain intact.

Next work should evaluate adlib-aware recognition or an explicitly uncertain phrase display. Do not generalize a confidence threshold from this one song or spread all parenthesized lyrics across a line as if those were acoustic timestamps. Preserve current successful word timing, source line ownership, concurrent span rendering, and user lead (currently 220ms).

Reference: https://docs.pytorch.org/audio/2.8/tutorials/hybrid_demucs_tutorial.html
