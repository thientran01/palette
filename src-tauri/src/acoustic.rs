//! Local MMS_FA acoustic alignment. No labels, network, or UI clock access.
//! The caller supplies a 16 kHz recording and its capture-to-song time map.
use crate::align::{tokenize, TimeMap, TimedLine, Word, WordPoint};
use ort::{session::Session, value::Tensor};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::OnceLock,
};

// Explicit model vocabulary order, independent of dictionary iteration order.
const LABELS: &[u8] = b"-aienoutsrmkldghybpwcvjzf'qx*";
const CLASSES: usize = 29;
const STAR: usize = 28;

/// Recipe pins the exported weights, normalization, window and decoder.
pub const RECIPE: &str = "mms-int8/1";
pub const MODEL_SHA256: &str = "50128ba8db1150101b9e7d3610cdfda5c8fd2637b918d151ff6aa6fde2b9c2de";
pub const RUNTIME_SHA256: &str = "c7151fd9844ad7c7d18525f1177e9ef62d91e4a6ac3583d0be700554a2b2b1d6";
static ASSETS: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Install writes the enable marker last; discovery performs no model load.
/// Session configuration is immutable until restart, including cache identity.
pub fn configure(dir: PathBuf) {
    let enabled = dir.join("enabled").is_file();
    let _ = ASSETS.set(enabled.then_some(dir));
    log::info!(
        "karaoke: acoustic model {}",
        if enabled {
            "enabled (mms-int8/1)"
        } else {
            "not installed"
        }
    );
}

pub fn assets() -> Option<&'static Path> {
    ASSETS.get().and_then(|v| v.as_deref())
}

fn verify_asset(path: &Path, expected: &str) -> Result<()> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    if format!("{:x}", hash.finalize()) != expected {
        return Err(format!(
            "acoustic asset checksum mismatch: {}",
            path.display()
        ));
    }
    Ok(())
}

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct TokenSpan {
    pub start: usize,
    pub end: usize,
    pub confidence: f32,
}

/// CTC Viterbi over blank-interleaved target states. Repeated letters must
/// cross a blank, while unlike letters may advance directly. Memory is
/// bounded before allocation; malformed or impossible input is rejected.
pub fn ctc_spans(logp: &[f32], classes: usize, targets: &[usize]) -> Result<Vec<TokenSpan>> {
    if classes < 2
        || logp.is_empty()
        || !logp.len().is_multiple_of(classes)
        || targets.is_empty()
        || targets.iter().any(|&x| x == 0 || x >= classes)
        || logp.iter().any(|x| !x.is_finite())
    {
        return Err("invalid CTC input".into());
    }
    let frames = logp.len() / classes;
    let repeats = targets.windows(2).filter(|w| w[0] == w[1]).count();
    if targets.len() + repeats > frames {
        return Err("CTC target exceeds audio window".into());
    }
    let states = targets.len() * 2 + 1;
    let cells = frames
        .checked_mul(states)
        .filter(|&n| n <= 4_000_000)
        .ok_or("CTC window exceeds memory bound")?;
    let mut back = vec![0u8; cells];
    let mut prev = vec![f32::NEG_INFINITY; states];
    let mut next = prev.clone();
    prev[0] = logp[0];
    prev[1] = logp[targets[0]];
    for t in 1..frames {
        next.fill(f32::NEG_INFINITY);
        for s in 0..states {
            let label = if s % 2 == 0 { 0 } else { targets[s / 2] };
            let mut best = prev[s];
            let mut step = 0;
            if s > 0 && prev[s - 1] > best {
                best = prev[s - 1];
                step = 1;
            }
            if s > 1 && s % 2 == 1 && targets[s / 2] != targets[s / 2 - 1] && prev[s - 2] > best {
                best = prev[s - 2];
                step = 2;
            }
            next[s] = best + logp[t * classes + label];
            back[t * states + s] = step;
        }
        std::mem::swap(&mut prev, &mut next);
    }
    let mut state = if prev[states - 1] > prev[states - 2] {
        states - 1
    } else {
        states - 2
    };
    if !prev[state].is_finite() {
        return Err("no CTC path".into());
    }
    let mut spans = vec![
        TokenSpan {
            start: frames,
            end: 0,
            confidence: 0.0
        };
        targets.len()
    ];
    for t in (0..frames).rev() {
        if state % 2 == 1 {
            let i = state / 2;
            spans[i].start = t;
            spans[i].end = spans[i].end.max(t + 1);
            spans[i].confidence += logp[t * classes + targets[i]].exp();
        }
        if t > 0 {
            state -= back[t * states + state] as usize;
        }
    }
    for span in &mut spans {
        if span.end <= span.start {
            return Err("incomplete CTC path".into());
        }
        span.confidence /= (span.end - span.start) as f32;
    }
    Ok(spans)
}

