//! Forced alignment of known lyric tokens onto recorded PCM.

use serde::{Deserialize, Serialize};

const HOP_MS: i64 = 10;
const PEAK_FLOOR: f32 = 0.02;
const VOICE_REL: f32 = 0.12;
const MIN_SPAN_MS: i64 = 80;

#[derive(Clone, Debug, PartialEq)]
pub struct TimedLine {
    pub t: i64,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Word {
    pub t: i64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
}

pub fn parse_lrc(lrc: &str) -> Vec<TimedLine> {
    let mut lines = Vec::new();
    for raw in lrc.split('\n') {
        let raw = raw.trim_end_matches('\r');
        let (stamps, rest) = split_stamps(raw);
        let text = rest.trim();
        if text.is_empty() {
            continue;
        }
        for t in stamps {
            lines.push(TimedLine {
                t,
                text: text.to_string(),
            });
        }
    }
    lines.sort_by_key(|l| l.t);
    lines.truncate(600);
    lines
}

fn split_stamps(raw: &str) -> (Vec<i64>, &str) {
    let mut i = 0;
    let mut stamps = Vec::new();
    let bytes = raw.as_bytes();
    while i < bytes.len() && bytes[i] == b'[' {
        let rest = &raw[i + 1..];
        let Some(close) = rest.find(']') else {
            break;
        };
        let inner = &rest[..close];
        match parse_stamp(inner) {
            Some(ms) => {
                stamps.push(ms);
                i += close + 2;
            }
            None => break,
        }
    }
    (stamps, &raw[i..])
}

fn parse_stamp(s: &str) -> Option<i64> {
    let (mm, rest) = s.split_once(':')?;
    let minutes: i64 = mm.parse().ok()?;
    let sec: f64 = rest.parse().ok()?;
    if !(0.0..60.0).contains(&sec) {
        return None;
    }
    Some(((minutes as f64 * 60.0 + sec) * 1000.0).round() as i64)
}

pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut latin = String::new();
    for c in text.chars() {
        if is_syllable_char(c) {
            flush_latin(&mut latin, &mut out);
            out.push(c.to_string());
        } else if c.is_whitespace() {
            if latin.is_empty() {
                if let Some(last) = out.last_mut() {
                    last.push(c);
                }
            } else {
                latin.push(c);
                out.push(std::mem::take(&mut latin));
            }
        } else {
            latin.push(c);
        }
    }
    if !latin.is_empty() {
        out.push(latin);
    }
    out.retain(|t| t.chars().any(|c| !c.is_whitespace()));
    out
}

fn flush_latin(latin: &mut String, out: &mut Vec<String>) {
    if !latin.is_empty() {
        out.push(std::mem::take(latin));
    }
}

fn is_syllable_char(c: char) -> bool {
    matches!(
        u32::from(c),
        0xAC00..=0xD7A3 | 0x1100..=0x11FF | 0x3130..=0x318F
            | 0x4E00..=0x9FFF | 0x3400..=0x4DBF
            | 0x3040..=0x30FF
    )
}

fn token_weight(tok: &str) -> f32 {
    let t = tok.trim();
    if t.is_empty() {
        return 0.05;
    }
    if t.chars().all(is_syllable_char) {
        return t.chars().count() as f32;
    }
    let n = t.chars().count() as f32;
    1.0 + (n - 1.0) * 0.12
}

pub fn align(pcm: &[f32], sample_rate: u32, lines: &[TimedLine], origin_ms: i64) -> Vec<Word> {
    if sample_rate == 0 || pcm.is_empty() || lines.is_empty() {
        return Vec::new();
    }
    let pcm_end_ms = origin_ms + (pcm.len() as i64 * 1000) / sample_rate as i64;
    let mut words = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let next_t = lines.get(i + 1).map(|n| n.t).unwrap_or(pcm_end_ms);
        if let Some(ws) = align_line(pcm, sample_rate, line, next_t, origin_ms, pcm_end_ms) {
            words.extend(ws);
        }
    }
    words
}

fn align_line(
    pcm: &[f32],
    sample_rate: u32,
    line: &TimedLine,
    next_t: i64,
    origin_ms: i64,
    pcm_end_ms: i64,
) -> Option<Vec<Word>> {
    let tokens = tokenize(&line.text);
    if tokens.is_empty() {
        return None;
    }
    let win_lo = line.t.max(origin_ms);
    let win_hi = next_t.min(pcm_end_ms);
    if win_hi - win_lo < MIN_SPAN_MS {
        return None;
    }
    let env = envelope(pcm, sample_rate, win_lo - origin_ms, win_hi - origin_ms)?;
    place_tokens(&tokens, &env, win_lo)
}

