# Karaoke: spell Japanese the way it is sung (2026-11-08)

Thien listens to a lot of Japanese music (Yorushika, YOASOBI, TUYU). After
PR #225 let those songs learn word sync at all, the syncs on kanji-heavy
lines were still loose: "what should we do about the kanji with Chinese
pronunciations".

## What was wrong

`align::tokenize` makes every kana and kanji its own token, and
`acoustic::plan_tokens` spelled each one on its own with uroman. Three
things came out wrong:

| Line | uroman, one character at a time | As sung |
|---|---|---|
| 水面に映る月 | shui mian ni ying ru yue | sui men ni utsu ru tsuki |
| 一人で泣いた夜 | 1 ren de qi i ta ye | hito ri de na i ta yoru |
| 待ってよ | dai tsu te yo | ma t te yo |
| 今日 / きょう | jin ri / ki yo u | kyo u / k yo u |
| 君は / 花へ | jun ha / hua he | kimi wa / hana e |

- Kanji: uroman has no Japanese readings, so it gives Mandarin pinyin.
- っ and small kana only make sense with their neighbour: っ doubles the
  next consonant, ょ joins the sound before it.
- The particles は and へ are said "wa" and "e".

The MMS model then listens for sounds the singer never makes.

## What changed

`acoustic::japanese` (recipe `mms-int8/4`), only for songs with kana
(`Numerals::for_song`, the same test PR #225 uses):

- `spell` turns a line's kana into letters across token boundaries: っ
  takes the next sound's consonant (t before ch), small kana join the
  sound before (き + ょ = k + yo; ね + ぇ = ne + e), ー holds the last
  vowel. Latin words, punctuation and marks it doesn't know keep the old
  path (uroman, then `spoken::fallback`). This needs no dictionary.
- `Reader` (jpreprocess 0.15, OpenJTalk's front end in pure Rust, with its
  naist-jdic) reads each Japanese stretch of a line as a whole and gives
  kanji and numbers their readings. A word's reading is laid over its
  characters: its kana must match where they stand (思い出す: 思 オモ, 出
  ダ), and a run of kanji shares the rest by mora. A character takes its
  own reading where the run starts or ends with it (時々: 時 トキ; 写真:
  真 シン; 11月: 11 ジュウイチ), otherwise the run splits evenly (一人: 一
  ヒト, 人 リ). The particles は and へ get ワ and エ. A word that doesn't
  fit, or text the dictionary rewrote, falls back to the old spelling for
  that stretch only.
- The dictionary is a karaoke-model asset: `install_karaoke_model.py
  --japanese-dictionary naist-jdic-jpreprocess.tar.gz` (jpreprocess's
  v0.15.0 release, 28.7 MB) unpacks the 8 pinned files to
  `karaoke-model/naist-jdic` (77 MB). jpreprocess reads only dictionaries
  its own version built, so the crate is pinned with them. The aligner
  checks the pins and loads it on the first Japanese song (about 0.15 s,
  ~110 MB), and it goes with the aligner after 5 idle minutes. Without it
  only `spell`'s kana fixes apply.
- `sung_syllables` counts the same letters, so a line's window is sized
  from morae instead of pinyin syllables.

## Result on Thien's five Japanese dumps

No truth labels for Japanese (JamendoLyrics has none, and Thien doesn't
tap-label), so the measure is the review's own support check from
`sync_review.rs`, calibrated on JamendoLyrics (support ≥ 0.9: 0.8% of
words more than 300 ms off; < 0.5: a third or more). Mix pass, Linux
container, `karaoke-dumps` from Thien's PC (YOASOBI 夜に駆ける, Vaundy
odoriko, Yorushika あぶく, Cö shu Nie undress me, TUYU It's Raining After
All): 276 lines, 3,188 words.

| Spelling | Doubted lines | Weak words (< 0.5) | Sure words (≥ 0.9) | Letter fit | Expected words > 300 ms off |
|---|---|---|---|---|---|
| Before (uroman per character) | 44.6% | 14.3% | 64.3% | 0.33 | ~9.9% |
| Kana fixes only, no dictionary | 41.3% | 12.0% | 66.4% | 0.35 | ~8.8% |
| Kana fixes + naist-jdic readings | 22.8% | 6.5% | 79.3% | 0.43 | ~5.4% |

"Letter fit" is the model's mean probability for each target letter along
the chosen path. It rises on every song, which says the audio matches the
new spelling better, not only that the decoder is surer of itself.
"Expected" applies the JamendoLyrics calibration to each word's support.

Per song, doubted lines fell on all five: 夜に駆ける 30% → 4%, あぶく 54%
→ 25%, undress me 46% → 28%, It's Raining After All 44% → 17%, odoriko
50% → 46% (more English and fewer kanji). 308 of 3,216 words moved by more
than 300 ms; spot checks on あぶく put them where the new reading is sung
(私に蠢く獣 "watashi ni ugomeku kemono", support 0.72 → 0.95; 過ぎ去る雲
0.10 → 0.84).

With the second ear as the app runs it (`align_reviewed`, Kim Vocal 2 on
the mono recording, 14 passes a song), on the same five songs:

| Spelling | Lines still doubted | Weak words | Sure words | Expected words > 300 ms off | Separator passes |
|---|---|---|---|---|---|
| Before | 36.6% | 9.2% | 68.2% | ~7.6% | 70 |
| Kana fixes + naist-jdic readings | 17.4% | 4.9% | 80.5% | ~4.7% | 59 |

The readings fix more than the second ear does on its own, and the ear is
spent on fewer lines.

## Limits

- Lyricists' own readings (本気 as maji, 宇宙 as sora) are written as
  furigana, which LRC drops. The dictionary gives the standard reading.
  Same for poetic words (水面 as minamo). One line in あぶく got worse for
  this reason: 暇 read "itoma" where it is sung "hima" (0.94 → 0.43). The
  review doubts such lines, so the second ear looks at them.
- A Chinese song that quotes Japanese kana is treated as Japanese.
- Readings are split over a word's characters by rule, so the highlight
  boundary inside a compound can be a mora off (ヶ月: ヶ カゲ, 月 ツ). The
  word's start and end are unaffected.

## Rejected

- lindera or vibrato with IPADIC: 一人 read ichinin, を as wo. UniDic
  2.1.2: 風 read fuu, 230 MB in memory. Embedding any dictionary in the
  exe: +47 MB (IPADIC) to +200 MB (UniDic), and a download in build.rs.
- Sudachi: not published on crates.io by its authors, and the largest
  dictionaries.
- A per-kanji table (KANJIDIC): it can't choose between on and kun
  readings without the word around it.
