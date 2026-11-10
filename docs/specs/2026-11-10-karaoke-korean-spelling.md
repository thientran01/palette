# Karaoke: spell Korean the way it is sung (2026-11-10)

Item M10 of the outside lyric-sync review: Korean wasn't spelled as sung.
`align::tokenize` makes every Hangul syllable its own token, and
`acoustic::plan_tokens` spelled each one on its own with uroman, which
writes out every letter as written. Korean sounds change where syllables
meet, so the model listened for sounds the singer never makes:

| Written | uroman, one syllable at a time | As sung |
|---|---|---|
| 같이 | gat i | ga ci (가치) |
| 좋아 | joh a | jo a (조아) |
| 없어 | eobs eo | eob seo (업서) |
| 있는 | iss neun | in neun (인는) |
| 국물 | gug mul | gung mul (궁물) |
| 몰라 | mol ra | mol la |

On 15 K-pop songs from LRCLIB, 19% of Hangul syllables and 67% of lines
change.

## What changed

`acoustic::korean`, for songs whose lyrics have Hangul and no kana
(`Numerals::for_song`, the same test the Japanese path uses):

- `pronounce` applies the regular rules of standard pronunciation (표준
  발음법) inside each written word, the ones g2pk applies without a
  dictionary: a final consonant carries over onto a vowel (clusters carry
  their second consonant), ㄷ/ㅌ before 이 become ㅈ/ㅊ, ㅎ goes silent
  before a vowel and aspirates a following ㄱ/ㄷ/ㅈ, stops turn nasal
  before ㄴ/ㅁ/ㄹ, ㄹ after ㅁ/ㅇ becomes ㄴ, ㄴ and ㄹ together become ㄹㄹ,
  consonant-led ㅢ is ㅣ, and any final consonant left is said as one of ㄱ
  ㄴ ㄷ ㄹ ㅁ ㅂ ㅇ. It hands back each token's syllable as sung.
- `spell` romanizes that syllable with uroman, so the letters stay the ones
  uroman uses for written Korean (ㅊ "c", ㅆ "ss"). ㄹㄹ has no single
  syllable spelling, so the second ㄹ's "r" becomes "l".
- Nothing carries across a space: the dataset below never does (326 of 326
  word-final consonants before a vowel-initial word stayed put).
- Rules that need to know the word are left out: ㄴ inserted in compounds
  (꽃잎 꼰닙), 의 as a particle, tensing after an adnominal ㄹ (할게), and
  the Sino-Korean exceptions to ㄹ assimilation.

### Two regular rules left out on purpose

Tensing after a stop (학교 학꾜) and a stop meeting ㅎ (축하 추카) are
real, but they made timing worse. Both put a hard consonant at the start of
the next syllable, and its letters ("gg", "k") take the stop's silent
closure with them. With them on, the syllables they touched landed 40-75 ms
early:

| Syllables whose onset the rule changed | Within 100 ms, written | With the rule |
|---|---|---|
| tensing ㄱ→ㄲ (110) | 86.4% | 45.5% |
| tensing ㄷ→ㄸ (120) | 94.2% | 70.8% |
| tensing ㅂ→ㅃ (38) | 100% | 52.6% |
| tensing ㅅ→ㅆ (76) | 80.3% | 59.2% |
| ㄱ+ㅎ→ㅋ (62) | 87.1% | 69.4% |

Written as they are, the closure stays with the syllable before.

## How it was measured

Korean has no JamendoLyrics songs and Thien doesn't tap-label, so this used
the Children's Song Dataset (CSD, Zenodo 4785016, CC BY-NC-SA): 50 Korean
children's songs, each sung in two keys by one professional singer, no
accompaniment. Every note has a hand-labelled onset (15,816 syllables), and
the dataset also transcribes how each syllable is sung, which checks the
rules directly.

- Rules against CSD's sung transcript: 93.4% of syllables match, against
  79.4% for the written syllable with its final consonant said plainly. Of
  the 1,040 that don't, 310 are ㄹㄹ (which `spell` fixes in the letters),
  480 are tensing (left out above, or after an adnominal ㄹ), and 250 are
  other word-dependent changes.
- Timing: the mix pass (`Knobs::MIX`, no second ear), identity time map,
  each line stamped at its first note's onset rounded to 10 ms, Linux
  container with the AMX shim. Scratch harness, not in the repo.

| | Syllables | Median error | Within 100 ms | Within 300 ms | Doubted lines | Weak words |
|---|---|---|---|---|---|---|
| Before, all | 15,816 | 41 ms | 86.0% | 96.4% | 17.3% | 3.2% |
| After, all | 15,816 | 40 ms | 86.9% | 96.5% | 15.7% | 2.9% |
| Before, respelled syllables | 2,792 | 53 ms | 82.8% | 98.0% | | |
| After, respelled syllables | 2,792 | 40 ms | 87.5% | 98.1% | | |

Carried-over consonants are the big win: those 1,068 syllables used to
start at their vowel, 62 ms late at the median, and now start at the
consonant (median error 67 → 35 ms, within 100 ms 78.1% → 89.5%). No group
of respelled syllables got worse. Unchanged syllables didn't move.

A solo voice is the easy case: the aligner already places 86% of syllables
within 100 ms. K-pop over a full mix doubts about 44% of lines, and 19% of
its syllables are respelled, so the gain there should be larger, but there
is no labelled K-pop to show it.

## Recipe

`RECIPE` stays `mms-int8/4`. Since PR #251 a bump re-decodes saved syncs
from their takes instead of relearning them, but takes only exist for
listens since that PR. Bumped now, nearly every saved sync in every
language would still go back to relearning from one listen and lose its
merged listens. Korean songs with doubted lines still get their extra
listens, which use the new spelling and merge in line by line where they
are surer. The new spelling keeps every line's window (vowel groups per
syllable don't change), so the next decoder bump, once takes have built
up, re-decodes Korean songs with it too.
