//! What a Japanese lyric line sounds like, in the aligner's letters.
//!
//! The aligner's tokens are one character each, and uroman spells each one
//! on its own, so three things come out wrong in Japanese songs:
//!
//! - Kanji read as Mandarin: 水面 "shuimian", 一人 "1 ren", 風 "feng".
//! - Sounds that only exist with a neighbour: っ said "tsu" (待って
//!   "matsute"), and きょ said "kiyo".
//! - The particles は and へ said "ha" and "he" instead of "wa" and "e".
//!
//! `Reader` asks OpenJTalk's dictionary (jpreprocess with naist-jdic, a
//! folder the karaoke installer stages beside the model) how each word is
//! read, and hands each kanji its share of the word's reading. `spell` then
//! turns the line's kana into letters across token boundaries. Without the
//! dictionary `spell` still fixes っ, small kana and ー; kanji keep uroman's
//! spelling. Every helper returns spellings, never timings, and a token it
//! can't spell keeps the caller's path (uroman and `spoken::fallback`).

use std::path::Path;

/// The folder inside karaoke-model that holds the dictionary.
pub const DICTIONARY_DIR: &str = "naist-jdic";

/// jpreprocess v0.15.0's prebuilt naist-jdic
/// (naist-jdic-jpreprocess.tar.gz on its v0.15.0 release). jpreprocess
/// reads only dictionaries its own version built, so Cargo.toml pins the
/// crate to match. The readings are part of RECIPE: a different dictionary
/// would teach different syncs.
const DICTIONARY: [(&str, &str); 8] = [
    (
        "char_def.bin",
        "cd3dbb6fa448906590c2e5ef2600378e4bd074c4d1d6fc5c049d4fd8c2d2d556",
    ),
    (
        "dict.da",
        "26f14fe3affdb0966e1696ae6cd4b7faac91bf85b7ca57a5d8cdc045da58d043",
    ),
    (
        "dict.vals",
        "7fb308e1909be247779be0a4c81070416635c4ff0ab266d48a5e964baffd345f",
    ),
    (
        "dict.words",
        "7ef63438285a3dd50299aa826f3d62d9657a907184e055a179af3e00eae820aa",
    ),
    (
        "dict.wordsidx",
        "67b30a728ab8816def37d9e2775d99dc98bf8eaa6f74a821dd3ee2fd009e8b61",
    ),
    (
        "matrix.mtx",
        "a64b58ac89b2ca21f3ae6c56ca772f8d4488cc636f47b821b4cca9c0c67c95d2",
    ),
    (
        "metadata.json",
        "8fa56e3d131c9d7cabc3633bb7c2add979469a1e22be90ecd714e81df20a5dd4",
    ),
    (
        "unk.bin",
        "1ff6b44ab3145cd8ac731b9cb0500804feae5fa257aee8bc78a65a8dc80bb454",
    ),
];

/// A word as the dictionary read it.
#[derive(Debug, Clone, PartialEq)]
pub struct Morpheme {
    pub surface: String,
    /// Part of speech: 名詞, 助詞, ...
    pub pos: String,
    /// The reading in katakana, as written: 今日 キョウ, は ハ.
    pub reading: String,
}

impl Morpheme {
    /// One row of jpreprocess's NJD features:
    /// surface,pos,pos1,pos2,pos3,ctype,cform,base,reading,pron,...
    fn parse(row: &str) -> Option<Morpheme> {
        let fields: Vec<&str> = row.split(',').collect();
        Some(Morpheme {
            surface: fields.first()?.to_string(),
            pos: fields.get(1)?.to_string(),
            reading: fields.get(8)?.to_string(),
        })
    }
}

/// OpenJTalk's reading dictionary. About 110 MB in memory, so the aligner
/// loads it only for a song with kana and drops it with the model.
pub struct Reader {
    jp: jpreprocess::JPreprocess<jpreprocess::DefaultTokenizer>,
}

