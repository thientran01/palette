# Trial entry timing

Scope approved in conversation: ship the positively reviewed entry improvements, excluding unvalidated held-note changes. One PR stacked on the vocal trial and source guard.

Separated-vocal worker uses align_entries; normal align and offline align_probe retain their behavior. Initial oh/ooh/ah tokens use 20ms RMS frames, 15% of interpolated p90 with a 0.001 floor, bridge <=100ms dips, require >=300ms support. Search starts no earlier than line minus 500ms or previous word end and stops at the next word start, with a 10s maximum. Choose the longest eligible run; require at least 120ms entry advance. Never shorten the original end or move following words; clear stale subword points. Parenthesized initial tokens are excluded.

Pure rows of 3–16 la tokens (no parentheses) omit the leading wildcard and stop inference at the following LRC stamp. Preserve text, line stamps, and freshly computed subword points. No changes to ordinary words, mixed adlib rows, held-note spellings, or manual timing injection.

Cache v2 uses mms-int8/demucs-entry-2. Fall back to v1 words while a new listen learns v2, then to the ordinary cache. Prior files remain untouched.

Validation: synthetic silence/burst/sustained-entry and neighbor invariance; row eligibility; cache fallback; Rust suite/Clippy; local captured examples compared against the accepted previews. Human verdicts are small-sample evidence, not universal accuracy guarantees. Ship in the opt-in development trial, not the release default.
