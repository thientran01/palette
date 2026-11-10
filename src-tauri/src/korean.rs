//! What a Korean lyric line sounds like, in the aligner's letters.
//!
//! The aligner's tokens are one Hangul syllable each, and uroman spells each
//! one on its own, letter for letter as written. Korean isn't sung as
//! written: a final consonant carries over onto a following vowel, ㅎ melts
//! into its neighbour, and consonants that meet change each other. Spelled a
//! syllable at a time:
//!
//! - 같이 is "gat i", sung "ga ci" (가치); 좋아 is "joh a", sung "jo a".
//! - 없어 is "eobs eo", sung "eob seo"; 있는 is "iss neun", sung "in neun".
//! - 국물 is "gug mul", sung "gung mul"; 몰라 is "mol ra", sung "mol la".
//!
//! `pronounce` applies the regular rules of standard pronunciation
//! (표준 발음법: carry-over, ㅎ before a consonant or vowel,
//! palatalization, nasalization, ㄹ assimilation, final consonants), the
//! ones g2pk applies without a dictionary, inside each written word, and
//! hands back each token's syllable as sung. `spell` turns those into
//! letters with the caller's romanizer, so a sung syllable is spelled
//! exactly as uroman spells it written that way.
//!
//! Two regular rules are left out because they made timing worse on the
//! Children's Song Dataset (docs/specs/2026-11-10-karaoke-korean-spelling.md):
//! tensing after a stop (학교 학꾜) and a stop meeting ㅎ (축하 추카). Both
//! put a hard consonant at the start of the next syllable, and its letters
//! ("gg", "k") take the stop's silent closure with them, so those syllables
//! landed about 40-75 ms early. Written as they are, the closure stays with
//! the syllable before. Rules that need to know the word are left out too
//! (ㄴ inserted in compounds, 의 as a particle, the Sino-Korean exceptions
//! to ㄹ assimilation): a wrong guess there costs more than the written
//! spelling. Every helper returns spellings, never timings.

const LEADS: [char; 19] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ',
    'ㅌ', 'ㅍ', 'ㅎ',
];
const TAILS: [&str; 28] = [
    "", "ㄱ", "ㄲ", "ㄱㅅ", "ㄴ", "ㄴㅈ", "ㄴㅎ", "ㄷ", "ㄹ", "ㄹㄱ", "ㄹㅁ", "ㄹㅂ", "ㄹㅅ",
    "ㄹㅌ", "ㄹㅍ", "ㄹㅎ", "ㅁ", "ㅂ", "ㅂㅅ", "ㅅ", "ㅆ", "ㅇ", "ㅈ", "ㅊ", "ㅋ", "ㅌ", "ㅍ",
    "ㅎ",
];
const VOWELS: u32 = 21;
/// ㅣ and ㅢ in the syllable block's vowel order.
const VOWEL_I: u32 = 20;
const VOWEL_UI: u32 = 19;

#[derive(Clone, Debug, PartialEq)]
struct Syllable {
    lead: char,
    vowel: u32,
    /// The final consonant as written: none, one, or a cluster of two.
    tail: Vec<char>,
}

impl Syllable {
    fn of(c: char) -> Option<Syllable> {
        let code = u32::from(c).checked_sub(0xAC00).filter(|&n| n < 11_172)?;
        Some(Syllable {
            lead: LEADS[(code / (VOWELS * 28)) as usize],
            vowel: code / 28 % VOWELS,
            tail: TAILS[(code % 28) as usize].chars().collect(),
        })
    }

    fn block(&self) -> Option<char> {
        let lead = LEADS.iter().position(|&l| l == self.lead)? as u32;
        let tail: String = self.tail.iter().collect();
        let tail = TAILS.iter().position(|&t| t == tail)? as u32;
        char::from_u32(0xAC00 + (lead * VOWELS + self.vowel) * 28 + tail)
    }
}

fn hangul(token: &str) -> Option<char> {
    let mut chars = token.trim().chars();
    let c = chars.next()?;
    (chars.next().is_none() && Syllable::of(c).is_some()).then_some(c)
}

