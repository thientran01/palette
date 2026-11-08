//! What a lyric token sounds like when uroman spells it with no letters.
//!
//! uroman reads kana, hangul and most Han characters, but a few marks that
//! every Japanese song uses come back empty: the long-vowel bar (トートロジー)
//! and the iteration marks (悠々). Han numerals (一, 二, 億) and Arabic
//! numbers come back as digits, which the CTC alphabet has no letters for.
//! Before this, any one of those failed the whole song's word sync, on every
//! listen, so most Japanese songs never got karaoke (あぶく, 2026-11-08).
//!
//! Each helper returns text for the romanizer to spell, never timings. A
//! guess here costs one word's spelling inside its line; a failure costs the
//! song.

/// Which language reads the song's numbers. LRC carries no language tag, so
/// the script decides: kana means Japanese even on kana-free lines (半信半疑),
/// then hangul, then Han.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Numerals {
    Japanese,
    Korean,
    Mandarin,
    English,
}

impl Numerals {
    pub fn for_song<'a>(texts: impl IntoIterator<Item = &'a str>) -> Numerals {
        let (mut kana, mut hangul, mut han) = (false, false, false);
        for c in texts.into_iter().flat_map(str::chars) {
            match u32::from(c) {
                // ・ (30FB) also shows up in Korean and Chinese titles.
                0x30FB => {}
                0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => kana = true,
                0xAC00..=0xD7A3 | 0x1100..=0x11FF | 0x3130..=0x318F => hangul = true,
                0x4E00..=0x9FFF | 0x3400..=0x4DBF => han = true,
                _ => {}
            }
        }
        if kana {
            Numerals::Japanese
        } else if hangul {
            Numerals::Korean
        } else if han {
            Numerals::Mandarin
        } else {
            Numerals::English
        }
    }
}

const ITERATION: [char; 6] = ['々', '〻', 'ゝ', 'ヽ', 'ゞ', 'ヾ'];
const LONG_VOWEL: [char; 2] = ['ー', 'ｰ'];

/// Spoken text for `token`, which uroman spelled with no letters. `prev` is
/// the previous token on the line and `prev_latin` its spelling; `next` is
/// the token after it. None when nothing sensible is left to say.
pub fn fallback(
    token: &str,
    prev: Option<&str>,
    prev_latin: Option<&str>,
    next: Option<&str>,
    numerals: Numerals,
) -> Option<String> {
    let core = token.trim_matches(|c: char| !c.is_alphanumeric());
    if core.is_empty() {
        return None;
    }
    if core.chars().all(|c| ITERATION.contains(&c)) {
        // 悠々 is 悠悠: say the previous character again.
        let prev = prev?.trim_matches(|c: char| !c.is_alphanumeric());
        return (!prev.is_empty()).then(|| prev.repeat(core.chars().count()));
    }
    if core.chars().all(|c| LONG_VOWEL.contains(&c)) {
        // トー holds ト's vowel. ンー or a line-initial bar has none.
        let vowel = prev_latin?.chars().rev().find(|c| "aeiou".contains(*c))?;
        return Some(vowel.to_string().repeat(core.chars().count()));
    }
    if core == "〆" {
        return Some("しめ".into());
    }
    if core.chars().all(|c| han_numeral(c, numerals).is_some()) {
        return Some(
            core.chars()
                .filter_map(|c| han_numeral(c, numerals))
                .collect(),
        );
    }
    if core.chars().any(|c| c.is_ascii_digit()) {
        // 7個目 and 11년째 read the number in the song's language. A number
        // standing apart (Lucky 7, "I was 11") is most often sung in English.
        let glued = !token.ends_with(char::is_whitespace)
            && next
                .and_then(|n| n.chars().next())
                .is_some_and(|c| c.is_alphabetic() && !c.is_ascii());
        let lang = if glued { numerals } else { Numerals::English };
        return Some(spell_digit_runs(core, lang));
    }
    None
}

