//! Local MMS_FA acoustic alignment. No labels, network, or UI clock access.
//! The caller supplies a 16 kHz recording and its capture-to-song time map.
use crate::align::{tokenize, TimeMap, TimedLine, Word, WordPoint};
use ort::{session::Session, value::Tensor};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::OnceLock,
};

// Explicit model vocabulary order, independent of dictionary iteration order.
const LABELS: &[u8] = b"-aienoutsrmkldghybpwcvjzf'qx*";
const CLASSES: usize = 29;
const STAR: usize = 28;

#[path = "entry_timing.rs"]
mod entry_timing;

/// Recipe pins the exported weights, normalization, window and decoder.
/// 2: Knobs::MIX's tuned decoder (2026-10-31). Bumping it re-learns every
/// saved sync on its next play.
pub const RECIPE: &str = "mms-int8/2";
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
            "enabled (mms-int8/2)"
        } else {
            "not installed"
        }
    );
}

pub fn assets() -> Option<&'static Path> {
    ASSETS.get().and_then(|v| v.as_deref())
}

pub(crate) fn verify_asset(path: &Path, expected: &str) -> Result<()> {
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
    ctc_spans_with_prior(logp, classes, targets, None)
}

fn ctc_spans_with_prior(
    logp: &[f32],
    classes: usize,
    targets: &[usize],
    prior: Option<EntryPrior>,
) -> Result<Vec<TokenSpan>> {
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
        let entry_cost = prior.map_or(0.0, |p| {
            ((t as f64 - p.deadline).max(0.0) as f32) * p.penalty_per_frame
        });
        for s in 0..states {
            let label = if s % 2 == 0 { 0 } else { targets[s / 2] };
            let mut best = prev[s];
            let mut step = 0;
            // Charge once when entering the target, never while dwelling on
            // it. A sustained syllable must not accumulate a lateness cost.
            let cost = if prior.is_some_and(|p| s == p.target * 2 + 1) {
                entry_cost
            } else {
                0.0
            };
            if s > 0 && prev[s - 1] - cost > best {
                best = prev[s - 1] - cost;
                step = 1;
            }
            if s > 1
                && s % 2 == 1
                && targets[s / 2] != targets[s / 2 - 1]
                && prev[s - 2] - cost > best
            {
                best = prev[s - 2] - cost;
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

#[derive(Clone, Copy)]
struct EntryPrior {
    target: usize,
    deadline: f64,
    penalty_per_frame: f32,
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

#[derive(Clone, Copy, PartialEq)]
enum EntryMode {
    None,
    Refine,
    Guide,
}

/// `next_t` is the following stamp, or for the last line the end of what
/// was recorded; `None` keeps the line out of the retry.
fn guide_eligible(
    line: &TimedLine,
    next_t: Option<i64>,
    tokens: &[String],
    first: i64,
    recorded_start: i64,
    late_ms: i64,
) -> bool {
    let Some(next_t) = next_t else {
        return false;
    };
    let span = next_t.saturating_sub(line.t);
    let initial = tokens.first().map(|t| {
        t.trim()
            .trim_matches(|c: char| !c.is_alphabetic())
            .to_ascii_lowercase()
    });
    span > 0
        && span <= 20_000
        && recorded_start <= line.t
        && !line.text.contains(['(', ')', '（', '）'])
        && !entry_timing::repeated_la(tokens)
        && !matches!(initial.as_deref(), Some("oh" | "ooh" | "ah"))
        && first.saturating_sub(line.t) > late_ms
}

/// A retried entrance the line's following targets join straight away is a
/// real earlier copy, however faint its first phoneme: support falls when
/// the free path grabbed a louder repeat later in the window. The noise
/// shape the support guard rejects is a lone early blip with the rest of
/// the line still waiting at the original position.
fn connected_entry_retry(
    original: &[TokenSpan],
    candidate: &[TokenSpan],
    first: usize,
    frame_ms: f64,
) -> bool {
    let Some(following) = candidate.get(first + 1) else {
        return false;
    };
    candidate[first].start < original[first].start
        && candidate[first].confidence > 0.0
        && following.start < original[first].start
        && (following.start.saturating_sub(candidate[first].end) as f64) * frame_ms <= 400.0
}

fn supported_entry_retry(original: &[TokenSpan], candidate: &[TokenSpan], first: usize) -> bool {
    // The source is a prior, not permission to consume an unsupported noise
    // frame and skip a strong held phoneme later. Posterior support is not a
    // calibrated timing probability; compare like-for-like first targets.
    // Also retain posterior mass across frames: a strong 20ms blip must not
    // replace a supported 360ms hold just because its mean score is similar.
    candidate[first].start < original[first].start
        && candidate[first].confidence > 0.0
        && candidate[first].confidence >= original[first].confidence * 0.5
        && candidate[first].confidence * (candidate[first].end - candidate[first].start) as f32
            >= original[first].confidence
                * (original[first].end - original[first].start) as f32
                * 0.5
}

/// Model output per input window, keyed by a hash of the window's samples.
/// An offline seam (examples/karaoke_bench.rs): decoder candidates rescore
/// stored emissions instead of rerunning the model. The app never sets one.
pub type EmissionCache = HashMap<u64, Vec<f32>>;

/// Decoder and placement switches. `Knobs::MIX` (align) and `Knobs::TRIAL`
/// (align_guided_entries) are what the app runs; any other value is an
/// offline candidate for examples/karaoke_bench.rs to score.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    /// Retry a late first word under the source-entry prior.
    pub guide: bool,
    /// How far past its stamp a first word must land to be retried.
    pub guide_late_ms: i64,
    /// Also retry the last line, which has no following stamp.
    pub guide_last: bool,
    /// Cap a line's window at its stamp + base + this per sung syllable
    /// when the next stamp is farther (last line, instrumental break).
    /// Per syllable, not per word: a held "intentandoooo" takes four
    /// seconds of a two-word line. 0 = off.
    pub cap_per_syllable_ms: i64,
    pub cap_base_ms: i64,
    /// A vocable-only line ("oh ah ah") whose path strays from an even
    /// spread by more than this falls back to the spread. 0 = off.
    pub vocable_tolerance_ms: i64,
    /// Also trust a retried entrance the rest of the line follows straight
    /// away, however faint its first phoneme (`connected_entry_retry`).
    pub connected_retry: bool,
}

impl Knobs {
    /// Scored on JamendoLyrics (examples/karaoke_bench.rs, 71 songs, stamps
    /// on time and 200ms early): each part won on its own, on the mix and
    /// on separated vocals, and together no song got clearly worse.
    /// docs/specs/2026-10-31-karaoke-sync-fixes.md. Changing these changes
    /// RECIPE: every saved sync was learned under them.
    pub const MIX: Knobs = Knobs {
        guide: true,
        guide_late_ms: 700,
        guide_last: true,
        cap_per_syllable_ms: 400,
        cap_base_ms: 3000,
        vocable_tolerance_ms: 1500,
        connected_retry: true,
    };
    pub const TRIAL: Knobs = Knobs::MIX;

    /// Where a line's window stops looking: the next stamp, or sooner when
    /// that stamp is farther than the line could plausibly take to sing.
    fn window_next(&self, line_t: i64, next_t: i64, syllables: usize) -> i64 {
        if self.cap_per_syllable_ms <= 0 {
            return next_t;
        }
        next_t.min(line_t + self.cap_base_ms + self.cap_per_syllable_ms * syllables as i64)
    }
}

/// Vowel groups in Latin spelling, one per CJK or Hangul character, and at
/// least one per token: a rough sung-syllable count for the window cap.
fn sung_syllables(tokens: &[String]) -> usize {
    const VOWELS: &str = "aeiouyàáâãäåæèéêëìíîïòóôõöøœùúûüýÿ";
    tokens
        .iter()
        .map(|token| {
            let mut count = 0;
            let mut in_vowel = false;
            for c in token.chars().flat_map(char::to_lowercase) {
                let vowel = VOWELS.contains(c);
                if (vowel && !in_vowel) || crate::align::is_syllable_char(c) {
                    count += 1;
                }
                in_vowel = vowel;
            }
            count.max(1)
        })
        .sum()
}

pub struct AcousticAligner {
    session: Session,
    romanizer: uroman::Uroman,
    emissions: Option<EmissionCache>,
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
            emissions: None,
        })
    }

    /// Offline seam: reuse and record model emissions per window.
    pub fn set_emission_cache(&mut self, cache: Option<EmissionCache>) {
        self.emissions = cache;
    }

    pub fn take_emission_cache(&mut self) -> Option<EmissionCache> {
        self.emissions.take()
    }

    pub fn align(&mut self, pcm: &[i16], lines: &[TimedLine], map: &TimeMap) -> Result<Vec<Word>> {
        self.align_probe(pcm, lines, map, 500.0, None)
    }

    /// Offline evaluation seam: either app pipeline with candidate knobs.
    /// `guided` selects align_guided_entries' path (separated vocals).
    pub fn align_with_knobs(
        &mut self,
        pcm: &[i16],
        lines: &[TimedLine],
        map: &TimeMap,
        guided: bool,
        knobs: Knobs,
        diagnostics: Option<&mut Vec<serde_json::Value>>,
    ) -> Result<Vec<Word>> {
        if !guided {
            return self.align_inner(pcm, lines, map, 500.0, diagnostics, EntryMode::None, knobs);
        }
        let mut words =
            self.align_inner(pcm, lines, map, 500.0, diagnostics, EntryMode::Guide, knobs)?;
        entry_timing::refine(pcm, map, &mut words);
        Ok(words)
    }

    /// Offline evidence seam. Production always uses the 500ms window above.
    /// Token confidence is acoustic path support, not a calibrated timing probability.
    pub fn align_probe(
        &mut self,
        pcm: &[i16],
        lines: &[TimedLine],
        map: &TimeMap,
        margin_ms: f64,
        diagnostics: Option<&mut Vec<serde_json::Value>>,
    ) -> Result<Vec<Word>> {
        self.align_inner(
            pcm,
            lines,
            map,
            margin_ms,
            diagnostics,
            EntryMode::None,
            Knobs::MIX,
        )
    }

    /// Offline comparison baseline: the vocal path without the entry retry.
    pub fn align_entries(
        &mut self,
        pcm: &[i16],
        lines: &[TimedLine],
        map: &TimeMap,
    ) -> Result<Vec<Word>> {
        let knobs = Knobs {
            guide: false,
            ..Knobs::TRIAL
        };
        let mut words = self.align_inner(pcm, lines, map, 500.0, None, EntryMode::Refine, knobs)?;
        entry_timing::refine(pcm, map, &mut words);
        Ok(words)
    }

    /// Source-guidance trial: retries the decoder on the same emissions.
    /// align_entries is the same path without the retry, for comparison.
    pub fn align_guided_entries(
        &mut self,
        pcm: &[i16],
        lines: &[TimedLine],
        map: &TimeMap,
        diagnostics: Option<&mut Vec<serde_json::Value>>,
    ) -> Result<Vec<Word>> {
        let mut words = self.align_inner(
            pcm,
            lines,
            map,
            500.0,
            diagnostics,
            EntryMode::Guide,
            Knobs::TRIAL,
        )?;
        entry_timing::refine(pcm, map, &mut words);
        Ok(words)
    }

    // Every public entry point is a thin preset over this one loop.
    #[allow(clippy::too_many_arguments)]
    fn align_inner(
        &mut self,
        pcm: &[i16],
        lines: &[TimedLine],
        map: &TimeMap,
        margin_ms: f64,
        mut diagnostics: Option<&mut Vec<serde_json::Value>>,
        mode: EntryMode,
        knobs: Knobs,
    ) -> Result<Vec<Word>> {
        let entries = mode != EntryMode::None;
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
            // A held "ooh" has no syllable budget (eight seconds apiece on
            // JamendoLyrics' Fantasma), so vocable lines keep the full gap.
            let capped = if tokens.iter().all(|t| is_vocable(t)) {
                next
            } else {
                knobs.window_next(line.t, next, sung_syllables(&tokens))
            };
            let Some(window) = recorded_window_margin(pcm.len(), map, line.t, capped, margin_ms)
            else {
                // A partial listen can end before later lyric rows. Keep the
                // captured prefix for diagnostics; cache_complete in the
                // worker still rejects an incomplete song for persistence.
                continue;
            };
            let la_entry = entries && entry_timing::repeated_la(&tokens);
            let (begin, mut end) = (window.start, window.end);
            if la_entry {
                end = end.min(
                    ((next as f64 - map.intercept_ms) / map.slope_ms)
                        .round()
                        .max(0.0) as usize,
                );
                if end.saturating_sub(begin) < 400 {
                    continue;
                }
            }
            if end - begin > 16_000 * 21 {
                return Err("acoustic input exceeds 21 second memory bound".into());
            }
            let mut plan = plan_tokens(tokens, &self.romanizer)?;
            if la_entry {
                plan.targets.remove(0);
                for range in plan.ranges.iter_mut().flatten() {
                    range.start -= 1;
                    range.end -= 1;
                }
            }
            if plan.ranges.iter().all(Option::is_none) {
                continue;
            }
            let logp = self.emissions_for(&pcm[begin..end])?;
            let logp = logp.as_slice();
            let ratio = (end - begin) as f64 / (logp.len() / CLASSES) as f64;
            let frame_ms = ratio * map.slope_ms;
            let mut spans = ctc_spans(logp, CLASSES, &plan.targets)?;
            let first = plan
                .ranges
                .iter()
                .flatten()
                .next()
                .expect("spoken target")
                .start;
            let original_entry = to_time(begin as f64 + spans[first].start as f64 * ratio);
            let mut guided = false;
            let mut retry = serde_json::Value::Null;
            let guide_next = lines.get(li + 1).map(|l| l.t).or(knobs
                .guide_last
                .then(|| to_time(end as f64).min(line.t + 20_000)));
            if knobs.guide
                && first == 1
                && guide_eligible(
                    line,
                    guide_next,
                    &plan.tokens,
                    original_entry,
                    to_time(begin as f64),
                    knobs.guide_late_ms,
                )
            {
                let prior = EntryPrior {
                    target: first,
                    deadline: (line.t as f64 + 500.0
                        - (map.intercept_ms + begin as f64 * map.slope_ms))
                        / (ratio * map.slope_ms),
                    penalty_per_frame: (8.0 * ratio * map.slope_ms / 1000.0) as f32,
                };
                if let Ok(candidate) =
                    ctc_spans_with_prior(logp, CLASSES, &plan.targets, Some(prior))
                {
                    let accepted = supported_entry_retry(&spans, &candidate, first)
                        || (knobs.connected_retry
                            && connected_entry_retry(&spans, &candidate, first, frame_ms));
                    if diagnostics.is_some() {
                        retry = serde_json::json!({
                            "entry": to_time(begin as f64 + candidate[first].start as f64 * ratio),
                            "support": candidate[first].confidence,
                            "original_support": spans[first].confidence,
                            "accepted": accepted,
                        });
                    }
                    if accepted {
                        spans = candidate;
                        guided = true;
                    }
                }
            }
            let positions = word_frames(&plan.ranges, &spans);
            let line_start = words.len();
            if let Some(rows) = diagnostics.as_deref_mut() {
                rows.push(serde_json::json!({
                    "line_index": li, "line_t": line.t, "next_t": next,
                    "source_guided": guided, "original_entry": original_entry, "retry": retry,
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
            if knobs.vocable_tolerance_ms > 0 {
                if let Some(next_line) = lines.get(li + 1) {
                    vocable_fallback(
                        &mut words[line_start..],
                        line.t,
                        next_line.t,
                        knobs.vocable_tolerance_ms,
                    );
                }
            }
        }
        Ok(words)
    }

    /// Model log-probabilities for one window, through the offline cache
    /// when one is installed.
    fn emissions_for(&mut self, input: &[i16]) -> Result<Vec<f32>> {
        let key = self.emissions.as_ref().map(|_| fingerprint(input));
        if let (Some(cache), Some(key)) = (&self.emissions, key) {
            if let Some(hit) = cache.get(&key) {
                return Ok(hit.clone());
            }
        }
        let samples: Vec<f32> = input.iter().map(|&s| s as f32 / 32768.0).collect();
        let tensor =
            Tensor::from_array(([1usize, samples.len()], samples)).map_err(|e| e.to_string())?;
        let output = self
            .session
            .run(ort::inputs!["waveform" => tensor])
            .map_err(|e| e.to_string())?;
        let (shape, logp) = output
            .get("log_probs")
            .ok_or("missing acoustic output")?
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        if shape.len() != 3 || shape[0] != 1 || shape[2] != CLASSES as i64 || shape[1] < 1 {
            return Err("unexpected acoustic output".into());
        }
        let logp = logp.to_vec();
        drop(output);
        if let (Some(cache), Some(key)) = (self.emissions.as_mut(), key) {
            cache.insert(key, logp.clone());
        }
        Ok(logp)
    }
}

/// FNV-1a over the window's samples: the emission cache key.
fn fingerprint(samples: &[i16]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ samples.len() as u64;
    for b in samples.iter().flat_map(|s| s.to_le_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Sung vocables give CTC no consonants to hold onto, so a free path can
/// compress "ooh ooh ooh ooh" into two seconds of a 30-second phrase or
/// slide a lone "oh" to the end of its gap.
fn is_vocable(token: &str) -> bool {
    let plain = token
        .trim()
        .trim_matches(|c: char| !c.is_alphabetic())
        .to_lowercase();
    !token.contains(['(', ')'])
        && matches!(
            plain.as_str(),
            "oh" | "ooh"
                | "oooh"
                | "ohh"
                | "oo"
                | "ooo"
                | "ah"
                | "ahh"
                | "aah"
                | "ahha"
                | "aw"
                | "uh"
                | "mm"
                | "mmm"
                | "hmm"
        )
}

/// Share of a vocable line's stamp-to-stamp interval the spread fills:
/// singers breathe before the next line (on JamendoLyrics the last vocable
/// onset sits at a median 74% of the interval).
const VOCABLE_SPAN: f64 = 0.85;

/// A vocable-only line whose decoded onsets stray from an even spread over
/// its interval by more than `tolerance_ms` takes the spread instead. A
/// path that agrees with the spread keeps its own, finer timing.
fn vocable_fallback(words: &mut [Word], line_t: i64, next_t: i64, tolerance_ms: i64) {
    if words.is_empty() || next_t <= line_t || !words.iter().all(|w| is_vocable(&w.text)) {
        return;
    }
    let span = (next_t - line_t) as f64 * VOCABLE_SPAN;
    let n = words.len() as f64;
    let spread = |i: usize| line_t + (span * i as f64 / n).round() as i64;
    let strays = words
        .iter()
        .enumerate()
        .any(|(i, w)| (w.t - spread(i)).abs() > tolerance_ms);
    if !strays {
        return;
    }
    for (i, w) in words.iter_mut().enumerate() {
        w.t = spread(i);
        w.end = Some(spread(i + 1));
        w.points.clear();
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
    // Regression: a leading wildcard can absorb a quiet first word and
    // match a louder copy near the end. A source prior guides the transition,
    // while the following word must still land on its acoustic evidence.
    #[test]
    fn source_prior_prefers_supported_entrance_and_preserves_following_hold() {
        let mut scores = vec![vec![-8.0f32; 4]; 120];
        for row in &mut scores {
            row[0] = -0.1;
            row[3] = -0.2;
        }
        scores[12][1] = -0.5;
        scores[94][1] = -0.1;
        for row in &mut scores[98..112] {
            row[2] = -0.01;
        }
        let flat: Vec<_> = scores.into_iter().flatten().collect();
        let targets = [3, 1, 2, 3];
        let old = ctc_spans(&flat, 4, &targets).unwrap();
        assert_eq!(old[1].start, 94);
        let guided = ctc_spans_with_prior(
            &flat,
            4,
            &targets,
            Some(EntryPrior {
                target: 1,
                deadline: 20.0,
                penalty_per_frame: 0.16,
            }),
        )
        .unwrap();
        assert_eq!(guided[1].start, 12);
        assert!(supported_entry_retry(&old, &guided, 1));
        assert_eq!(guided[2], old[2]);
        assert_eq!(ctc_spans_with_prior(&flat, 4, &targets, None).unwrap(), old);
    }

    #[test]
    fn source_prior_is_soft_when_the_early_audio_contradicts_it() {
        let mut scores = vec![vec![-100.0f32; 4]; 120];
        for row in &mut scores {
            row[0] = -0.1;
            row[3] = -0.2;
        }
        scores[94][1] = -0.1;
        for row in &mut scores[98..112] {
            row[2] = -0.01;
        }
        let flat: Vec<_> = scores.into_iter().flatten().collect();
        let old = ctc_spans(&flat, 4, &[3, 1, 2, 3]).unwrap();
        let guided = ctc_spans_with_prior(
            &flat,
            4,
            &[3, 1, 2, 3],
            Some(EntryPrior {
                target: 1,
                deadline: 20.0,
                penalty_per_frame: 0.16,
            }),
        )
        .unwrap();
        assert_eq!(guided, old);
    }

    #[test]
    fn source_guide_excludes_good_entries_and_ambiguous_voices() {
        let line = TimedLine {
            t: 1000,
            text: "one two".into(),
        };
        let next = TimedLine {
            t: 5000,
            text: "next".into(),
        };
        let eligible = |line: &TimedLine, next: Option<&TimedLine>, first, start| {
            guide_eligible(
                line,
                next.map(|n| n.t),
                &tokenize(&line.text),
                first,
                start,
                1500,
            )
        };
        assert!(eligible(&line, Some(&next), 2501, 500));
        assert!(!eligible(&line, Some(&next), 2500, 500));
        assert!(!eligible(&line, Some(&next), 800, 500));
        assert!(!eligible(&line, None, 4900, 500));
        assert!(!eligible(&line, Some(&line), 4900, 500));
        assert!(!eligible(&line, Some(&next), 4900, 1001));
        assert!(!eligible(
            &line,
            Some(&TimedLine {
                t: 21001,
                text: String::new()
            }),
            20500,
            500
        ));
        for text in [
            "one (two)",
            "one （two）",
            "Oh, one",
            "Ooh one",
            "Ah one",
            "La la la",
        ] {
            assert!(
                !eligible(
                    &TimedLine {
                        t: 1000,
                        text: text.into()
                    },
                    Some(&next),
                    4900,
                    500
                ),
                "{text}"
            );
        }
        let short = TimedLine {
            t: 3000,
            text: String::new(),
        };
        assert!(!eligible(&line, Some(&short), 2500, 500));
        assert!(eligible(&line, Some(&short), 2501, 500));
    }

    #[test]
    fn source_retry_rejects_noise_that_discards_a_supported_hold() {
        for early_score in [-6.0, 0.6] {
            let mut rows = vec![[-0.1_f64, -12.0, -12.0, -0.2]; 1000];
            rows[45][1] = early_score;
            for row in &mut rows[825..843] {
                *row = [-4.0, -0.01, -12.0, -4.0];
            }
            for row in &mut rows[850..875] {
                *row = [-4.0, -12.0, -0.01, -4.0];
            }
            let flat: Vec<f32> = rows
                .into_iter()
                .flat_map(|row| {
                    let z = row.iter().map(|v| v.exp()).sum::<f64>().ln();
                    row.map(|v| (v - z) as f32)
                })
                .collect();
            let targets = [3, 1, 2, 3];
            let original = ctc_spans(&flat, 4, &targets).unwrap();
            let candidate = ctc_spans_with_prior(
                &flat,
                4,
                &targets,
                Some(EntryPrior {
                    target: 1,
                    deadline: 50.0,
                    penalty_per_frame: 0.16,
                }),
            )
            .unwrap();
            assert_eq!((original[1].start, original[1].end), (825, 843));
            assert_eq!((candidate[1].start, candidate[1].end), (45, 46));
            assert!(
                !supported_entry_retry(&original, &candidate, 1),
                "early score {early_score}"
            );
        }
    }

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

    // Regression (JamendoLyrics "i've waited i've waited"): the free path
    // skipped a faint first copy of the line for a louder one 1.6s on. The
    // retry finds the real entrance, whose first phoneme has too little
    // support for the support guard; the rest of the line following it
    // straight away is what tells it apart from a lone noise blip.
    #[test]
    fn connected_retry_trusts_a_faint_entrance_the_line_follows() {
        let mut rows = vec![[-0.1_f64, -12.0, -12.0, -0.2]; 400];
        rows[40] = [-1.0, -3.0, -12.0, -1.0];
        for row in &mut rows[42..60] {
            *row = [-0.5, -12.0, -0.5, -0.5];
        }
        // The louder repeat, which the trailing wildcard can also absorb.
        for row in &mut rows[200..215] {
            *row = [-4.0, -0.01, -12.0, -0.3];
        }
        for row in &mut rows[217..225] {
            *row = [-4.0, -12.0, -0.01, -0.3];
        }
        let flat: Vec<f32> = rows
            .into_iter()
            .flat_map(|row| {
                let z = row.iter().map(|v| v.exp()).sum::<f64>().ln();
                row.map(|v| (v - z) as f32)
            })
            .collect();
        let targets = [3, 1, 2, 3];
        let original = ctc_spans(&flat, 4, &targets).unwrap();
        assert_eq!(original[1].start, 200);
        let candidate = ctc_spans_with_prior(
            &flat,
            4,
            &targets,
            Some(EntryPrior {
                target: 1,
                deadline: 45.0,
                penalty_per_frame: 0.16,
            }),
        )
        .unwrap();
        assert_eq!((candidate[1].start, candidate[2].start), (40, 42));
        assert!(!supported_entry_retry(&original, &candidate, 1));
        assert!(connected_entry_retry(&original, &candidate, 1, 20.0));
    }

    #[test]
    fn connected_retry_still_rejects_a_lone_blip() {
        let original = [
            TokenSpan {
                start: 0,
                end: 10,
                confidence: 0.9,
            },
            TokenSpan {
                start: 825,
                end: 843,
                confidence: 0.9,
            },
            TokenSpan {
                start: 850,
                end: 875,
                confidence: 0.9,
            },
        ];
        let blip = [
            TokenSpan {
                start: 0,
                end: 10,
                confidence: 0.9,
            },
            TokenSpan {
                start: 45,
                end: 46,
                confidence: 0.4,
            },
            TokenSpan {
                start: 850,
                end: 875,
                confidence: 0.9,
            },
        ];
        assert!(!connected_entry_retry(&original, &blip, 1, 20.0));
        let mut far = blip.clone();
        far[2] = TokenSpan {
            start: 80,
            end: 90,
            confidence: 0.9,
        };
        // 34 frames of 20ms after the entrance: not the same phrase.
        assert!(!connected_entry_retry(&original, &far, 1, 20.0));
        far[2].start = 50;
        assert!(connected_entry_retry(&original, &far, 1, 20.0));
    }

    fn vocable_line(texts: &[&str], onsets: &[i64]) -> Vec<Word> {
        texts
            .iter()
            .zip(onsets)
            .map(|(text, &t)| Word {
                text: text.to_string(),
                t,
                end: Some(t + 100),
                line_t: Some(10_000),
                points: vec![WordPoint { t, fraction: 0.0 }],
            })
            .collect()
    }

    #[test]
    fn vocable_line_that_collapsed_takes_the_bounded_spread() {
        // "ooh ooh ooh ooh" sung across 26s, decoded into its first two.
        let mut words = vocable_line(
            &["Ooh, ", "ooh ", "ooh ", "ooh"],
            &[10_400, 10_800, 11_200, 11_900],
        );
        vocable_fallback(&mut words, 10_000, 30_000, 1000);
        let onsets: Vec<i64> = words.iter().map(|w| w.t).collect();
        assert_eq!(onsets, [10_000, 14_250, 18_500, 22_750]);
        assert_eq!(words[0].end, Some(14_250));
        assert_eq!(words[3].end, Some(27_000));
        assert!(words.iter().all(|w| w.points.is_empty()));
    }

    #[test]
    fn vocable_fallback_keeps_agreeing_paths_and_real_words() {
        let agreeing = vocable_line(&["oh ", "ah ", "ah"], &[10_100, 10_900, 11_600]);
        let mut words = agreeing.clone();
        vocable_fallback(&mut words, 10_000, 12_800, 1000);
        assert_eq!(words, agreeing);
        for texts in [["oh ", "my ", "love"], ["(oh) ", "(ah) ", "(ah)"]] {
            let line = vocable_line(&texts, &[10_000, 25_000, 26_000]);
            let mut words = line.clone();
            vocable_fallback(&mut words, 10_000, 30_000, 1000);
            assert_eq!(words, line);
        }
    }

    #[test]
    fn sung_syllables_count_vowel_groups_and_characters() {
        let count = |text: &str| sung_syllables(&tokenize(text));
        assert_eq!(count("intentando destacar"), 7);
        assert_eq!(count("will burn"), 2);
        assert_eq!(count("rhythm"), 1);
        assert_eq!(count("mmm"), 1);
        assert_eq!(count("canción él"), 3);
        assert_eq!(count("사랑해"), 3);
    }

    #[test]
    fn window_cap_only_shortens_long_gaps() {
        let off = Knobs {
            cap_per_syllable_ms: 0,
            ..Knobs::MIX
        };
        assert_eq!(off.window_next(10_000, 40_000, 3), 40_000);
        let cap = Knobs {
            cap_per_syllable_ms: 900,
            cap_base_ms: 1500,
            ..Knobs::MIX
        };
        assert_eq!(cap.window_next(10_000, 40_000, 3), 14_200);
        assert_eq!(cap.window_next(10_000, 12_000, 3), 12_000);
    }

    #[test]
    fn guide_threshold_and_last_line_are_explicit() {
        let line = TimedLine {
            t: 1000,
            text: "will burn".into(),
        };
        let tokens = tokenize(&line.text);
        assert!(!guide_eligible(&line, Some(5000), &tokens, 2200, 500, 1500));
        assert!(guide_eligible(&line, Some(5000), &tokens, 2200, 500, 1000));
        // The last line's bound is what was recorded after its stamp.
        assert!(guide_eligible(
            &line,
            Some(12_000),
            &tokens,
            4000,
            500,
            1500
        ));
        assert!(!guide_eligible(&line, None, &tokens, 4000, 500, 1500));
    }

    #[test]
    fn both_paths_run_the_tuned_decoder() {
        assert_eq!(Knobs::MIX, Knobs::TRIAL);
        let k = Knobs::MIX;
        assert!(k.guide && k.guide_last && k.connected_retry);
        assert_eq!(k.window_next(0, 99_000, 9), 6600);
        assert_eq!(RECIPE, "mms-int8/2");
    }

    #[test]
    fn emission_cache_keys_distinguish_windows() {
        let a = [1i16, 2, 3];
        assert_eq!(fingerprint(&a), fingerprint(&[1, 2, 3]));
        assert_ne!(fingerprint(&a), fingerprint(&[1, 2, 4]));
        assert_ne!(fingerprint(&a), fingerprint(&a[..2]));
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