impl Reader {
    pub fn load(dir: &Path) -> Result<Reader, String> {
        for (name, sha) in DICTIONARY {
            super::verify_asset(&dir.join(name), sha)?;
        }
        let dictionary = jpreprocess::SystemDictionaryConfig::File(dir.to_path_buf())
            .load()
            .map_err(|e| e.to_string())?;
        Ok(Reader {
            jp: jpreprocess::JPreprocess::with_dictionaries(dictionary, None),
        })
    }

    /// The katakana each token is read as, where the dictionary knows
    /// better than uroman: kanji, numbers, and the particles は and へ.
    /// None leaves a token to `spell`'s own kana or the caller.
    pub fn read(&self, tokens: &[String]) -> Vec<Option<String>> {
        let mut out = vec![None; tokens.len()];
        for run in runs(tokens) {
            let text: String = run.iter().map(|&i| tokens[i].trim()).collect();
            if !text.chars().any(|c| wild(c) || c == 'は' || c == 'へ') {
                continue;
            }
            let Ok(rows) = self.jp.run_frontend(&text) else {
                continue;
            };
            let words: Vec<Morpheme> = rows.iter().filter_map(|r| Morpheme::parse(r)).collect();
            place(
                tokens,
                &run,
                &words,
                &|i| self.alone(tokens[i].trim()),
                &mut out,
            );
        }
        out
    }

    /// How one character (or number) is read on its own: 宇 ウ, 11
    /// ジュウイチ. Used to find where a compound's reading splits.
    fn alone(&self, text: &str) -> Option<String> {
        let rows = self.jp.run_frontend(text).ok()?;
        let reading: String = rows
            .iter()
            .map(|r| Morpheme::parse(r).map(|m| m.reading))
            .collect::<Option<Vec<_>>>()?
            .concat();
        (!reading.is_empty() && reading.chars().map(katakana).all(is_kana)).then_some(reading)
    }
}

/// A token's reading on its own, if the dictionary has one.
type Alone<'a> = &'a dyn Fn(usize) -> Option<String>;

fn is_kana(c: char) -> bool {
    matches!(u32::from(c), 0x3041..=0x3096 | 0x30A1..=0x30FA | 0x30FC)
}

/// Characters whose sound only the dictionary knows: kanji, the
/// iteration marks that repeat one, the counter ヶ, and digits.
fn wild(c: char) -> bool {
    matches!(
        u32::from(c),
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x3005 | 0x303B | 0x30F5 | 0x30F6
    ) || c.is_ascii_digit()
}

fn japanese(c: char) -> bool {
    wild(c) || matches!(u32::from(c), 0x3041..=0x309F | 0x30A1..=0x30FF)
}