struct Env {
    hop_ms: i64,
    values: Vec<f32>,
}

fn envelope(pcm: &[f32], sample_rate: u32, start_ms: i64, end_ms: i64) -> Option<Env> {
    let sr = sample_rate as i64;
    let hop = (sr * HOP_MS / 1000).max(1);
    let win = hop * 2;
    let start = ((start_ms.max(0) * sr) / 1000) as usize;
    let end = ((end_ms.max(0) * sr) / 1000).clamp(0, pcm.len() as i64) as usize;
    if end <= start + win as usize {
        return None;
    }
    let mut values = Vec::new();
    let mut prev = 0.0f32;
    let mut i = start;
    while i + win as usize <= end {
        let mut sum = 0.0f32;
        for k in 0..win as usize {
            let x = pcm[i + k];
            let hp = x - prev;
            prev = x;
            sum += hp * hp;
        }
        values.push((sum / win as f32).sqrt());
        i += hop as usize;
    }
    if values.is_empty() {
        return None;
    }
    Some(Env {
        hop_ms: HOP_MS,
        values,
    })
}

fn place_tokens(tokens: &[String], env: &Env, win_lo: i64) -> Option<Vec<Word>> {
    let peak = env.values.iter().copied().fold(0.0f32, f32::max);
    if peak < PEAK_FLOOR {
        return None;
    }
    let floor = peak * VOICE_REL;
    let first = env.values.iter().position(|&v| v >= floor)?;
    let last = env
        .values
        .iter()
        .rposition(|&v| v >= floor)
        .unwrap_or(first);
    if (last.saturating_sub(first) as i64) * env.hop_ms < MIN_SPAN_MS {
        return None;
    }
    let span = &env.values[first..=last];
    let weights: Vec<f32> = tokens.iter().map(|t| token_weight(t)).collect();
    let total_w: f32 = weights.iter().sum();
    if total_w <= 0.0 {
        return None;
    }
    let mut cum = Vec::with_capacity(span.len() + 1);
    let mut acc = 0.0f32;
    cum.push(0.0);
    for &v in span {
        acc += v.max(0.0);
        cum.push(acc);
    }
    if acc <= f32::EPSILON {
        return None;
    }
    let onsets = pick_rises(span, floor, env.hop_ms);
    if onsets.len() >= 2 {
        return Some(place_on_onsets(
            tokens, &onsets, win_lo, first, last, env.hop_ms,
        ));
    }
    let mut words = Vec::with_capacity(tokens.len());
    let mut consumed = 0.0f32;
    let mut prev_t: Option<i64> = None;
    for (tok, weight) in tokens.iter().zip(weights.iter()) {
        let target = (consumed / total_w) * acc;
        let frame = cum
            .iter()
            .position(|&c| c >= target)
            .unwrap_or(0)
            .min(span.len().saturating_sub(1));
        consumed += *weight;
        let next_target = (consumed / total_w) * acc;
        let end_frame = cum
            .iter()
            .position(|&c| c >= next_target)
            .unwrap_or(span.len())
            .min(span.len());
        let mut t = win_lo + (first as i64 + frame as i64) * env.hop_ms;
        if let Some(prev) = prev_t {
            t = t.max(prev + env.hop_ms);
        }
        let end = (win_lo + (first as i64 + end_frame as i64) * env.hop_ms).max(t + env.hop_ms);
        prev_t = Some(t);
        words.push(Word {
            t,
            text: tok.clone(),
            end: Some(end),
        });
    }
    Some(words)
}

fn place_on_onsets(
    tokens: &[String],
    onsets: &[usize],
    win_lo: i64,
    first: usize,
    last: usize,
    hop_ms: i64,
) -> Vec<Word> {
    let k = onsets.len();
    let n = tokens.len();
    let island_end = win_lo + last as i64 * hop_ms + hop_ms;
    let stamp = |frame: usize| win_lo + (first as i64 + frame as i64) * hop_ms;
    let at = |i: usize| {
        if n == 1 {
            return onsets[0];
        }
        let den = n - 1;
        let num = i * (k - 1);
        let lo = (num / den).min(k - 1);
        let rem = num % den;
        let a = onsets[lo];
        let b = onsets[(lo + 1).min(k - 1)];
        a + (b.saturating_sub(a)) * rem / den
    };
    let mut words = Vec::with_capacity(n);
    for (i, tok) in tokens.iter().enumerate() {
        let t = stamp(at(i));
        let end = if i + 1 < n {
            stamp(at(i + 1))
        } else {
            island_end
        };
        words.push(Word {
            t,
            text: tok.clone(),
            end: Some(end.max(t + hop_ms)),
        });
    }
    words
}