// ort rc10 can panic when Windows cannot load a checksum-valid DLL's
// dependencies. Preserve the worker's error/dump/retry path in that case.
fn guard_runtime<T>(load: impl FnOnce() -> Result<T> + std::panic::UnwindSafe) -> Result<T> {
    std::panic::catch_unwind(load).map_err(|_| "ONNX Runtime initialization failed".to_string())?
}

struct TokenPlan {
    tokens: Vec<String>,
    targets: Vec<usize>,
    ranges: Vec<Option<std::ops::Range<usize>>>,
}

fn expand_digit(token: &str) -> String {
    const NAMES: [&str; 10] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    ];
    // Multi-digit values need language/context (12 != one two); leave them
    // to the existing romanizer rather than inventing a pronunciation.
    if token.chars().filter(char::is_ascii_digit).count() != 1 {
        return token.to_string();
    }
    token
        .chars()
        .map(|c| {
            if c.is_ascii_digit() {
                NAMES[c as usize - '0' as usize].to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

fn plan_tokens(tokens: Vec<String>, romanizer: &uroman::Uroman) -> Result<TokenPlan> {
    let mut targets = vec![STAR];
    let mut ranges = Vec::new();
    // No reliable language tag accompanies LRC. Require ASCII plus multiple
    // English context words before assigning English names to bare digits.
    // 4sho is an explicit English rebus, not a language-wide number rule.
    let english_context = tokens.iter().all(|t| t.is_ascii())
        && tokens
            .iter()
            .filter(|t| {
                matches!(
                    t.trim()
                        .trim_matches(|c: char| !c.is_alphabetic())
                        .to_ascii_lowercase()
                        .as_str(),
                    "the"
                        | "we"
                        | "all"
                        | "my"
                        | "you"
                        | "your"
                        | "fingers"
                        | "door"
                        | "doors"
                        | "know"
                        | "this"
                        | "for"
                        | "and"
                )
            })
            .count()
            >= 2;
    for token in &tokens {
        // Preserve display text while giving isolated digits (and 4sho-style
        // spellings) an English spoken target. Never drop a spoken digit.
        let spoken = if english_context || token.trim().eq_ignore_ascii_case("4sho") {
            expand_digit(token)
        } else {
            token.clone()
        };
        let normalized: String = romanizer
            .romanize_string::<uroman::rom_format::Str>(&spoken, None)
            .to_string()
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_lowercase() || *c == '\'')
            .collect();
        if normalized.is_empty() || !token.chars().any(char::is_alphanumeric) {
            // Nonverbal punctuation keeps its display slot but has no CTC
            // target. Unsupported letters must not silently become punctuation.
            if token.chars().any(char::is_alphanumeric) {
                return Err("unrepresentable spoken token".into());
            }
            ranges.push(None);
            continue;
        }
        let start = targets.len();
        for c in normalized.bytes() {
            targets.push(
                LABELS
                    .iter()
                    .position(|&b| b == c)
                    .ok_or("unknown model symbol")?,
            );
        }
        ranges.push(Some(start..targets.len()));
    }
    targets.push(STAR);
    Ok(TokenPlan {
        tokens,
        targets,
        ranges,
    })
}

fn word_frames(
    ranges: &[Option<std::ops::Range<usize>>],
    spans: &[TokenSpan],
) -> Vec<(usize, usize)> {
    ranges
        .iter()
        .enumerate()
        .map(|(i, range)| {
            if let Some(r) = range {
                return (spans[r.start].start, spans[r.end - 1].end);
            }
            // Punctuation follows the preceding spoken token's final frame;
            // leading punctuation follows the next token's first frame.
            if let Some(prev) = ranges[..i].iter().rev().flatten().next() {
                let end = spans[prev.end - 1].end;
                (end - 1, end)
            } else {
                let next = ranges[i + 1..]
                    .iter()
                    .flatten()
                    .next()
                    .expect("at least one spoken token");
                let start = spans[next.start].start;
                (start, start + 1)
            }
        })
        .collect()
}

#[cfg(test)]
fn recorded_window(
    pcm_len: usize,
    map: &TimeMap,
    line_t: i64,
    next_t: i64,
) -> Option<std::ops::Range<usize>> {
    recorded_window_margin(pcm_len, map, line_t, next_t, 500.0)
}

fn recorded_window_margin(
    pcm_len: usize,
    map: &TimeMap,
    line_t: i64,
    next_t: i64,
    margin_ms: f64,
) -> Option<std::ops::Range<usize>> {
    let audio_end = map.intercept_ms + pcm_len as f64 * map.slope_ms;
    if line_t as f64 >= audio_end {
        return None;
    }
    let sample = |ms: f64| {
        ((ms - map.intercept_ms) / map.slope_ms)
            .round()
            .clamp(0.0, pcm_len as f64) as usize
    };
    let begin = sample(line_t as f64 - margin_ms);
    let end = sample((next_t as f64 + margin_ms).min(line_t as f64 + 20_000.0));
    // Wider offline probes must respect the same bounded model input.
    let end = if margin_ms > 500.0 {
        end.min(begin.saturating_add(16_000 * 21))
    } else {
        end
    };
    (end.saturating_sub(begin) >= 400).then_some(begin..end)
}

pub struct AcousticAligner {
    session: Session,
    romanizer: uroman::Uroman,
}

impl AcousticAligner {
    /// Runtime must be an explicitly provisioned ONNX Runtime 1.22 DLL.
    /// Called only from the background alignment worker, never audio callbacks.
    pub fn load(model: &Path, runtime: &Path) -> Result<Self> {
        Self::load_tuned(model, runtime, 2, true)
    }

    /// Benchmark seam; production settings are chosen by load().
    pub fn load_tuned(
        model: &Path,
        runtime: &Path,
        threads: usize,
        spinning: bool,
    ) -> Result<Self> {
        if !(1..=8).contains(&threads) {
            return Err("thread count must be 1..=8".into());
        }
        if !model.is_file() || !runtime.is_file() {
            return Err("acoustic model/runtime missing".into());
        }
        verify_asset(model, MODEL_SHA256)?;
        verify_asset(runtime, RUNTIME_SHA256)?;
        guard_runtime(|| Self::load_verified(model, runtime, threads, spinning))
    }

    fn load_verified(model: &Path, runtime: &Path, threads: usize, spinning: bool) -> Result<Self> {
        ort::init_from(runtime.to_string_lossy())
            .commit()
            .map_err(|e| e.to_string())?;
        let session = Session::builder()
            .map_err(|e| e.to_string())?
            .with_intra_threads(threads)
            .map_err(|e| e.to_string())?
            .with_inter_threads(1)
            .map_err(|e| e.to_string())?
            .with_intra_op_spinning(spinning)
            .map_err(|e| e.to_string())?
            .with_inter_op_spinning(spinning)
            .map_err(|e| e.to_string())?
            .commit_from_file(model)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            session,
            romanizer: uroman::Uroman::new(),
        })
    }

    pub fn align(&mut self, pcm: &[i16], lines: &[TimedLine], map: &TimeMap) -> Result<Vec<Word>> {
        self.align_probe(pcm, lines, map, 500.0, None)
    }

    /// Offline evidence seam. Production always uses the 500ms window above.
    /// Token confidence is acoustic path support, not a calibrated timing probability.
    pub fn align_probe(
        &mut self,
        pcm: &[i16],
        lines: &[TimedLine],
        map: &TimeMap,
        margin_ms: f64,
        mut diagnostics: Option<&mut Vec<serde_json::Value>>,
    ) -> Result<Vec<Word>> {
        if !margin_ms.is_finite() || !(0.0..=1500.0).contains(&margin_ms) {
            return Err("probe margin must be 0..=1500ms".into());
        }
        if !map.slope_ms.is_finite() || map.slope_ms <= 0.0 || !map.intercept_ms.is_finite() {
            return Err("invalid capture time map".into());
        }
        let to_time = |sample: f64| (map.intercept_ms + sample * map.slope_ms).round() as i64;
        let mut words = Vec::new();
        for (li, line) in lines.iter().enumerate() {
            let tokens = tokenize(&line.text);
            if tokens.is_empty() {
                continue;
            }
            let next = lines.get(li + 1).map_or(to_time(pcm.len() as f64), |l| l.t);
            let Some(window) = recorded_window_margin(pcm.len(), map, line.t, next, margin_ms)
            else {
                // A partial listen can end before later lyric rows. Keep the
                // captured prefix for diagnostics; cache_complete in the
                // worker still rejects an incomplete song for persistence.
                continue;
            };
            let (begin, end) = (window.start, window.end);
            if end - begin > 16_000 * 21 {
                return Err("acoustic input exceeds 21 second memory bound".into());
            }
            let plan = plan_tokens(tokens, &self.romanizer)?;
            if plan.ranges.iter().all(Option::is_none) {
                continue;
            }
            let input: Vec<f32> = pcm[begin..end]
                .iter()
                .map(|&s| s as f32 / 32768.0)
                .collect();
            let tensor =
                Tensor::from_array(([1usize, input.len()], input)).map_err(|e| e.to_string())?;
            let output = self
                .session
                .run(ort::inputs!["waveform" => tensor])
                .map_err(|e| e.to_string())?;
            let (shape, logp) = output
                .get("log_probs")
                .ok_or("missing acoustic output")?
                .try_extract_tensor::<f32>()
                .map_err(|e| e.to_string())?;
            if shape.len() != 3 || shape[0] != 1 || shape[2] != CLASSES as i64 {
                return Err("unexpected acoustic output".into());
            }
            let spans = ctc_spans(logp, CLASSES, &plan.targets)?;
            let ratio = (end - begin) as f64 / shape[1] as f64;
            let positions = word_frames(&plan.ranges, &spans);
            if let Some(rows) = diagnostics.as_deref_mut() {
                rows.push(serde_json::json!({
                    "line_index": li, "line_t": line.t, "next_t": next,
                    "window_start": to_time(begin as f64), "window_end": to_time(end as f64),
                    "frame_ms": ratio * map.slope_ms,
                    "tokens": plan.tokens, "targets": plan.targets.iter().map(|&t| LABELS[t] as char).collect::<String>(),
                    "spans": spans.iter().map(|s| serde_json::json!({
                        "start": to_time(begin as f64 + s.start as f64 * ratio),
                        "end": to_time(begin as f64 + s.end as f64 * ratio),
                        "support": s.confidence,
                    })).collect::<Vec<_>>()
                }));
            }
            for ((text, range), (start, end)) in
                plan.tokens.into_iter().zip(&plan.ranges).zip(positions)
            {
                let points = range
                    .as_ref()
                    .map(|range| {
                        spelling_points(
                            &text,
                            &plan.targets[range.clone()],
                            &spans[range.clone()],
                            |frame| to_time(begin as f64 + frame as f64 * ratio),
                        )
                    })
                    .unwrap_or_default();
                words.push(Word {
                    text,
                    t: to_time(begin as f64 + start as f64 * ratio),
                    end: Some(to_time(begin as f64 + end as f64 * ratio)),
                    line_t: Some(line.t),
                    points,
                });
            }
        }
        Ok(words)
    }
}