/// Stretches of the line the dictionary reads as one text: Japanese
/// characters and numbers, broken at spaces, punctuation and Latin words.
fn runs(tokens: &[String]) -> Vec<Vec<usize>> {
    let mut runs = Vec::new();
    let mut run = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let core = token.trim();
        let member = !core.is_empty()
            && (core.chars().count() == 1 && core.chars().all(japanese)
                || core.chars().all(|c| c.is_ascii_digit()));
        if member {
            run.push(i);
        } else if !run.is_empty() {
            runs.push(std::mem::take(&mut run));
        }
        if member && core.len() != token.len() && !run.is_empty() {
            runs.push(std::mem::take(&mut run));
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}

/// jpreprocess writes digits full width (３つ).
fn fold(c: char) -> char {
    match u32::from(c) {
        0xFF10..=0xFF19 => char::from_u32(u32::from(c) - 0xFEE0).unwrap_or(c),
        _ => c,
    }
}

fn katakana(c: char) -> char {
    match u32::from(c) {
        0x3041..=0x3096 => char::from_u32(u32::from(c) + 0x60).unwrap_or(c),
        _ => c,
    }
}

/// Lays the dictionary's words back over the run's tokens. A word whose
/// text doesn't match the line (the dictionary rewrote it) ends the run:
/// everything after it is uroman's again.
fn place(
    tokens: &[String],
    run: &[usize],
    words: &[Morpheme],
    alone: Alone,
    out: &mut [Option<String>],
) {
    let chars: Vec<(char, usize)> = run
        .iter()
        .flat_map(|&i| tokens[i].trim().chars().map(move |c| (c, i)))
        .collect();
    let mut at = 0;
    for word in words {
        let surface: Vec<char> = word.surface.chars().map(fold).collect();
        if surface.is_empty() {
            continue;
        }
        let Some(slice) = chars.get(at..at + surface.len()) else {
            return;
        };
        if slice.iter().map(|&(c, _)| c).ne(surface.iter().copied()) {
            return;
        }
        at += surface.len();
        if word.pos == "助詞" && slice.len() == 1 {
            let said = match slice[0].0 {
                'は' => Some("ワ"),
                'へ' => Some("エ"),
                _ => None,
            };
            if let Some(said) = said {
                out[slice[0].1] = Some(said.into());
            }
            continue;
        }
        if slice.iter().any(|&(c, _)| wild(c)) {
            for (token, kana) in share(slice, &word.reading, alone) {
                out[token] = Some(kana);
            }
        }
    }
}

/// Splits a word's reading over its characters. Kana in the word must match
/// the reading where they stand (思い出 オモイデ: 思 オモ, 出 デ). A stretch
/// of kanji shares what lies between them (`split`). A word that doesn't
/// fit gives nothing.
fn share(slice: &[(char, usize)], reading: &str, alone: Alone) -> Vec<(usize, String)> {
    let reading: Vec<char> = reading.chars().map(katakana).collect();
    if reading.is_empty() || reading.len() > 64 || reading.iter().any(|&c| !is_kana(c)) {
        return Vec::new();
    }
    // Pieces: a stretch of wild characters (their tokens), or one kana.
    enum Piece {
        Wild(Vec<usize>),
        Kana(char),
    }
    let mut pieces: Vec<Piece> = Vec::new();
    for &(c, token) in slice {
        if wild(c) {
            if let Some(Piece::Wild(tokens)) = pieces.last_mut() {
                if tokens.last() != Some(&token) {
                    tokens.push(token);
                }
                continue;
            }
            pieces.push(Piece::Wild(vec![token]));
        } else {
            pieces.push(Piece::Kana(katakana(c)));
        }
    }
    fn fit(pieces: &[Piece], reading: &[char], lens: &mut Vec<usize>) -> bool {
        match pieces.split_first() {
            None => reading.is_empty(),
            Some((Piece::Kana(k), rest)) => {
                reading.first() == Some(k) && fit(rest, &reading[1..], lens)
            }
            Some((Piece::Wild(_), rest)) => {
                for len in 1..=reading.len() {
                    lens.push(len);
                    if fit(rest, &reading[len..], lens) {
                        return true;
                    }
                    lens.pop();
                }
                false
            }
        }
    }
    let mut lens = Vec::new();
    if !fit(&pieces, &reading, &mut lens) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = 0;
    let mut lens = lens.into_iter();
    for piece in &pieces {
        match piece {
            Piece::Kana(_) => at += 1,
            Piece::Wild(tokens) => {
                let len = lens.next().unwrap_or(0);
                let said: String = reading[at..at + len].iter().collect();
                at += len;
                out.extend(split(tokens, &morae(&said), alone).unwrap_or_default());
            }
        }
    }
    out
}

/// Shares a stretch of kanji's reading by mora. A character takes its own
/// reading where the stretch starts with it (時々 トキドキ: 時 トキ, 11月:
/// 11 ジュウイチ) or the last one where the stretch ends with it (写真
/// シャシン: 真 シン). Otherwise the stretch splits evenly, earlier
/// characters taking the extra (一人 ヒトリ: 一 ヒト, 人 リ). None when there
/// are fewer morae than characters.
fn split(tokens: &[usize], morae: &[String], alone: Alone) -> Option<Vec<(usize, String)>> {
    let (&first, rest) = tokens.split_first()?;
    if morae.len() < tokens.len() {
        return None;
    }
    if rest.is_empty() {
        return Some(vec![(first, morae.concat())]);
    }
    let own = |token: usize| {
        alone(token)
            .map(|a| morae_of(&a))
            .filter(|own| !own.is_empty() && own.len() + rest.len() <= morae.len())
    };
    if let Some(own) = own(first).filter(|own| morae[..own.len()] == own[..]) {
        let mut out = vec![(first, own.concat())];
        out.extend(split(rest, &morae[own.len()..], alone)?);
        return Some(out);
    }
    let last = *rest.last()?;
    if let Some(own) = own(last).filter(|own| morae[morae.len() - own.len()..] == own[..]) {
        let head = &tokens[..tokens.len() - 1];
        let mut out = split(head, &morae[..morae.len() - own.len()], alone)?;
        out.push((last, own.concat()));
        return Some(out);
    }
    let take = morae.len().div_ceil(tokens.len());
    let mut out = vec![(first, morae[..take].concat())];
    out.extend(split(rest, &morae[take..], alone)?);
    Some(out)
}

fn morae_of(kana: &str) -> Vec<String> {
    morae(&kana.chars().map(katakana).collect::<String>())
}

/// Small kana, っ and ー ride on the sound before them.
fn rides(c: char) -> bool {
    matches!(
        c,
        'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ャ' | 'ュ' | 'ョ' | 'ヮ' | 'ッ' | 'ー'
    )
}

fn morae(kana: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in kana.chars() {
        match out.last_mut() {
            Some(last) if rides(c) => last.push(c),
            _ => out.push(c.to_string()),
        }
    }
    out
}

/// One katakana sound in the aligner's letters (Hepburn, as uroman spells
/// whole words).
fn letters(c: char) -> Option<&'static str> {
    Some(match c {
        'ア' => "a",
        'イ' => "i",
        'ウ' => "u",
        'エ' => "e",
        'オ' => "o",
        'カ' | 'ヵ' => "ka",
        'キ' => "ki",
        'ク' => "ku",
        'ケ' | 'ヶ' => "ke",
        'コ' => "ko",
        'ガ' => "ga",
        'ギ' => "gi",
        'グ' => "gu",
        'ゲ' => "ge",
        'ゴ' => "go",
        'サ' => "sa",
        'シ' => "shi",
        'ス' => "su",
        'セ' => "se",
        'ソ' => "so",
        'ザ' => "za",
        'ジ' | 'ヂ' => "ji",
        'ズ' | 'ヅ' => "zu",
        'ゼ' => "ze",
        'ゾ' => "zo",
        'タ' => "ta",
        'チ' => "chi",
        'ツ' => "tsu",
        'テ' => "te",
        'ト' => "to",
        'ダ' => "da",
        'デ' => "de",
        'ド' => "do",
        'ナ' => "na",
        'ニ' => "ni",
        'ヌ' => "nu",
        'ネ' => "ne",
        'ノ' => "no",
        'ハ' => "ha",
        'ヒ' => "hi",
        'フ' => "fu",
        'ヘ' => "he",
        'ホ' => "ho",
        'バ' => "ba",
        'ビ' => "bi",
        'ブ' => "bu",
        'ベ' => "be",
        'ボ' => "bo",
        'パ' => "pa",
        'ピ' => "pi",
        'プ' => "pu",
        'ペ' => "pe",
        'ポ' => "po",
        'マ' => "ma",
        'ミ' => "mi",
        'ム' => "mu",
        'メ' => "me",
        'モ' => "mo",
        'ヤ' => "ya",
        'ユ' => "yu",
        'ヨ' => "yo",
        'ラ' => "ra",
        'リ' => "ri",
        'ル' => "ru",
        'レ' => "re",
        'ロ' => "ro",
        'ワ' => "wa",
        'ヰ' => "i",
        'ヱ' => "e",
        'ヲ' => "o",
        'ン' => "n",
        'ヴ' => "vu",
        _ => return None,
    })
}