/// Written words: runs of Hangul tokens with no space or punctuation
/// between them. The tokenizer leaves a following space on the token
/// before it, so a token with any whitespace ends its word.
fn words(tokens: &[String]) -> Vec<Vec<usize>> {
    let mut words = Vec::new();
    let mut word: Vec<usize> = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let joins = word
            .last()
            .is_some_and(|&p| tokens[p].trim() == tokens[p].as_str());
        if !joins && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        if hangul(token).is_some() {
            word.push(i);
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// The sound a final consonant is said as when nothing carries it over:
/// one of ㄱ ㄴ ㄷ ㄹ ㅁ ㅂ ㅇ. `next` is the following syllable's first
/// consonant, which decides ㄺ (맑게 keeps ㄹ, 맑다 keeps ㄱ).
fn said(tail: &[char], next: Option<char>, word: &Syllable) -> Option<char> {
    Some(match *tail {
        [] => return None,
        ['ㄹ', 'ㄱ'] => {
            if next == Some('ㄱ') {
                'ㄹ'
            } else {
                'ㄱ'
            }
        }
        ['ㄹ', 'ㅁ'] => 'ㅁ',
        ['ㄹ', 'ㅍ'] => 'ㅂ',
        // 밟다 is the one ㄼ said as ㅂ.
        ['ㄹ', 'ㅂ'] if word.lead == 'ㅂ' && word.vowel == 0 => 'ㅂ',
        ['ㄹ', _] => 'ㄹ',
        [first, _] => first,
        [c] => match c {
            'ㄱ' | 'ㄲ' | 'ㅋ' => 'ㄱ',
            'ㄷ' | 'ㅅ' | 'ㅆ' | 'ㅈ' | 'ㅊ' | 'ㅌ' | 'ㅎ' => 'ㄷ',
            'ㅂ' | 'ㅍ' => 'ㅂ',
            other => other,
        },
        _ => return None,
    })
}

fn nasal(c: char) -> char {
    match c {
        'ㄱ' => 'ㅇ',
        'ㄷ' => 'ㄴ',
        'ㅂ' => 'ㅁ',
        other => other,
    }
}

/// Two syllables of one word meet: `a`'s final consonant and `b`'s first.
fn meet(a: &mut Syllable, b: &mut Syllable) {
    if a.tail.is_empty() {
        return;
    }
    let before_i = b.vowel == VOWEL_I;
    match b.lead {
        // A vowel carries the final consonant over (ㅎ goes silent, ㅇ
        // stays): 좋아 조아, 읽어 일거, 없어 업서, 같이 가치.
        'ㅇ' => {
            if a.tail.last() == Some(&'ㅎ') {
                a.tail.pop();
            }
            match *a.tail.as_slice() {
                [] | ['ㅇ'] => {}
                // ㄷ and ㅌ before 이 are said ㅈ and ㅊ.
                ['ㄷ' | 'ㅌ'] | ['ㄹ', 'ㅌ'] if before_i => {
                    b.lead = if a.tail.ends_with(&['ㄷ']) {
                        'ㅈ'
                    } else {
                        'ㅊ'
                    };
                    a.tail.pop();
                }
                [first, second] => {
                    b.lead = second;
                    a.tail = vec![first];
                }
                [only] => {
                    b.lead = only;
                    a.tail.clear();
                }
                _ => {}
            }
        }
        // A stop before ㅎ stays a final consonant (see the module doc);
        // ㅎ before ㅎ is one.
        'ㅎ' => {
            if a.tail.last() == Some(&'ㅎ') {
                a.tail.pop();
            }
            let end = said(&a.tail, Some('ㅎ'), a);
            a.tail = end.into_iter().collect();
        }
        lead => {
            if a.tail.last() == Some(&'ㅎ') {
                // ㅎ before a consonant: 좋고 조코, 많다 만타, 좋소 조소,
                // 놓는 논는, 않네 안네.
                a.tail.pop();
                let aspirated = match lead {
                    'ㄱ' => 'ㅋ',
                    'ㄷ' => 'ㅌ',
                    'ㅈ' => 'ㅊ',
                    'ㅅ' => 'ㅅ',
                    _ => {
                        if a.tail.is_empty() {
                            a.tail.push('ㄷ');
                        }
                        consonants(a, b);
                        return;
                    }
                };
                b.lead = aspirated;
                return;
            }
            consonants(a, b);
        }
    }
}

/// A final consonant before another consonant: both said as the rules make
/// them (국물 궁물, 신라 실라, 정리 정니, 백리 뱅니). Tensing is left out
/// (see the module doc).
fn consonants(a: &mut Syllable, b: &mut Syllable) {
    let Some(mut r) = said(&a.tail, Some(b.lead), a) else {
        return;
    };
    let mut l = b.lead;
    let stop = matches!(r, 'ㄱ' | 'ㄷ' | 'ㅂ');
    match (r, l) {
        _ if stop && matches!(l, 'ㄴ' | 'ㅁ') => r = nasal(r),
        _ if stop && l == 'ㄹ' => {
            r = nasal(r);
            l = 'ㄴ';
        }
        ('ㅁ' | 'ㅇ', 'ㄹ') => l = 'ㄴ',
        ('ㄴ', 'ㄹ') => r = 'ㄹ',
        ('ㄹ', 'ㄴ') => l = 'ㄹ',
        _ => {}
    }
    a.tail = vec![r];
    b.lead = l;
}

/// Each token's syllable as sung, or None for a token that isn't one
/// Hangul syllable.
pub fn pronounce(tokens: &[String]) -> Vec<Option<char>> {
    let mut out = vec![None; tokens.len()];
    for word in words(tokens) {
        let mut sung: Vec<Syllable> = word
            .iter()
            .filter_map(|&i| hangul(&tokens[i]).and_then(Syllable::of))
            .collect();
        // ㅢ after a consonant is ㅣ (희망 히망, 띄어 띠어).
        for s in &mut sung {
            if s.vowel == VOWEL_UI && s.lead != 'ㅇ' {
                s.vowel = VOWEL_I;
            }
        }
        for k in 1..sung.len() {
            let (done, rest) = sung.split_at_mut(k);
            meet(&mut done[k - 1], &mut rest[0]);
        }
        if let Some(last) = sung.last_mut() {
            let end = said(&last.tail, None, last);
            last.tail = end.into_iter().collect();
        }
        for (&i, s) in word.iter().zip(&sung) {
            out[i] = s.block().or_else(|| hangul(&tokens[i]));
        }
    }
    out
}

/// Letters for each token of a Korean line, or None where the caller's own
/// spelling stands. `romanize` is how the caller spells one syllable on its
/// own (uroman), so only the sound changes differ from the written
/// spelling. ㄹㄹ is said l-l, which no single syllable spells: the second
/// ㄹ's "r" becomes "l" (몰라 "molla").
pub fn spell(tokens: &[String], romanize: impl Fn(&str) -> String) -> Vec<Option<String>> {
    let sung = pronounce(tokens);
    let mut out = vec![None; tokens.len()];
    for word in words(tokens) {
        let mut tail_l = false;
        for &i in &word {
            let Some(c) = sung[i] else { continue };
            let mut letters = romanize(&c.to_string());
            let s = Syllable::of(c);
            if tail_l && s.as_ref().is_some_and(|s| s.lead == 'ㄹ') && letters.starts_with('r') {
                letters.replace_range(..1, "l");
            }
            tail_l = s.is_some_and(|s| s.tail == ['ㄹ']);
            out[i] = Some(letters);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::tokenize;

    fn sung(text: &str) -> String {
        let tokens = tokenize(text);
        pronounce(&tokens)
            .iter()
            .zip(&tokens)
            .map(|(s, t)| match s {
                Some(c) => {
                    let space = &t[t.trim_end().len()..];
                    format!("{c}{space}")
                }
                None => t.clone(),
            })
            .collect()
    }

    #[test]
    fn blocks_round_trip() {
        for c in ['가', '같', '없', '읽', '힣', '똑', '앉', '밟'] {
            assert_eq!(Syllable::of(c).and_then(|s| s.block()), Some(c));
        }
        assert_eq!(Syllable::of('a'), None);
        assert_eq!(Syllable::of('ㄱ'), None);
    }

    #[test]
    fn final_consonants_carry_over_onto_vowels() {
        assert_eq!(sung("사람이"), "사라미");
        assert_eq!(sung("읽어"), "일거");
        assert_eq!(sung("없어"), "업서");
        assert_eq!(sung("앉아"), "안자");
        assert_eq!(sung("있어"), "이써");
        assert_eq!(sung("젊은"), "절믄");
        // ㅇ stays where it is.
        assert_eq!(sung("사랑을"), "사랑을");
    }

    #[test]
    fn h_melts_into_its_neighbours() {
        assert_eq!(sung("좋아"), "조아");
        assert_eq!(sung("많아"), "마나");
        assert_eq!(sung("싫어"), "시러");
        assert_eq!(sung("좋고"), "조코");
        assert_eq!(sung("많다"), "만타");
        assert_eq!(sung("좋습니다"), "조슴니다");
        assert_eq!(sung("놓는"), "논는");
        assert_eq!(sung("않네"), "안네");
        // A stop before ㅎ keeps it (see the module doc), said plainly.
        assert_eq!(sung("축하해"), "축하해");
        assert_eq!(sung("따뜻해"), "따뜯해");
        assert_eq!(sung("밝히다"), "박히다");
        // ㅎ after a vowel or a sonorant stays.
        assert_eq!(sung("사랑해"), "사랑해");
    }

    #[test]
    fn palatalization() {
        assert_eq!(sung("같이"), "가치");
        assert_eq!(sung("굳이"), "구지");
        assert_eq!(sung("끝이"), "끄치");
    }

    #[test]
    fn nasals_and_liquids() {
        assert_eq!(sung("국물"), "궁물");
        assert_eq!(sung("믿는"), "민는");
        assert_eq!(sung("있는"), "인는");
        assert_eq!(sung("없는"), "엄는");
        assert_eq!(sung("감사합니다"), "감사함니다");
        assert_eq!(sung("정리"), "정니");
        assert_eq!(sung("신라"), "실라");
        assert_eq!(sung("설날"), "설랄");
        assert_eq!(sung("몰라"), "몰라");
    }

    #[test]
    fn no_tensing() {
        // Said 학꾜, 안꼬, 말께, 꼳따발; spelled plain (see the module doc).
        assert_eq!(sung("학교"), "학교");
        assert_eq!(sung("앉고"), "안고");
        assert_eq!(sung("맑게"), "말게");
        assert_eq!(sung("꽃다발"), "꼳다발");
    }

    #[test]
    fn word_final_consonants_are_said_plainly() {
        assert_eq!(sung("꽃"), "꼳");
        assert_eq!(sung("부엌"), "부억");
        assert_eq!(sung("앞"), "압");
        assert_eq!(sung("닭"), "닥");
        assert_eq!(sung("삶"), "삼");
        assert_eq!(sung("여덟"), "여덜");
        assert_eq!(sung("값"), "갑");
    }

    #[test]
    fn ui_after_a_consonant() {
        assert_eq!(sung("희망"), "히망");
        assert_eq!(sung("의미"), "의미");
    }

    #[test]
    fn words_stop_at_spaces_punctuation_and_latin() {
        // Each written word on its own: no carry-over across the space.
        assert_eq!(sung("옷 입어"), "옫 이버");
        assert_eq!(sung("같이,있어"), "가치,이써");
        assert_eq!(sung("널 love해"), "널 love해");
        assert_eq!(sung("Baby 같이"), "Baby 가치");
    }

    #[test]
    fn spells_through_the_romanizer() {
        // A stand-in for uroman: the syllable's jamo letters.
        let romanize = |s: &str| {
            let syl = Syllable::of(s.chars().next().unwrap()).unwrap();
            let lead = [
                "g", "gg", "n", "d", "dd", "r", "m", "b", "bb", "s", "ss", "", "j", "jj",
            ]
            .get(LEADS.iter().position(|&l| l == syl.lead).unwrap())
            .copied()
            .unwrap_or("?");
            let tail: String = syl
                .tail
                .iter()
                .map(|&c| if c == 'ㄹ' { 'l' } else { c })
                .collect();
            format!("{lead}{}{tail}", if syl.vowel == 0 { "a" } else { "o" })
        };
        let tokens = tokenize("몰라 Baby");
        let letters = spell(&tokens, romanize);
        assert_eq!(letters[0].as_deref(), Some("mol"));
        assert_eq!(letters[1].as_deref(), Some("la"));
        assert_eq!(letters[2], None);
        // ㄹ starting a word keeps its r.
        let letters = spell(&tokens[1..2], romanize);
        assert_eq!(letters[0].as_deref(), Some("ra"));
    }
}