/// Every run of ASCII digits read as a number, other letters kept, the
/// rest (24/7's slash) dropped.
fn spell_digit_runs(core: &str, lang: Numerals) -> String {
    let mut out = String::new();
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if !run.is_empty() {
            out.push_str(&number(run, lang));
            out.push(' ');
            run.clear();
        }
    };
    for c in core.chars() {
        if c.is_ascii_digit() {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            if c.is_alphabetic() {
                out.push(c);
            } else {
                out.push(' ');
            }
        }
    }
    flush(&mut run, &mut out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A digit string read aloud. Leading zeros and very long numbers (phone
/// numbers, 134340) are read digit by digit.
fn number(digits: &str, lang: Numerals) -> String {
    let value = digits.parse::<u64>().ok();
    let whole = match (value, lang) {
        (Some(n), Numerals::English) if digits.len() <= 4 && !leading_zero(digits) => {
            Some(english(n))
        }
        (Some(n), _) if digits.len() <= 8 && !leading_zero(digits) && lang != Numerals::English => {
            Some(cjk(n, lang))
        }
        _ => None,
    };
    whole.unwrap_or_else(|| {
        digits
            .chars()
            .map(|c| digit_word(c.to_digit(10).unwrap_or(0) as u64, lang))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

fn leading_zero(digits: &str) -> bool {
    digits.len() > 1 && digits.starts_with('0')
}

const ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

fn english(n: u64) -> String {
    match n {
        0..=19 => ONES[n as usize].into(),
        20..=99 => match n % 10 {
            0 => TENS[(n / 10) as usize].into(),
            r => format!("{} {}", TENS[(n / 10) as usize], ONES[r as usize]),
        },
        100..=999 => match n % 100 {
            0 => format!("{} hundred", ONES[(n / 100) as usize]),
            r => format!("{} hundred {}", ONES[(n / 100) as usize], english(r)),
        },
        // Years are sung in pairs: nineteen ninety-nine, twenty twenty.
        1100..=1999 | 2010..=2099 if !n.is_multiple_of(100) => {
            let low = n % 100;
            let low = if low < 10 {
                format!("oh {}", ONES[low as usize])
            } else {
                english(low)
            };
            format!("{} {low}", english(n / 100))
        }
        _ => match n % 1000 {
            0 => format!("{} thousand", english(n / 1000)),
            r => format!("{} thousand {}", english(n / 1000), english(r)),
        },
    }
}

/// Japanese and Korean come back as kana and hangul for uroman to spell, so
/// they sound like the rest of the song; Mandarin is pinyin (uroman turns Han
/// numerals into digits).
fn digit_word(d: u64, lang: Numerals) -> &'static str {
    const JA: [&str; 10] = [
        "ぜろ",
        "いち",
        "に",
        "さん",
        "よん",
        "ご",
        "ろく",
        "なな",
        "はち",
        "きゅう",
    ];
    const KO: [&str; 10] = ["영", "일", "이", "삼", "사", "오", "육", "칠", "팔", "구"];
    const ZH: [&str; 10] = [
        "ling", "yi", "er", "san", "si", "wu", "liu", "qi", "ba", "jiu",
    ];
    let d = d as usize % 10;
    match lang {
        Numerals::Japanese => JA[d],
        Numerals::Korean => KO[d],
        Numerals::Mandarin => ZH[d],
        Numerals::English => ONES[d],
    }
}

/// Ten, hundred, thousand, ten thousand.
fn units(lang: Numerals) -> [&'static str; 4] {
    match lang {
        Numerals::Japanese => ["じゅう", "ひゃく", "せん", "まん"],
        Numerals::Korean => ["십", "백", "천", "만"],
        _ => ["shi", "bai", "qian", "wan"],
    }
}

/// Positional reading below 10^8, the way all three languages count: 2024 is
/// two-thousand zero-hundred two-ten four. One before ten, hundred or
/// thousand is silent (十, 百, 千), but not before 万.
fn cjk(n: u64, lang: Numerals) -> String {
    if n == 0 {
        return digit_word(0, lang).into();
    }
    let [ten, hundred, thousand, man] = units(lang);
    let below_man = |n: u64| {
        let mut s = String::new();
        for (place, unit) in [(1000, thousand), (100, hundred), (10, ten)] {
            let d = n / place % 10;
            if d > 1 {
                s.push_str(digit_word(d, lang));
            }
            if d > 0 {
                s.push_str(unit);
            }
        }
        if !n.is_multiple_of(10) {
            s.push_str(digit_word(n % 10, lang));
        }
        s
    };
    let mut out = String::new();
    if n >= 10_000 {
        out.push_str(&below_man(n / 10_000));
        out.push_str(man);
    }
    out.push_str(&below_man(n % 10_000));
    if lang == Numerals::Japanese {
        // Rendaku the way it is sung: sanbyaku, roppyaku, happyaku, sanzen.
        for (from, to) in [
            ("さんひゃく", "さんびゃく"),
            ("ろくひゃく", "ろっぴゃく"),
            ("はちひゃく", "はっぴゃく"),
            ("さんせん", "さんぜん"),
            ("はちせん", "はっせん"),
        ] {
            out = out.replace(from, to);
        }
    }
    out
}

/// A Han numeral read on its own. Each Han character is its own token, so
/// 二十 arrives as 二 then 十, and ni + juu is how it is sung.
fn han_numeral(c: char, lang: Numerals) -> Option<&'static str> {
    let value = match c {
        '〇' | '零' => 0,
        '一' | '壱' | '壹' | '弌' => 1,
        '二' | '弐' | '貳' | '贰' | '弍' | '兩' => 2,
        '三' | '参' | '參' | '叁' | '叄' | '弎' => 3,
        '四' | '肆' => 4,
        '五' | '伍' => 5,
        '六' | '陆' | '陸' => 6,
        '七' | '柒' | '漆' => 7,
        '八' | '捌' => 8,
        '九' | '玖' => 9,
        '十' | '拾' | '什' => 10,
        '百' | '佰' | '陌' => 100,
        '千' | '仟' | '阡' => 1000,
        '万' | '萬' => 10_000,
        '億' | '亿' => 100_000_000,
        '兆' => 1_000_000_000_000,
        _ => return None,
    };
    let lang = if lang == Numerals::English {
        Numerals::Mandarin
    } else {
        lang
    };
    Some(match value {
        0 if lang == Numerals::Japanese => "れい",
        0..=9 => digit_word(value, lang),
        10 => units(lang)[0],
        100 => units(lang)[1],
        1000 => units(lang)[2],
        10_000 => units(lang)[3],
        100_000_000 => match lang {
            Numerals::Japanese => "おく",
            Numerals::Korean => "억",
            _ => "yi",
        },
        _ => match lang {
            Numerals::Japanese => "ちょう",
            Numerals::Korean => "조",
            _ => "zhao",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_picks_the_song_language() {
        assert_eq!(
            Numerals::for_song(["半信半疑", "あぶく"]),
            Numerals::Japanese
        );
        assert_eq!(Numerals::for_song(["11년째 독학 중"]), Numerals::Korean);
        assert_eq!(Numerals::for_song(["我愛你"]), Numerals::Mandarin);
        assert_eq!(Numerals::for_song(["Lucky 7"]), Numerals::English);
        assert_eq!(Numerals::for_song(["사랑・Love"]), Numerals::Korean);
    }

    #[test]
    fn marks_borrow_the_previous_sound() {
        let ja = Numerals::Japanese;
        assert_eq!(
            fallback("々", Some("悠"), Some("you"), None, ja).as_deref(),
            Some("悠")
        );
        assert_eq!(
            fallback("ー", Some("ト"), Some("to"), None, ja).as_deref(),
            Some("o")
        );
        assert_eq!(
            fallback("ー ", Some("ジ"), Some("ji"), None, ja).as_deref(),
            Some("i")
        );
        assert_eq!(fallback("ー", None, None, None, ja), None);
        assert_eq!(fallback("ー", Some("ン"), Some("n"), None, ja), None);
        assert_eq!(fallback("々", None, None, None, ja), None);
    }

    #[test]
    fn numbers_read_in_the_language_around_them() {
        let ja = Numerals::Japanese;
        let ko = Numerals::Korean;
        let say = |t: &str, next: Option<&str>, lang| fallback(t, None, None, next, lang).unwrap();
        assert_eq!(say("7", Some("個"), ja), "なな");
        assert_eq!(say("11", Some("년"), ko), "십일");
        assert_eq!(say("4", Some("월"), ko), "사");
        assert_eq!(say("7 ", Some("個"), ja), "seven");
        assert_eq!(say("5, ", Some("4"), ko), "five");
        assert_eq!(say("11, ", Some("smoking"), Numerals::English), "eleven");
        assert_eq!(say("20 ", None, Numerals::English), "twenty");
        assert_eq!(say("24/7", None, Numerals::English), "twenty four seven");
        assert_eq!(say("1999", None, Numerals::English), "nineteen ninety nine");
        assert_eq!(say("2007", None, Numerals::English), "two thousand seven");
        assert_eq!(say("1905", None, Numerals::English), "nineteen oh five");
        assert_eq!(
            say("134340", None, Numerals::English),
            "one three four three four zero"
        );
        assert_eq!(say("007", None, Numerals::English), "zero zero seven");
        assert_eq!(say("300", Some("円"), ja), "さんびゃく");
        assert_eq!(say("2024", Some("年"), ja), "にせんにじゅうよん");
        assert_eq!(say("10000", Some("円"), ja), "いちまん");
    }

    #[test]
    fn han_numerals_follow_the_song() {
        let say = |t: &str, lang| fallback(t, None, None, None, lang).unwrap();
        assert_eq!(say("一", Numerals::Japanese), "いち");
        assert_eq!(say("十", Numerals::Japanese), "じゅう");
        assert_eq!(say("億", Numerals::Japanese), "おく");
        assert_eq!(say("零", Numerals::Japanese), "れい");
        assert_eq!(say("拾", Numerals::Japanese), "じゅう");
        assert_eq!(say("一", Numerals::Mandarin), "yi");
        assert_eq!(say("二", Numerals::Korean), "이");
        assert_eq!(say("〆", Numerals::Japanese), "しめ");
        assert_eq!(fallback("、", None, None, None, Numerals::Japanese), None);
    }
}