/// Keep internal CTC spans only when labels map directly to displayed ASCII
/// spelling. Romanized scripts retain their existing syllable-token behavior.
fn spelling_points(
    text: &str,
    targets: &[usize],
    spans: &[TokenSpan],
    time: impl Fn(usize) -> i64,
) -> Vec<WordPoint> {
    if !text.is_ascii() {
        return Vec::new();
    }
    let letters: Vec<_> = text
        .bytes()
        .enumerate()
        .filter(|(_, b)| b.is_ascii_alphabetic() || *b == b'\'')
        .collect();
    if letters.len() < 4
        || letters.len() != targets.len()
        || spans.len() != targets.len()
        || letters
            .iter()
            .zip(targets)
            .any(|((_, b), &target)| LABELS.get(target) != Some(&b.to_ascii_lowercase()))
    {
        return Vec::new();
    }
    let width = text.trim_end().len() as f64;
    let mut points = Vec::with_capacity(spans.len() * 2);
    for (i, ((offset, _), span)) in letters.iter().zip(spans).enumerate() {
        let fraction = if i == 0 { 0.0 } else { *offset as f64 / width };
        points.push(WordPoint {
            t: time(span.start),
            fraction,
        });
        let fraction = if i + 1 == letters.len() {
            1.0
        } else {
            letters[i + 1].0 as f64 / width
        };
        points.push(WordPoint {
            t: time(span.end),
            fraction,
        });
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spelling_detail_preserves_internal_gap_and_source_punctuation() {
        let romanizer = uroman::Uroman::new();
        let plan = plan_tokens(vec!["(hallway!) ".into()], &romanizer).unwrap();
        let range = plan.ranges[0].clone().unwrap();
        let spans: Vec<_> = (0..7)
            .map(|i| TokenSpan {
                start: i * 10 + if i >= 4 { 30 } else { 0 },
                end: i * 10 + 5 + if i >= 4 { 30 } else { 0 },
                confidence: 1.0,
            })
            .collect();
        let points = spelling_points(&plan.tokens[0], &plan.targets[range], &spans, |frame| {
            1000 + frame as i64 * 10
        });
        assert_eq!(points.first().unwrap().fraction, 0.0);
        assert_eq!(points.last().unwrap().fraction, 1.0);
        assert_eq!(points[7].fraction, points[8].fraction);
        assert_eq!(points[8].t - points[7].t, 350);
        assert!(points
            .windows(2)
            .all(|p| p[0].t <= p[1].t && p[0].fraction <= p[1].fraction));
        assert!(spelling_points("한글", &[1; 7], &spans, |f| f as i64).is_empty());
        assert!(spelling_points("hallway", &[1; 7], &spans, |f| f as i64).is_empty());
    }

    #[test]
    fn repeated_letters_require_blank_and_have_distinct_spans() {
        let scores = [0.01f32, 0.99, 0.99, 0.01, 0.01, 0.99].map(f32::ln);
        let spans = ctc_spans(&scores, 2, &[1, 1]).unwrap();
        assert_eq!((spans[0].start, spans[0].end), (0, 1));
        assert_eq!((spans[1].start, spans[1].end), (2, 3));
        assert!(ctc_spans(&scores[..4], 2, &[1, 1]).is_err());
    }
    #[test]
    fn matches_independent_torchaudio_oracle() {
        #[derive(serde::Deserialize)]
        struct Case {
            classes: usize,
            scores: Vec<f32>,
            targets: Vec<usize>,
            spans: Vec<TokenSpan>,
        }
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../tests/fixtures/ctc_oracle.json")).unwrap();
        for (i, c) in cases.iter().enumerate() {
            let spans = ctc_spans(&c.scores, c.classes, &c.targets).unwrap();
            for (actual, expected) in spans.iter().zip(&c.spans) {
                assert_eq!(
                    (actual.start, actual.end),
                    (expected.start, expected.end),
                    "case {i}"
                );
                assert!(
                    (actual.confidence - expected.confidence).abs() < 1e-5,
                    "case {i}"
                );
            }
            assert_eq!(spans.len(), c.spans.len());
        }
    }
    #[test]
    fn punctuation_keeps_display_slots_without_acoustic_targets() {
        let roman = uroman::Uroman::new();
        let plan = plan_tokens(tokenize("안녕!"), &roman).unwrap();
        assert_eq!(plan.tokens.concat(), "안녕!");
        assert_eq!(plan.ranges.len(), 3);
        assert!(plan.ranges[2].is_none());
        let spans: Vec<_> = plan
            .targets
            .iter()
            .enumerate()
            .map(|(i, _)| TokenSpan {
                start: i * 2,
                end: i * 2 + 2,
                confidence: 1.0,
            })
            .collect();
        let frames = word_frames(&plan.ranges, &spans);
        assert_eq!(frames[2], (frames[1].1 - 1, frames[1].1));
        let quotes = plan_tokens(tokenize("'안녕'"), &roman).unwrap();
        assert!(quotes.ranges.first().unwrap().is_none());
        assert!(quotes.ranges.last().unwrap().is_none());
        let contraction = plan_tokens(vec!["don't".into()], &roman).unwrap();
        assert!(contraction.targets.contains(&25));
        let lead = plan_tokens(vec!["!".into(), "hello".into()], &roman).unwrap();
        let frames = word_frames(&lead.ranges, &spans);
        assert_eq!(frames[0].0, frames[1].0);
        assert!(plan_tokens(vec!["!".into()], &roman)
            .unwrap()
            .ranges
            .iter()
            .all(Option::is_none));
    }
    #[test]
    fn dll_loader_panics_take_the_retryable_error_path() {
        let result: Result<()> = guard_runtime(|| panic!("missing runtime dependency"));
        assert!(result.unwrap_err().contains("initialization failed"));
    }
    #[test]
    fn partial_listen_keeps_recorded_rows_and_skips_unheard_tail() {
        // Live Revenge capture ended after ~123s of a 187.5s song. The
        // old loop errored on row 33, losing the already-aligned prefix.
        let map = TimeMap::from_origin(111, 16_000);
        let count = 123 * 16_000;
        assert!(recorded_window(count, &map, 115_000, 120_000).is_some());
        assert!(recorded_window(count, &map, 124_000, 130_000).is_none());
        assert!(recorded_window(count, &map, 160_000, 165_000).is_none());
        let last = recorded_window(count, &map, 122_000, 125_000).unwrap();
        assert_eq!(last.end, count);
    }
    #[test]
    fn wide_probe_long_gap_respects_input_bound() {
        let map = TimeMap::from_origin(0, 16_000);
        let window = recorded_window_margin(640_000, &map, 10_000, 30_000, 1500.0).unwrap();
        assert_eq!(window.start, 136_000);
        assert_eq!(window.len(), 336_000);
        let baseline = recorded_window_margin(640_000, &map, 10_000, 30_000, 500.0).unwrap();
        assert_eq!(baseline, 152_000..480_000);
    }

    #[test]
    fn malformed_emissions_fail_closed() {
        assert!(ctc_spans(&[], 29, &[1]).is_err());
        assert!(ctc_spans(&[0.0, f32::NAN], 2, &[1]).is_err());
        assert!(ctc_spans(&[0.0, 0.0], 2, &[0]).is_err());
        assert!(ctc_spans(&[0.0, 0.0], 2, &[2]).is_err());
    }
}

#[cfg(test)]
mod digit_tests {
    use super::*;
    #[test]
    fn four_sho_spoken_digits_keep_original_display_slots() {
        let roman = uroman::Uroman::new();
        let tokens = tokenize("All we know 4 fingers up");
        let original = tokens.clone();
        let plan = plan_tokens(tokens, &roman).unwrap();
        assert_eq!(plan.tokens, original);
        let range = plan.ranges[3].clone().unwrap();
        let spoken: String = plan.targets[range]
            .iter()
            .map(|&i| LABELS[i] as char)
            .collect();
        assert_eq!(spoken, "four");
        let slang = plan_tokens(vec!["4sho".into()], &roman).unwrap();
        let spoken: String = slang.targets[slang.ranges[0].clone().unwrap()]
            .iter()
            .map(|&i| LABELS[i] as char)
            .collect();
        assert_eq!(spoken, "foursho");
        assert_eq!(slang.tokens[0], "4sho");
        assert_eq!(expand_digit("12"), "12");
        assert_eq!(expand_digit("hello"), "hello");
        assert!(plan_tokens(tokenize("4월"), &roman).is_err());
        assert!(plan_tokens(vec!["4".into()], &roman).is_err());
    }
}
