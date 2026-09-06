# Orchestra backing-phrase fallback

The 30.84s saved Orchestra capture reproduces unreliable forced timings for the opening adlibs. Independent voice decoding, wider model context, local English ASR and vocal separation did not establish reliable word boundaries. No acoustic model change is shipped.

The display now extends the existing source-phrase fallback to two narrow source-text patterns: an Ah/Hah/Oh interjection followed by a repeated-word parenthetical chant, and a single ASCII word followed by its partial/full two-token echo (Orchestra / Orches-orchestra). A real next source stamp or explicit blank marker is required, with a maximum eight-second interval. This is a display fallback based on source timing, not measured adlib syllables or a confidence-calibrated detector. It must not be described as a solved acoustic-alignment problem.

Backing text uses the source interval concurrently with the unchanged lead timings, using the existing backingWords transform. This preserves word-span count, text, wrapping and line ownership, and discards stale inner points only on remapped backing fragments. No model/decoder/cache writes, new runtime, timers, or styling changes. Ordinary parenthetical replies such as Breathe and Please remain acoustic; unknown/long intervals and non-ASCII patterns are excluded. Repeated-oh behavior from Let Me In is unchanged.

Validation: three tests failed against the previous behavior using Orchestra's actual cached words, then passed. Frontend driver regression covers backing continuation, the existing 220ms lead, backward seek, pause and cleanup. Native live listening remains necessary to judge the fallback's feel; it deliberately trades unavailable word precision for a stable phrase sweep.


Scope audit: 25,366 cached source rows examined; six matches across duplicate cached versions, covering only the three Orchestra opening texts. Three-perspective review found a no-whitespace parenthesis/span-splitting risk; the matcher now requires a whitespace boundary and tests exclude unseparated variants. Re-review verified the fix, preserving lead points and spans. Final validation: 55 frontend tests, production build, diff whitespace check. No new models are shipped.
