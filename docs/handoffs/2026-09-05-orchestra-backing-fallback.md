# Orchestra fallback rejected and removed

User listening feedback: the opening adlibs are the finishing part of each lead line, not simultaneous backing across the entire source interval. The phrase-wide fallback in d7fc19d incorrectly began highlighting them with the lead. Removed that fallback and restored the prior acoustic timing path.

The source text patterns (interjection plus repeated parenthetical words, or a printed echo) cannot establish whether vocals overlap. A future change must use actual sequential audio evidence, not infer onset from parentheses or source-line start. Source line timing is not ground truth for backing onset. Existing successful Let Me In repeated-oh handling and English subword fill remain unchanged.

Regression test uses the actual Orchestra cached rows: the first echo remains unfilled at 1s with the user's 220ms lead and only progresses at its own cached onset. Every original word, time, endpoint and line stamp is preserved. The test failed with the fallback and passes after removal. This restores previous behavior; it does not solve the remaining acoustic accuracy problem.

Saved source audio: %TEMP%/pulse-karaoke-mms-lab/orchestra/capture (30.84s). Do not request another recording or overwrite user timings with unvalidated model guesses.
