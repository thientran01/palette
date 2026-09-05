# Standalone vocalization follow-up

PR #162 is merged. The listener reports very accurate normal lyrics and smooth native rendering, with hiccups in Let Me In opening vocalizations and layered/ad-lib sections.

## Evidence

An uninterrupted local recording of Let Me In has 44 raw player anchors, a 197.68s PCM capture and 2.87ms map residual RMS. This is internal clock consistency, not a claim of audible accuracy. Private audio and model outputs remain outside the repository.

| Opening phrase | LRC source interval | New full-mix MMS interval | Vocals-only MMS interval |
| --- | --- | --- | --- |
| Oh-oh | 2760–5620ms | 2984–3225ms | 2944–3205ms |
| Oh-oh-oh-oh | 5620–8610ms | 8829–9090ms | 6523–7205ms |

The explicit blank marker at 8610ms matters: the next visible lyric starts at 12920ms. Using only visible rows would smear the phrase across the intervening silence. An earlier recording produced different poor opening spans (5678–5759 and 8549–8709ms), so a hardcoded acoustic offset is inappropriate.

Vowel-only targets and fewer repetitions did not reliably recover phrase spans. A local torchaudio 2.8 HDEMUCS_HIGH_MUSDB_PLUS comparison on the first 15 seconds improved the second onset but still compressed/misaligned the phrase. The capture is mono 16kHz; it was resampled to 44.1kHz and duplicated to stereo for separation, so this is not an evaluation on original stereo audio. Separation is not installed in the app. Its measured 18.33s included loading/download and is not an inference benchmark.

## Candidate behavior and limits

The frontend treats a standalone hyphenated repeated-oh token as a source-timed phrase. Its existing text stays in one span, line/scroll stamps stay intact, and the wipe progresses linearly through the source interval. This is a fallback, not learned or measured syllable timing. Parenthesized mixed lyrics, normal words, and single oh tokens continue using acoustic timings and the existing 90ms attack. The 160ms listener lead remains unchanged. Cached model data remains unchanged; both initial loads and karaoke-ready attachment preserve the phrase policy. No cache invalidation or full-song relisten is required.

The fallback is intentionally limited to this observed text family. It cannot correct inaccurate source LRC stamps. Mixed overlapping ad-libs, other vocalizations, and lead/backing-voice separation remain open. Live listening confirmation of this candidate is required before merge.

## Verification

Regression test failed on the old code: expected source interval 2760–5620ms, received acoustic interval 2984–3225ms. Frontend suite increased 40 → 42, all passing; production build passes. Tests cover short blank-marker preservation, phrase midpoint wipe, repeated attachment, unchanged ordinary word data, and exclusions/missing end bounds. No Rust/model changes.

Review caught a final-row endpoint bug: track duration alone is not a vocal endpoint. A regression test reproduced the 179-second wipe, then the fallback was restricted to a following source lyric or explicit empty marker. Four review perspectives ran; the confirmed finding was fixed.