fn pick_rises(span: &[f32], floor: f32, hop_ms: i64) -> Vec<usize> {
    let min_gap = (70 / hop_ms).max(1) as usize;
    let mut out = Vec::new();
    let mut last: Option<usize> = None;
    for i in 0..span.len() {
        let prev = if i == 0 { 0.0 } else { span[i - 1] };
        if span[i] < floor || prev >= floor {
            continue;
        }
        if let Some(j) = last {
            if i - j < min_gap {
                continue;
            }
        }
        out.push(i);
        last = Some(i);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(sr: u32, start_ms: i64, end_ms: i64, hz: f32, out: &mut [f32]) {
        let sr_f = sample_rate(sr);
        let lo = ((start_ms * sr as i64) / 1000) as usize;
        let hi = ((end_ms * sr as i64) / 1000).min(out.len() as i64) as usize;
        for (i, s) in out.iter_mut().enumerate().take(hi).skip(lo) {
            let t = i as f32 / sr_f;
            *s = (t * hz * std::f32::consts::TAU).sin() * 0.4;
        }
    }

    fn sample_rate(sr: u32) -> f32 {
        sr as f32
    }

    #[test]
    fn words_land_on_vocal_bursts() {
        let sr = 16_000u32;
        let mut pcm = vec![0.0f32; sr as usize * 4];
        tone(sr, 1000, 1200, 1000.0, &mut pcm);
        tone(sr, 1300, 1500, 1200.0, &mut pcm);
        tone(sr, 1600, 1800, 1400.0, &mut pcm);
        let lines = parse_lrc("[00:01.00]one two three\n[00:03.00]next");
        let words = align(&pcm, sr, &lines, 0);
        assert_eq!(words.len(), 3, "{words:?}");
        assert_eq!(words[0].text.trim(), "one");
        assert_eq!(words[1].text.trim(), "two");
        assert_eq!(words[2].text.trim(), "three");
        assert!(
            (words[0].t - 1000).abs() <= 40,
            "first word at {}, want ~1000",
            words[0].t
        );
        assert!(
            (words[1].t - 1300).abs() <= 40,
            "second word at {}, want ~1300",
            words[1].t
        );
        assert!(
            (words[2].t - 1600).abs() <= 40,
            "third word at {}, want ~1600",
            words[2].t
        );
        assert!(
            words[2].t < 2000,
            "must not leak into the silent tail: {}",
            words[2].t
        );
    }

    #[test]
    fn silent_line_emits_no_words() {
        let sr = 16_000u32;
        let pcm = vec![0.0f32; sr as usize * 3];
        let lines = parse_lrc("[00:01.00]hello world\n[00:02.00]next");
        assert!(align(&pcm, sr, &lines, 0).is_empty());
    }

    #[test]
    fn hangul_tokens_are_syllables() {
        assert_eq!(tokenize("사랑해"), vec!["사", "랑", "해"]);
        assert_eq!(tokenize("hello 사랑"), vec!["hello ", "사", "랑"]);
    }

    #[test]
    fn latin_keeps_trailing_space() {
        assert_eq!(tokenize("one two three"), vec!["one ", "two ", "three"]);
    }

    #[test]
    fn connected_island_still_orders_tokens() {
        let sr = 16_000u32;
        let mut pcm = vec![0.0f32; sr as usize * 3];
        tone(sr, 1000, 1900, 1000.0, &mut pcm);
        let lines = parse_lrc("[00:01.00]one two three\n[00:02.50]next");
        let words = align(&pcm, sr, &lines, 0);
        assert_eq!(words.len(), 3);
        assert!(words[0].t < words[1].t && words[1].t < words[2].t);
        assert!(words[0].t >= 980 && words[0].t <= 1120);
        assert!(words[2].t <= 1920);
    }

    #[test]
    fn word_starts_snap_to_burst_attacks() {
        let sr = 16_000u32;
        let mut pcm = vec![0.0f32; sr as usize * 3];
        tone(sr, 1000, 1180, 1000.0, &mut pcm);
        tone(sr, 1600, 1780, 1200.0, &mut pcm);
        let lines = parse_lrc("[00:01.00]one two\n[00:02.50]next");
        let words = align(&pcm, sr, &lines, 0);
        assert_eq!(words.len(), 2, "{words:?}");
        assert!(
            (words[0].t - 1000).abs() <= 40,
            "first attack at {}, want ~1000",
            words[0].t
        );
        assert!(
            (words[1].t - 1600).abs() <= 40,
            "second attack at {}, want ~1600",
            words[1].t
        );
    }

    #[test]
    fn parse_lrc_skips_empty_markers() {
        let lines = parse_lrc("[00:01.00]verse\n[00:04.00] \n[00:08.00]next");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "verse");
        assert_eq!(lines[1].t, 8000);
    }
}
