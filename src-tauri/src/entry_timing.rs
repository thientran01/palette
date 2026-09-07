//! Conservative corrections on separated vocals only; no model or UI state.
use crate::align::{TimeMap, Word};

fn plain(text: &str) -> String {
    text.trim()
        .trim_matches(|c: char| !c.is_alphabetic())
        .to_ascii_lowercase()
}

pub(super) fn repeated_la(tokens: &[String]) -> bool {
    tokens.len() >= 3
        && tokens.len() <= 16
        && tokens
            .iter()
            .all(|t| plain(t) == "la" && !t.contains(['(', ')']))
}

pub(super) fn refine(pcm: &[i16], map: &TimeMap, words: &mut [Word]) {
    if !map.slope_ms.is_finite() || map.slope_ms <= 0.0 || !map.intercept_ms.is_finite() {
        return;
    }
    let sample = |t: i64| {
        (((t as f64 - map.intercept_ms) / map.slope_ms)
            .round()
            .max(0.0) as usize)
            .min(pcm.len())
    };
    let time = |s: usize| (map.intercept_ms + s as f64 * map.slope_ms).round() as i64;
    for i in 0..words.len().saturating_sub(1) {
        let Some(line) = words[i].line_t else {
            continue;
        };
        if (i > 0 && words[i - 1].line_t == Some(line)) || words[i + 1].line_t != Some(line) {
            continue;
        }
        if !matches!(plain(&words[i].text).as_str(), "oh" | "ooh" | "ah")
            || words[i].text.contains(['(', ')'])
        {
            continue;
        }
        let Some(old_end) = words[i].end else {
            continue;
        };
        let previous_end = if i > 0 {
            words[i - 1].end.unwrap_or(words[i - 1].t)
        } else {
            0
        };
        let lo = (line - 500).max(previous_end);
        let hi = words[i + 1].t;
        if hi <= lo || hi - lo > 10_000 || old_end > hi {
            continue;
        }
        let (a, b) = (sample(lo), sample(hi));
        if b.saturating_sub(a) < 320 * 15 {
            continue;
        }
        let envelope: Vec<f64> = pcm[a..b]
            .as_chunks::<320>()
            .0
            .iter()
            .map(|c| (c.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum::<f64>() / 320.0).sqrt())
            .collect();
        let mut ordered = envelope.clone();
        ordered.sort_by(f64::total_cmp);
        let q = (ordered.len() - 1) as f64 * 0.9;
        let p90 = ordered[q.floor() as usize] * (1.0 - q.fract())
            + ordered[q.ceil() as usize] * q.fract();
        let threshold = (p90 * 0.15).max(0.001);
        let mut active: Vec<bool> = envelope.iter().map(|&e| e >= threshold).collect();
        let mut j = 0;
        while j < active.len() {
            if active[j] {
                j += 1;
                continue;
            }
            let start = j;
            while j < active.len() && !active[j] {
                j += 1;
            }
            if start > 0 && j < active.len() && j - start <= 5 {
                active[start..j].fill(true);
            }
        }
        let mut best = (0, 0);
        j = 0;
        while j < active.len() {
            if !active[j] {
                j += 1;
                continue;
            }
            let start = j;
            while j < active.len() && active[j] {
                j += 1;
            }
            if j - start >= 15 && j - start > best.1 - best.0 {
                best = (start, j);
            }
        }
        if best.0 == best.1 {
            continue;
        }
        let start = time(a + best.0 * 320).max(lo);
        let end = time(a + best.1 * 320).min(hi).max(old_end);
        // Entry correction only. Never delay an entrance or shorten its tail.
        if start + 120 > words[i].t || end <= start {
            continue;
        }
        words[i].t = start;
        words[i].end = Some(end);
        // Old spelling checkpoints no longer describe the corrected interval.
        words[i].points.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(text: &str) -> Vec<Word> {
        vec![
            Word {
                text: text.into(),
                t: 2800,
                end: Some(2880),
                line_t: Some(1000),
                ..Default::default()
            },
            Word {
                text: "next".into(),
                t: 3000,
                end: Some(3400),
                line_t: Some(1000),
                ..Default::default()
            },
        ]
    }
    #[test]
    fn sustained_entry_extends_without_moving_neighbor() {
        let mut pcm = vec![0; 64_000];
        pcm[16_000..43_200].fill(5000);
        let mut words = row("Ooh, ");
        let next = words[1].clone();
        refine(&pcm, &TimeMap::from_origin(0, 16000), &mut words);
        assert_eq!(words[0].t, 1000);
        assert_eq!(words[0].end, Some(2880));
        assert_eq!(words[1], next);
    }
    #[test]
    fn silence_short_bursts_and_ordinary_words_are_unchanged() {
        for text in ["Ooh, ", "Still ", "(Oh) "] {
            let mut pcm = vec![0; 64_000];
            pcm[16_000..17_600].fill(5000);
            let mut words = row(text);
            let original = words.clone();
            refine(&pcm, &TimeMap::from_origin(0, 16000), &mut words);
            assert_eq!(words, original);
        }
        let mut words = row("Still ");
        let original = words.clone();
        refine(
            &vec![5000; 64_000],
            &TimeMap::from_origin(0, 16000),
            &mut words,
        );
        assert_eq!(words, original);
    }
    #[test]
    fn repeated_la_excludes_mixed_text_and_parentheses() {
        let tokens = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        assert!(repeated_la(&tokens("La, la, la")));
        for s in ["la la", "la la here", "la la (la)", "Still here still here"] {
            assert!(!repeated_la(&tokens(s)));
        }
    }
}