fn is_vowel(c: char) -> bool {
    "aeiou".contains(c)
}

/// The consonant a っ doubles: the next sound's first, t before ch.
fn doubled(next: &str) -> Option<char> {
    if next.starts_with("ch") {
        return Some('t');
    }
    next.chars()
        .next()
        .filter(|&c| c.is_ascii_lowercase() && !is_vowel(c))
}

/// Letters for each token of a Japanese line, or None where the caller's
/// own spelling stands. `readings` comes from `Reader::read` (or is all
/// None); `fallback` is how the caller spells a token on its own, used here
/// only to know what sound follows a っ or precedes a small kana.
pub fn spell(
    tokens: &[String],
    readings: &[Option<String>],
    fallback: impl Fn(&str) -> String,
) -> Vec<Option<String>> {
    let n = tokens.len();
    let mut out: Vec<String> = vec![String::new(); n];
    let mut ours = vec![false; n];
    let mut failed = vec![false; n];
    // The last sound said: its token, its letters, and whether `spell`
    // wrote them (only those can be reshaped by a small kana).
    let mut last: Option<(usize, String, bool)> = None;
    let mut sokuon: Option<usize> = None;
    for (i, token) in tokens.iter().enumerate() {
        let core = token.trim();
        let kana: Option<String> = match readings.get(i).cloned().flatten() {
            Some(reading) => Some(reading.chars().map(katakana).collect()),
            None if core.chars().count() == 1 && core.chars().all(is_kana) => {
                Some(core.chars().map(katakana).collect())
            }
            None => None,
        };
        let Some(kana) = kana else {
            let latin = fallback(token);
            if let Some(t) = sokuon.take() {
                out[t].extend(doubled(&latin));
            }
            last = Some((i, latin, false));
            continue;
        };
        ours[i] = true;
        for c in kana.chars() {
            match c {
                'ッ' => sokuon = Some(i),
                'ー' => {
                    let vowel = last
                        .as_ref()
                        .and_then(|(_, l, _)| l.chars().rev().find(|&c| is_vowel(c)));
                    out[i].extend(vowel);
                }
                'ャ' | 'ュ' | 'ョ' | 'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ヮ' => {
                    let vowel = match c {
                        'ャ' | 'ァ' | 'ヮ' => 'a',
                        'ュ' | 'ゥ' => 'u',
                        'ョ' | 'ォ' => 'o',
                        'ィ' => 'i',
                        _ => 'e',
                    };
                    let glide = match c {
                        'ャ' | 'ュ' | 'ョ' => "y",
                        'ヮ' => "w",
                        _ => "",
                    };
                    let said = match last.as_mut() {
                        // ねぇ holds ね's vowel.
                        Some((_, letters, _)) if glide.is_empty() && letters.ends_with(vowel) => {
                            vowel.to_string()
                        }
                        // キ + ョ: キ keeps its consonant and ョ says "yo".
                        Some((t, letters, true))
                            if letters.len() >= 2 && letters.ends_with(is_vowel) =>
                        {
                            out[*t].pop();
                            letters.pop();
                            let soft = ["sh", "ch", "j"].iter().any(|s| letters.ends_with(s));
                            let glide = if soft && glide == "y" { "" } else { glide };
                            format!("{glide}{vowel}")
                        }
                        _ => format!("{glide}{vowel}"),
                    };
                    if let Some(t) = sokuon.take() {
                        out[t].extend(doubled(&said));
                    }
                    out[i].push_str(&said);
                    last = Some((i, said, true));
                }
                _ => {
                    let Some(said) = letters(c) else {
                        failed[i] = true;
                        last = None;
                        continue;
                    };
                    if let Some(t) = sokuon.take() {
                        out[t].extend(doubled(said));
                    }
                    out[i].push_str(said);
                    last = Some((i, said.to_string(), true));
                }
            }
        }
        // A space is a breath: no っ doubles across it.
        if core.len() != token.len() {
            sokuon = None;
        }
    }
    out.into_iter()
        .enumerate()
        .map(|(i, s)| (ours[i] && !failed[i] && !s.is_empty()).then_some(s))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &[&str]) -> Vec<String> {
        text.iter().map(|s| s.to_string()).collect()
    }

    fn word(surface: &str, pos: &str, reading: &str) -> Morpheme {
        Morpheme {
            surface: surface.into(),
            pos: pos.into(),
            reading: reading.into(),
        }
    }

    fn none(_: usize) -> Option<String> {
        None
    }

    fn spelled(tokens: &[String], readings: &[Option<String>]) -> Vec<Option<String>> {
        spell(tokens, readings, |t| t.trim().to_lowercase())
    }

    fn joined(spelled: &[Option<String>]) -> Vec<&str> {
        spelled
            .iter()
            .map(|s| s.as_deref().unwrap_or("-"))
            .collect()
    }

    #[test]
    fn sokuon_doubles_the_next_consonant() {
        let t = tokens(&["待", "っ", "て"]);
        let s = spelled(&t, &[Some("マ".into()), None, None]);
        assert_eq!(joined(&s), ["ma", "t", "te"]);
        // Before ch it doubles as t (matcha), and across tokens' readings.
        let t = tokens(&["一", "緒"]);
        let s = spelled(&t, &[Some("イッ".into()), Some("ショ".into())]);
        assert_eq!(joined(&s), ["is", "sho"]);
        let t = tokens(&["ま", "っ", "ち", "ゃ"]);
        assert_eq!(
            joined(&spelled(&t, &[None, None, None, None])),
            ["ma", "t", "ch", "a"]
        );
    }

    #[test]
    fn a_line_final_sokuon_keeps_the_callers_spelling() {
        let t = tokens(&["あ", "っ"]);
        assert_eq!(joined(&spelled(&t, &[None, None])), ["a", "-"]);
        // A space is a breath, so nothing doubles across it.
        let t = tokens(&["あ", "っ ", "か"]);
        assert_eq!(joined(&spelled(&t, &[None, None, None])), ["a", "-", "ka"]);
    }

    #[test]
    fn small_kana_join_the_sound_before() {
        let t = tokens(&["き", "ょ", "う"]);
        assert_eq!(joined(&spelled(&t, &[None, None, None])), ["k", "yo", "u"]);
        let t = tokens(&["し", "ゃ"]);
        assert_eq!(joined(&spelled(&t, &[None, None])), ["sh", "a"]);
        let t = tokens(&["フ", "ァ"]);
        assert_eq!(joined(&spelled(&t, &[None, None])), ["f", "a"]);
        let t = tokens(&["テ", "ィ"]);
        assert_eq!(joined(&spelled(&t, &[None, None])), ["t", "i"]);
        // A bare vowel keeps itself: ウィ is "ui".
        let t = tokens(&["ウ", "ィ"]);
        assert_eq!(joined(&spelled(&t, &[None, None])), ["u", "i"]);
        // The same vowel again holds it: ねぇ is "nee".
        let t = tokens(&["ね", "ぇ"]);
        assert_eq!(joined(&spelled(&t, &[None, None])), ["ne", "e"]);
        // A small kana alone, or after a token spell didn't write, stands alone.
        let t = tokens(&["ゃ"]);
        assert_eq!(joined(&spelled(&t, &[None])), ["ya"]);
    }

    #[test]
    fn long_vowel_bar_holds_the_last_vowel() {
        let t = tokens(&["ト", "ー", "ト"]);
        assert_eq!(joined(&spelled(&t, &[None, None, None])), ["to", "o", "to"]);
        // From a kanji's reading too, and nothing at the start of a line.
        let t = tokens(&["ー", "夜"]);
        assert_eq!(
            joined(&spelled(&t, &[None, Some("ヨル".into())])),
            ["-", "yoru"]
        );
    }

    #[test]
    fn latin_words_and_unknown_marks_stay_the_callers() {
        let t = tokens(&["love ", "ゝ", "と"]);
        assert_eq!(joined(&spelled(&t, &[None, None, None])), ["-", "-", "to"]);
        // A っ before a Latin word doubles its first consonant.
        let t = tokens(&["あ", "っ", "baby"]);
        assert_eq!(joined(&spelled(&t, &[None, None, None])), ["a", "b", "-"]);
    }

    #[test]
    fn runs_break_at_spaces_punctuation_and_latin() {
        let t = tokens(&["悠", "々", "と ", "歩", "く", "、", "love ", "1", "1", "月"]);
        assert_eq!(runs(&t), vec![vec![0, 1, 2], vec![3, 4], vec![7, 8, 9]]);
        let t = tokens(&["11", "月", "の"]);
        assert_eq!(runs(&t), vec![vec![0, 1, 2]]);
    }

    #[test]
    fn readings_land_on_their_kanji() {
        let t = tokens(&["水", "面", "に", "映", "る"]);
        let words = [
            word("水面", "名詞", "スイメン"),
            word("に", "助詞", "ニ"),
            word("映る", "動詞", "ウツル"),
        ];
        let mut out = vec![None; t.len()];
        place(&t, &[0, 1, 2, 3, 4], &words, &none, &mut out);
        assert_eq!(
            out,
            [
                Some("スイ".into()),
                Some("メン".into()),
                None,
                Some("ウツ".into()),
                None
            ]
        );
        let s = spelled(&t, &out);
        assert_eq!(joined(&s), ["sui", "men", "ni", "utsu", "ru"]);
    }

    #[test]
    fn okurigana_anchor_the_split() {
        let t = tokens(&["思", "い", "出", "す"]);
        let words = [word("思い出す", "動詞", "オモイダス")];
        let mut out = vec![None; t.len()];
        place(&t, &[0, 1, 2, 3], &words, &none, &mut out);
        assert_eq!(out, [Some("オモ".into()), None, Some("ダ".into()), None]);
    }

    #[test]
    fn compounds_share_by_mora_and_iteration_marks_repeat() {
        let t = tokens(&["一", "人"]);
        let mut out = vec![None; 2];
        place(
            &t,
            &[0, 1],
            &[word("一人", "名詞", "ヒトリ")],
            &none,
            &mut out,
        );
        assert_eq!(out, [Some("ヒト".into()), Some("リ".into())]);
        let t = tokens(&["悠", "々"]);
        let mut out = vec![None; 2];
        place(
            &t,
            &[0, 1],
            &[word("悠々", "副詞", "ユウユウ")],
            &none,
            &mut out,
        );
        assert_eq!(out, [Some("ユウ".into()), Some("ユウ".into())]);
        // A character's own reading wins where the compound starts with it.
        let t = tokens(&["宇", "宙"]);
        let mut out = vec![None; 2];
        let words = [word("宇宙", "名詞", "ウチュウ")];
        place(&t, &[0, 1], &words, &none, &mut out);
        assert_eq!(out, [Some("ウチュ".into()), Some("ウ".into())]);
        let alone = |i: usize| (i == 0).then(|| "う".to_string());
        place(&t, &[0, 1], &words, &alone, &mut out);
        assert_eq!(out, [Some("ウ".into()), Some("チュウ".into())]);
        // Or where it ends with the last one's: 宙 チュウ.
        let alone = |i: usize| (i == 1).then(|| "チュウ".to_string());
        place(&t, &[0, 1], &words, &alone, &mut out);
        assert_eq!(out, [Some("ウ".into()), Some("チュウ".into())]);
        // An own reading that would leave the next character nothing loses.
        let alone = |i: usize| (i == 0).then(|| "ウチュウ".to_string());
        place(&t, &[0, 1], &words, &alone, &mut out);
        assert_eq!(out, [Some("ウチュ".into()), Some("ウ".into())]);
        // Small kana and っ stay with their mora: 一緒 is イッ + ショ.
        assert_eq!(morae("イッショ"), ["イッ", "ショ"]);
        assert_eq!(morae("キョウ"), ["キョ", "ウ"]);
    }

    #[test]
    fn numbers_read_with_their_counter() {
        let t = tokens(&["11", "月", "の", "3", "つ"]);
        let words = [
            word("１１月", "名詞", "ジュウイチガツ"),
            word("の", "助詞", "ノ"),
            word("３つ", "名詞", "ミッツ"),
        ];
        let mut out = vec![None; t.len()];
        let alone = |i: usize| (i == 0).then(|| "ジュウイチ".to_string());
        place(&t, &[0, 1, 2, 3, 4], &words, &alone, &mut out);
        assert_eq!(
            out,
            [
                Some("ジュウイチ".into()),
                Some("ガツ".into()),
                None,
                Some("ミッ".into()),
                None
            ]
        );
        assert_eq!(
            joined(&spelled(&t, &out)),
            ["juuichi", "gatsu", "no", "mit", "tsu"]
        );
    }

    #[test]
    fn particles_wa_and_e() {
        let t = tokens(&["君", "は", "花", "へ"]);
        let words = [
            word("君", "名詞", "キミ"),
            word("は", "助詞", "ハ"),
            word("花", "名詞", "ハナ"),
            word("へ", "助詞", "ヘ"),
        ];
        let mut out = vec![None; t.len()];
        place(&t, &[0, 1, 2, 3], &words, &none, &mut out);
        assert_eq!(joined(&spelled(&t, &out)), ["kimi", "wa", "hana", "e"]);
        // は inside a word is still ha.
        let t = tokens(&["は", "な"]);
        let mut out = vec![None; 2];
        place(
            &t,
            &[0, 1],
            &[word("はな", "名詞", "ハナ")],
            &none,
            &mut out,
        );
        assert_eq!(out, [None, None]);
    }

    #[test]
    fn a_word_that_does_not_fit_gives_nothing() {
        // The dictionary rewrote the text: the rest of the run is uroman's.
        let t = tokens(&["今", "日", "空"]);
        let words = [
            word("今", "名詞", "イマ"),
            word("ひ", "名詞", "ヒ"),
            word("空", "名詞", "ソラ"),
        ];
        let mut out = vec![None; 3];
        place(&t, &[0, 1, 2], &words, &none, &mut out);
        assert_eq!(out, [Some("イマ".into()), None, None]);
        // Kana that disagree with the reading, a reading shorter than its
        // kanji, and an unknown reading give nothing for that word.
        assert!(share(&[('見', 0), ('た', 1)], "ミル", &none).is_empty());
        assert!(share(&[('五', 0), ('月', 1), ('雨', 2)], "サミ", &none).is_empty());
        assert!(share(&[('鬱', 0)], "*", &none).is_empty());
    }

    #[test]
    fn parses_njd_feature_rows() {
        let row = "今日,名詞,副詞可能,*,*,*,*,今日,キョウ,キョー,1/2,C3,0";
        assert_eq!(Morpheme::parse(row), Some(word("今日", "名詞", "キョウ")));
        assert_eq!(Morpheme::parse("x,y"), None);
    }
}
