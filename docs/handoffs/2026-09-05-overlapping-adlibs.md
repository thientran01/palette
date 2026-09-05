# Concurrent backing-vocal highlights

User example: Let Me In, 2:28.19–2:33.67. The trailing parenthesized repeated-oh phrase is sung behind the lead throughout the line, but sequential acoustic targets place it at the printed tail and the single highlight cursor skips it as the line advances.

The frame driver now computes progress per span and bounds each row across every voice. Explicit line-owned word payloads retain source order instead of sorting concurrent words by onset. This supports independent simultaneous highlights without changing row stamps, DOM span order or the single animation loop.

For a trailing parenthesized repeated-oh phrase only, the frontend maps its existing text spans across the source line interval. A following source timestamp or explicit empty marker is required. This is a phrase-level display fallback supported by the user's observation, not acoustic syllable alignment. Exact concatenated token/source text identity is required. Mixed spoken ad-libs, single-oh interjections and other vocalizations are not assigned invented independent timings. The original standalone Oh-oh policy is preserved.

Saved Let Me In capture reconciliation: exact text match; the lead tokens for the reported line are unchanged; zero ordinary rows changed. The backing sweep spans 148190–153670ms instead of beginning at the compressed acoustic tail. The source cache remains unchanged, so no relisten/reprocessing is needed. Display timing uses the same listener lead as before.

Validation: 42 → 46 frontend tests pass. Before the fix, source attachment reordered overlapping text, the backing phrase stayed dark, and the wrong cursor could fill future lead words. Running the new driver tests against a temporary copy of the old driver produced two assertion failures (future lead incorrectly 100%; row incorrectly inactive when backing starts). The temporary test files were removed. Tests also cover explicit short blank marker, reattachment, pause/backward seek and both voice bounds. Native listening remains required before merge.
