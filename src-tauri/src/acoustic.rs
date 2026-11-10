//! Local MMS_FA acoustic alignment. No labels, network, or UI clock access.
//! The caller supplies a 16 kHz recording and its capture-to-song time map.
use crate::align::{tokenize, TimeMap, TimedLine, Word, WordPoint};
use crate::takes::{Ear, Take};
use ort::{session::Session, value::Tensor};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Read,
    ops::Range,
    path::{Path, PathBuf},
    sync::OnceLock,
};

// Explicit model vocabulary order, independent of dictionary iteration order.
const LABELS: &[u8] = b"-aienoutsrmkldghybpwcvjzf'qx*";
const CLASSES: usize = 29;
const STAR: usize = 28;

#[path = "entry_timing.rs"]
mod entry_timing;
#[path = "japanese.rs"]
mod japanese;
#[path = "sync_review.rs"]
mod review;
#[path = "spoken.rs"]
mod spoken;
use spoken::Numerals;

/// Recipe pins the exported weights, normalization, window and decoder.
/// 2: Knobs::MIX's tuned decoder (2026-10-31). 3: the review and its
/// second ear (2026-11-08). 4: Japanese lines spelled as sung (`japanese`:
/// kanji readings from naist-jdic when it is installed, っ, small kana and
/// particles), 2026-11-08. Bumping it re-decodes every saved sync that
/// kept its takes (karaoke/redecode.rs) in the background, when EMISSIONS
/// is unchanged and the new decoder looks at the same windows; the rest
/// relearn on the song's next full listen. Add the old value to
/// EARLIER_RECIPES.
pub const RECIPE: &str = "mms-int8/4";
/// Recipes RECIPE replaced. Words saved under them keep showing, and are
/// never deleted on mismatch, until the next full listen relearns the song.
pub const EARLIER_RECIPES: [&str; 3] = ["mms-int8/1", "mms-int8/2", "mms-int8/3"];
pub const MODEL_SHA256: &str = "50128ba8db1150101b9e7d3610cdfda5c8fd2637b918d151ff6aa6fde2b9c2de";
/// What a saved take's emissions (takes.rs) came from: this model and
/// separator on 16 kHz input scaled by 1/32768. RECIPE pins the decoder;
/// this pins only the model's input and weights, so a decoder change can
/// re-decode saved takes and a change here cannot. Bump it with either
/// model, the input scaling, or the separator's settings.
pub const EMISSIONS: &str = "mms-int8+kim2/1";
pub const RUNTIME_SHA256: &str = "c7151fd9844ad7c7d18525f1177e9ef62d91e4a6ac3583d0be700554a2b2b1d6";
static ASSETS: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Install writes the enable marker last; discovery performs no model load.
/// Session configuration is immutable until restart, including cache identity.
pub fn configure(dir: PathBuf) {
    let enabled = dir.join("enabled").is_file();
    let _ = ASSETS.set(enabled.then_some(dir));
    if enabled {
        log::info!("karaoke: acoustic model enabled ({RECIPE})");
    } else {
        log::info!("karaoke: acoustic model not installed");
    }
}

pub fn assets() -> Option<&'static Path> {
    ASSETS.get().and_then(|v| v.as_deref())
}

/// The review's second ear runs when the vocal separator's model sits
/// beside the aligner's (`acoustic_worker`). The recorder keeps the
/// capture's stereo for it only then.
pub fn second_ear_installed() -> bool {
    assets().is_some_and(|dir| dir.join(crate::separation::MODEL_FILE).is_file())
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

/// The error a token gets when neither uroman nor `spoken::fallback` can say
/// it. It costs that one line its words, never the song.
const UNREPRESENTABLE: &str = "unrepresentable spoken token";

/// `spelled` holds letters a token should be said as (`japanese::spell`),
/// or is empty; a token it leaves None is spelled here.
fn plan_tokens(
    tokens: Vec<String>,
    romanizer: &uroman::Uroman,
    numerals: Numerals,
    spelled: &[Option<String>],
) -> Result<TokenPlan> {
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
    let spell = |text: &str| -> String {
        romanizer
            .romanize_string::<uroman::rom_format::Str>(text, None)
            .to_string()
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_lowercase() || *c == '\'')
            .collect()
    };
    let mut prev_latin: Option<String> = None;
    for (i, token) in tokens.iter().enumerate() {
        // Preserve display text while giving isolated digits (and 4sho-style
        // spellings) an English spoken target. Never drop a spoken digit.
        let spoken = if english_context || token.trim().eq_ignore_ascii_case("4sho") {
            expand_digit(token)
        } else {
            token.clone()
        };
        let mut normalized = match spelled.get(i) {
            Some(Some(said)) => said
                .chars()
                .filter(|c| c.is_ascii_lowercase() || *c == '\'')
                .collect(),
            _ => spell(&spoken),
        };
        if normalized.is_empty() && token.chars().any(char::is_alphanumeric) {
            // ー, 々, 一 and 11 spell as nothing (or digits) in uroman.
            let prev = i.checked_sub(1).map(|p| tokens[p].as_str());
            let said = spoken::fallback(
                token,
                prev,
                prev_latin.as_deref(),
                tokens.get(i + 1).map(String::as_str),
                numerals,
            );
            normalized = said.map(|s| spell(&s)).unwrap_or_default();
        }
        if normalized.is_empty() || !token.chars().any(char::is_alphanumeric) {
            // Nonverbal punctuation keeps its display slot but has no CTC
            // target. Unsupported letters must not silently become punctuation.
            if token.chars().any(char::is_alphanumeric) {
                return Err(format!("{UNREPRESENTABLE} {:?}", token.trim()));
            }
            ranges.push(None);
            continue;
        }
        prev_latin = Some(normalized.clone());
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
    // The last target is the trailing wildcard, which follows any entrance
    // by construction: a one-letter line has nothing to connect to.
    let Some(following) = candidate
        .get(first + 1)
        .filter(|_| first + 2 < candidate.len())
    else {
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
    /// Second look (`sync_review.rs`): a line any of whose words' onset
    /// holds less than this share of its posterior (within
    /// REVIEW_RADIUS_MS of the decoded frame) is doubted, and goes to the
    /// second ear when there is one. 0 = off.
    pub review_below: f32,
    /// Also decode a doubted line on both ears' emissions mixed frame by
    /// frame (`mix_ears`), a third candidate.
    pub review_blend: bool,
    /// Separator passes (`SecondEar::passes`) the second ear may spend on
    /// a song, least sure lines first. 0 = no limit.
    pub review_passes: usize,
    /// A song whose lines start this long or more before their stamps
    /// (`late_stamps`) is looked for again with every window moved to
    /// where the singer is. 0 = off.
    pub recentre_ms: i64,
}

impl Knobs {
    /// Scored on JamendoLyrics (examples/karaoke_bench.rs, 71 songs, stamps
    /// on time and 200ms early): each part won on its own, on the mix and
    /// on separated vocals, and together no song got clearly worse.
    /// docs/specs/2026-10-31-karaoke-sync-fixes.md. Changing these changes
    /// RECIPE: every saved sync was learned under them. `review_passes`
    /// aside (see REVIEW_PASSES).
    pub const MIX: Knobs = Knobs {
        guide: true,
        guide_late_ms: 700,
        guide_last: true,
        cap_per_syllable_ms: 400,
        cap_base_ms: 3000,
        vocable_tolerance_ms: 1500,
        connected_retry: true,
        review_below: REVIEW_BELOW,
        review_blend: true,
        review_passes: REVIEW_PASSES,
        recentre_ms: RECENTRE_MS,
    };
    pub const TRIAL: Knobs = Knobs::MIX;
    /// The decoder before 2026-10-31 (recipe mms-int8/1), for offline
    /// comparison only.
    pub const BASELINE: Knobs = Knobs {
        guide: false,
        guide_late_ms: 1500,
        guide_last: false,
        cap_per_syllable_ms: 0,
        cap_base_ms: 0,
        vocable_tolerance_ms: 0,
        connected_retry: false,
        review_below: 0.0,
        review_blend: false,
        review_passes: 0,
        recentre_ms: 0,
    };

    /// Where a line's window stops looking: the next stamp, or sooner when
    /// that stamp is farther than the line could plausibly take to sing.
    fn window_next(&self, line_t: i64, next_t: i64, syllables: usize) -> i64 {
        if self.cap_per_syllable_ms <= 0 {
            return next_t;
        }
        next_t.min(line_t + self.cap_base_ms + self.cap_per_syllable_ms * syllables as i64)
    }
}

fn japanese_letters(
    romanizer: &uroman::Uroman,
    reader: Option<&japanese::Reader>,
    tokens: &[String],
) -> Vec<Option<String>> {
    let readings = match reader {
        Some(reader) => reader.read(tokens),
        None => vec![None; tokens.len()],
    };
    japanese::spell(tokens, &readings, |token| {
        romanizer
            .romanize_string::<uroman::rom_format::Str>(token, None)
            .to_string()
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_lowercase())
            .collect()
    })
}

/// How near its Viterbi frame a word's onset posterior must sit to count
/// as support for it.
const REVIEW_RADIUS_MS: f64 = 150.0;
/// A line with any word under this support goes to the second ear.
pub(crate) const REVIEW_BELOW: f32 = 0.5;
/// `Knobs::recentre_ms` as shipped. Half the window's 500 ms reach: a
/// song this late already has lines at the edge.
pub(crate) const RECENTRE_MS: i64 = 250;
/// No limit: every doubted line gets its second listen. Until 2026-11-10
/// this was 14 passes (about 90s on Thien's PC at 6.4s a pass), to keep a
/// song's job inside the next song, which ALIGNING then skipped. That
/// fixed 89% of what no limit did on JamendoLyrics (310 of 347 words),
/// but K-pop doubts 44% of lines, not 18%, and every such song hit the
/// cap. Songs that finish during a job now wait their turn (karaoke's
/// queue), so a long job costs time instead. Neighbouring stretches merge
/// whenever that is no dearer, so a song costs at most about one whole-song
/// separation (about 43 passes for four minutes); `acoustic_worker`'s deadline
/// still stops a busy PC. Not part of RECIPE: saves the old cap starved
/// relearn through karaoke's detail_v instead.
const REVIEW_PASSES: usize = 0;

/// Vowel groups in each token's romanization (the same spelling the decoder
/// targets, `spelled` where it has one), at least one per token: a rough
/// sung-syllable count for the window cap that holds for Cyrillic, Hangul
/// or kana alike.
fn sung_syllables(
    tokens: &[String],
    romanizer: &uroman::Uroman,
    spelled: &[Option<String>],
) -> usize {
    tokens
        .iter()
        .enumerate()
        .map(|(i, token)| {
            let latin = match spelled.get(i) {
                Some(Some(said)) => said.clone(),
                _ => romanizer
                    .romanize_string::<uroman::rom_format::Str>(token, None)
                    .to_string()
                    .to_lowercase(),
            };
            let mut count = 0;
            let mut in_vowel = false;
            for c in latin.chars() {
                let vowel = "aeiouy".contains(c);
                if vowel && !in_vowel {
                    count += 1;
                }
                in_vowel = vowel;
            }
            count.max(1)
        })
        .sum()
}

/// A second listener for the lines the review doubts: the vocal separator
/// (`separation::VocalEar`), or the bench's cached copy of it.
pub trait SecondEar {
    /// Separated vocals for `range` of the recording (16 kHz samples), the
    /// same length.
    fn hear(&mut self, range: Range<usize>) -> Result<Vec<i16>>;
    /// What hearing `range` costs, in separator passes.
    fn passes(&self, range: Range<usize>) -> usize;
}

struct LineDecode {
    spans: Vec<TokenSpan>,
    guided: Option<EntryPrior>,
    retry: serde_json::Value,
    original_entry: i64,
    ratio: f64,
    /// Posterior mass near each spoken word's onset; empty with the review off.
    support: Vec<f32>,
    /// The emissions this decode came from.
    logp: Vec<f32>,
}

impl LineDecode {
    fn doubted(&self, below: f32) -> bool {
        below > 0.0 && self.support.iter().any(|&p| p < below)
    }

    /// How sure the model is of the line as placed: the geometric mean of
    /// its words' support, so one lost word counts against the line.
    fn score(&self) -> f32 {
        let logs: f32 = self.support.iter().map(|p| p.max(1e-3).ln()).sum();
        logs / self.support.len().max(1) as f32
    }

    fn onsets(&self, plan: &TokenPlan, begin: usize, map: &TimeMap) -> Vec<i64> {
        plan.ranges
            .iter()
            .flatten()
            .map(|r| {
                let frame = begin as f64 + self.spans[r.start].start as f64 * self.ratio;
                (map.intercept_ms + frame * map.slope_ms).round() as i64
            })
            .collect()
    }
}

/// One line between the first pass and the words: where it was looked
/// for, what it was looking for, and what was found.
struct LineWork {
    li: usize,
    next: i64,
    begin: usize,
    end: usize,
    plan: TokenPlan,
    guide_next: Option<i64>,
    decode: LineDecode,
    /// The second look's diagnostics, when there was one.
    second: serde_json::Value,
    /// The second ear heard this line again (whichever decode won).
    reheard: bool,
}

pub struct AcousticAligner {
    /// None for a decoder that only re-decodes saved takes.
    session: Option<Session>,
    romanizer: uroman::Uroman,
    emissions: Option<EmissionCache>,
    /// naist-jdic beside the model, until the first Japanese song loads it.
    japanese_dir: Option<PathBuf>,
    japanese: Option<japanese::Reader>,
    /// Every window's emissions this align decoded (`record_take`), or,
    /// while `replay`, the saved ones it decodes instead of the model's.
    take: Option<Take>,
    replay: bool,
}

/// A window the saved take doesn't hold (`AcousticAligner::redecode`).
const NOT_SAVED: &str = "window not in the saved take";

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
            // Every line window is a different length, so a memory pattern
            // planned for one shape never fits the next (same output).
            .with_memory_pattern(false)
            .map_err(|e| e.to_string())?
            .commit_from_file(model)
            .map_err(|e| e.to_string())?;
        Ok(Self::with_session(Some(session), model.parent()))
    }

    fn with_session(session: Option<Session>, model_dir: Option<&Path>) -> Self {
        Self {
            session,
            romanizer: uroman::Uroman::new(),
            emissions: None,
            japanese_dir: model_dir
                .map(|dir| dir.join(japanese::DICTIONARY_DIR))
                .filter(|dir| dir.is_dir()),
            japanese: None,
            take: None,
            replay: false,
        }
    }

    /// The decoder alone, for `redecode`: no model or runtime loads, so it
    /// costs no memory beyond the Japanese dictionary when a song needs it.
    /// `model_dir` is where that dictionary sits (the karaoke-model folder).
    pub fn decoder_only(model_dir: &Path) -> Self {
        Self::with_session(None, Some(model_dir))
    }

    /// Keep every window's emissions the next align decodes, for
    /// `finish_take`. `pcm_len` and `map` are that align's.
    pub fn record_take(&mut self, pcm_len: usize, map: &TimeMap) {
        self.replay = false;
        self.take = Some(Take::new(pcm_len, map.clone()));
    }

    /// What the align since `record_take` decoded.
    pub fn finish_take(&mut self) -> Option<Take> {
        self.take.take().filter(|_| !self.replay)
    }

    /// The listen `take` holds, decoded again by today's decoder: the same
    /// path as `align_reviewed`, or `align` when the take has no vocals,
    /// with the saved emissions in place of the model's. A doubted line
    /// whose vocals weren't saved keeps the mix's decode, as if the second
    /// ear ran out of time. Fails when a mix window is missing (the decoder
    /// moved a window), so the song relearns by listening instead.
    pub fn redecode(&mut self, take: Take, lines: &[TimedLine]) -> Result<Vec<Word>> {
        // Zeros only stand in for the recording's length: every window's
        // emissions come from the take. calloc'd, so this costs no memory.
        let pcm = vec![0i16; take.samples];
        let map = take.map.clone();
        self.take = Some(take);
        self.replay = true;
        let mut ear = SavedEar;
        let words = self.align_inner(
            &pcm,
            lines,
            &map,
            500.0,
            None,
            EntryMode::None,
            Knobs::MIX,
            Some(&mut ear),
        );
        self.take = None;
        self.replay = false;
        words
    }

    /// Letters for a Japanese line's tokens (`japanese::spell`), reading
    /// kanji with the dictionary when it is installed. The dictionary
    /// loads on the first Japanese song and goes with the aligner.
    fn spell_japanese(&mut self, tokens: &[String]) -> Vec<Option<String>> {
        if let Some(dir) = self.japanese_dir.take() {
            match japanese::Reader::load(&dir) {
                Ok(reader) => {
                    log::info!("karaoke: Japanese readings loaded");
                    self.japanese = Some(reader);
                }
                Err(e) => log::warn!("karaoke: Japanese readings unavailable ({e})"),
            }
        }
        japanese_letters(&self.romanizer, self.japanese.as_ref(), tokens)
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
            return self.align_inner(
                pcm,
                lines,
                map,
                500.0,
                diagnostics,
                EntryMode::None,
                knobs,
                None,
            );
        }
        let mut words = self.align_inner(
            pcm,
            lines,
            map,
            500.0,
            diagnostics,
            EntryMode::Guide,
            knobs,
            None,
        )?;
        entry_timing::refine(pcm, map, &mut words);
        Ok(words)
    }

    /// The mix path with its second look: a line the review doubts is
    /// decoded again on `ear`'s separated vocals for the same window, and
    /// the decode the model is surest of wins (`Knobs::review_below`).
    pub fn align_reviewed(
        &mut self,
        pcm: &[i16],
        lines: &[TimedLine],
        map: &TimeMap,
        knobs: Knobs,
        ear: &mut dyn SecondEar,
        diagnostics: Option<&mut Vec<serde_json::Value>>,
    ) -> Result<Vec<Word>> {
        self.align_inner(
            pcm,
            lines,
            map,
            500.0,
            diagnostics,
            EntryMode::None,
            knobs,
            Some(ear),
        )
    }

    /// Offline evidence seam on the shipped decoder (`Knobs::MIX`, the same
    /// path as `align`). Production always uses the 500ms window above.
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
            None,
        )
    }

    /// Offline comparison baseline: the vocal path on the pre-2026-10-31
    /// decoder (`Knobs::BASELINE`), with entry refinement but no retry.
    pub fn align_entries(
        &mut self,
        pcm: &[i16],
        lines: &[TimedLine],
        map: &TimeMap,
    ) -> Result<Vec<Word>> {
        let mut words = self.align_inner(
            pcm,
            lines,
            map,
            500.0,
            None,
            EntryMode::Refine,
            Knobs::BASELINE,
            None,
        )?;
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
            None,
        )?;
        entry_timing::refine(pcm, map, &mut words);
        Ok(words)
    }

    // Every public entry point is a thin preset over this one loop.
    /// Every line decoded on the recording as captured, in its own window.
    #[allow(clippy::too_many_arguments)]
    fn first_pass(
        &mut self,
        pcm: &[i16],
        lines: &[TimedLine],
        map: &TimeMap,
        margin_ms: f64,
        entries: bool,
        knobs: Knobs,
        want: bool,
    ) -> Result<Vec<LineWork>> {
        let to_time = |sample: f64| (map.intercept_ms + sample * map.slope_ms).round() as i64;
        let mut work: Vec<LineWork> = Vec::new();
        let numerals = Numerals::for_song(lines.iter().map(|l| l.text.as_str()));
        for (li, line) in lines.iter().enumerate() {
            let tokens = tokenize(&line.text);
            if tokens.is_empty() {
                continue;
            }
            let next = lines.get(li + 1).map_or(to_time(pcm.len() as f64), |l| l.t);
            let spelled = if numerals == Numerals::Japanese {
                self.spell_japanese(&tokens)
            } else {
                Vec::new()
            };
            // A held "ooh" has no syllable budget (eight seconds apiece on
            // JamendoLyrics' Fantasma), so vocable lines keep the full gap.
            let capped = if tokens.iter().all(|t| is_vocable(t)) {
                next
            } else {
                knobs.window_next(
                    line.t,
                    next,
                    sung_syllables(&tokens, &self.romanizer, &spelled),
                )
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
            let mut plan = match plan_tokens(tokens, &self.romanizer, numerals, &spelled) {
                Ok(plan) => plan,
                // One unsayable word leaves its line on line timing; the
                // rest of the song still learns.
                Err(e) if e.starts_with(UNREPRESENTABLE) => {
                    log::warn!("karaoke: no words for the line at {}ms ({e})", line.t);
                    continue;
                }
                Err(e) => return Err(e),
            };
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
            let guide_next = lines.get(li + 1).map(|l| l.t).or(knobs
                .guide_last
                .then(|| to_time(end as f64).min(line.t + 20_000)));
            let decode = self.decode_line(
                &pcm[begin..end],
                begin,
                Ear::Mix,
                &plan,
                line,
                guide_next,
                map,
                knobs,
                want,
            )?;
            work.push(LineWork {
                li,
                next,
                begin,
                end,
                plan,
                guide_next,
                decode,
                second: serde_json::Value::Null,
                reheard: false,
            });
        }
        Ok(work)
    }

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
        ear: Option<&mut dyn SecondEar>,
    ) -> Result<Vec<Word>> {
        let entries = mode != EntryMode::None;
        if !margin_ms.is_finite() || !(0.0..=1500.0).contains(&margin_ms) {
            return Err("probe margin must be 0..=1500ms".into());
        }
        if !map.slope_ms.is_finite() || map.slope_ms <= 0.0 || !map.intercept_ms.is_finite() {
            return Err("invalid capture time map".into());
        }
        let to_time = |sample: f64| (map.intercept_ms + sample * map.slope_ms).round() as i64;
        let want = diagnostics.is_some();
        // Every line on the recording as captured, then the second look at
        // the doubted ones, then the words in line order.
        let original = lines;
        let mut moved: Vec<TimedLine>;
        let mut lines = lines;
        let mut work = self.first_pass(pcm, lines, map, margin_ms, entries, knobs, want)?;
        // A song whose lines mostly start before their stamps (a late LRC)
        // is looked for again with every window moved to where the singer
        // is; the words still carry the stamps the lyrics came with.
        let mut shift = 0;
        for _ in 0..RECENTRE_ROUNDS {
            let Some(by) = song_shift(&work, lines, map, knobs) else {
                break;
            };
            shift += by;
            log::info!(
                "karaoke: lines start {}ms before their stamps; looking again {}ms earlier",
                -by,
                -shift
            );
            moved = original
                .iter()
                .map(|l| TimedLine {
                    t: l.t + shift,
                    text: l.text.clone(),
                })
                .collect();
            lines = &moved;
            work = self.first_pass(pcm, lines, map, margin_ms, entries, knobs, want)?;
        }
        if let Some(ear) = ear {
            self.second_look(lines, map, knobs, &mut work, ear, want);
        }
        let mut words = Vec::new();
        for LineWork {
            li,
            next,
            begin,
            end,
            plan,
            decode,
            second,
            reheard,
            ..
        } in work
        {
            let line = &lines[li];
            let stamp = original[li].t;
            let LineDecode {
                spans,
                guided,
                retry,
                original_entry,
                ratio,
                support,
                ..
            } = decode;
            let positions = word_frames(&plan.ranges, &spans);
            let line_start = words.len();
            // `support` holds one entry per spoken word, in order.
            let mut supports = support.iter();
            if let Some(rows) = diagnostics.as_deref_mut() {
                rows.push(serde_json::json!({
                    "line_index": li, "line_t": stamp, "looked_from": line.t, "next_t": next,
                    "source_guided": guided.is_some(), "support": support, "second_ear": second,
                    "original_entry": original_entry, "retry": retry,
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
                let support = range
                    .as_ref()
                    .and_then(|_| supports.next())
                    .map(|&p| (p.clamp(0.0, 1.0) * 100.0).round() / 100.0);
                words.push(Word {
                    text,
                    t: to_time(begin as f64 + start as f64 * ratio),
                    end: Some(to_time(begin as f64 + end as f64 * ratio)),
                    line_t: Some(stamp),
                    points,
                    support,
                    reheard,
                    voice: None,
                });
            }
            if knobs.vocable_tolerance_ms > 0 {
                if let Some(next_line) = lines.get(li + 1) {
                    // Never spread past what was decoded or recorded.
                    let bound = next_line
                        .t
                        .min(line.t + 20_000)
                        .min(to_time(pcm.len() as f64));
                    vocable_fallback(
                        &mut words[line_start..],
                        line.t,
                        bound,
                        knobs.vocable_tolerance_ms,
                    );
                }
            }
        }
        Ok(words)
    }

    /// The review's repair: the doubted lines `plan_second_look` fits in
    /// the budget, decoded again on the second ear's vocals for the same
    /// window and, with `Knobs::review_blend`, on both ears mixed. The
    /// decode the model is surest of keeps the line (`surest`).
    #[allow(clippy::too_many_arguments)]
    fn second_look(
        &mut self,
        lines: &[TimedLine],
        map: &TimeMap,
        knobs: Knobs,
        work: &mut [LineWork],
        ear: &mut dyn SecondEar,
        want: bool,
    ) {
        let doubted: Vec<usize> = (0..work.len())
            .filter(|&i| work[i].decode.doubted(knobs.review_below))
            .collect();
        let asks: Vec<(f32, Range<usize>)> = doubted
            .iter()
            .map(|&i| (work[i].decode.score(), work[i].begin..work[i].end))
            .collect();
        let plan = plan_second_look(&asks, knobs.review_passes, |r| ear.passes(r));
        let to_time = |sample: usize| (map.intercept_ms + sample as f64 * map.slope_ms).round();
        for (stretch, members) in plan {
            let vocals = match ear.hear(stretch.clone()) {
                Ok(v) if v.len() == stretch.len() => v,
                Ok(_) => {
                    log::warn!("karaoke: second ear returned a different stretch");
                    continue;
                }
                Err(e) => {
                    log::warn!("karaoke: second ear skipped {} lines ({e})", members.len());
                    continue;
                }
            };
            for m in members {
                let w = &mut work[doubted[m]];
                let line = &lines[w.li];
                let window = &vocals[w.begin - stretch.start..w.end - stretch.start];
                let heard = self.decode_line(
                    window,
                    w.begin,
                    Ear::Vocals,
                    &w.plan,
                    line,
                    w.guide_next,
                    map,
                    knobs,
                    want,
                );
                let mut candidates = match heard {
                    Ok(vocal) => vec![vocal],
                    // The listen being re-decoded never heard this line
                    // again: it keeps the mix's decode.
                    Err(e) if e == NOT_SAVED => continue,
                    Err(e) => {
                        log::warn!("karaoke: second ear skipped a line ({e})");
                        continue;
                    }
                };
                if knobs.review_blend {
                    let blended = mix_ears(&w.decode.logp, &candidates[0].logp).and_then(|logp| {
                        decode_emissions(
                            logp,
                            window.len(),
                            w.begin,
                            &w.plan,
                            line,
                            w.guide_next,
                            map,
                            knobs,
                            want,
                        )
                    });
                    match blended {
                        Ok(blended) => candidates.push(blended),
                        Err(e) => log::warn!("karaoke: blended ears skipped a line ({e})"),
                    }
                }
                let pick = surest(&w.decode, &candidates);
                w.reheard = true;
                if want {
                    w.second = serde_json::json!({
                        "candidates": candidates.iter().map(|c| serde_json::json!({
                            "onsets": c.onsets(&w.plan, w.begin, map),
                            "support": c.support,
                        })).collect::<Vec<_>>(),
                        "own_support": w.decode.support,
                        "taken": pick,
                        "stretch": [to_time(stretch.start), to_time(stretch.end)],
                    });
                }
                if let Some(i) = pick {
                    w.decode = candidates.swap_remove(i);
                }
            }
        }
    }

    /// One line's Viterbi path on `window` (the recording from sample
    /// `begin`), the source-entry retry, and, when the review is on, how
    /// much posterior each spoken word's onset holds.
    #[allow(clippy::too_many_arguments)]
    fn decode_line(
        &mut self,
        window: &[i16],
        begin: usize,
        ear: Ear,
        plan: &TokenPlan,
        line: &TimedLine,
        guide_next: Option<i64>,
        map: &TimeMap,
        knobs: Knobs,
        diagnostics: bool,
    ) -> Result<LineDecode> {
        let logp = self.emissions_for(window, begin, ear)?;
        decode_emissions(
            logp,
            window.len(),
            begin,
            plan,
            line,
            guide_next,
            map,
            knobs,
            diagnostics,
        )
    }

    /// Model log-probabilities for one window (`ear`'s audio from recording
    /// sample `begin`): from the saved take while re-decoding, else through
    /// the offline cache when one is installed, else the model. Recorded
    /// into the take when one is being kept.
    fn emissions_for(&mut self, input: &[i16], begin: usize, ear: Ear) -> Result<Vec<f32>> {
        if self.replay {
            return self
                .take
                .as_ref()
                .and_then(|take| take.emissions(ear, begin, input.len()))
                .ok_or_else(|| NOT_SAVED.to_string());
        }
        let logp = self.model_emissions(input)?;
        if let Some(take) = self.take.as_mut() {
            take.record(ear, begin, input.len(), &logp);
        }
        Ok(logp)
    }

    fn model_emissions(&mut self, input: &[i16]) -> Result<Vec<f32>> {
        let key = self.emissions.as_ref().map(|_| fingerprint(input));
        if let (Some(cache), Some(key)) = (&self.emissions, key) {
            if let Some(hit) = cache.get(&key) {
                return Ok(hit.clone());
            }
        }
        let session = self
            .session
            .as_mut()
            .ok_or("no acoustic model loaded (decoder only)")?;
        let samples: Vec<f32> = input.iter().map(|&s| s as f32 / 32768.0).collect();
        let tensor =
            Tensor::from_array(([1usize, samples.len()], samples)).map_err(|e| e.to_string())?;
        let output = session
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

/// The second ear while re-decoding a saved take: its "vocals" are never
/// read, since every window's emissions come from the take (or are missing
/// from it, `NOT_SAVED`).
struct SavedEar;

impl SecondEar for SavedEar {
    fn hear(&mut self, range: Range<usize>) -> Result<Vec<i16>> {
        Ok(vec![0; range.len()])
    }

    fn passes(&self, _: Range<usize>) -> usize {
        0
    }
}

/// `decode_line` on emissions already in hand, such as both ears mixed
/// (`mix_ears`).
#[allow(clippy::too_many_arguments)]
fn decode_emissions(
    emissions: Vec<f32>,
    window_len: usize,
    begin: usize,
    plan: &TokenPlan,
    line: &TimedLine,
    guide_next: Option<i64>,
    map: &TimeMap,
    knobs: Knobs,
    diagnostics: bool,
) -> Result<LineDecode> {
    let to_time = |sample: f64| (map.intercept_ms + sample * map.slope_ms).round() as i64;
    let logp = emissions.as_slice();
    let ratio = window_len as f64 / (logp.len() / CLASSES) as f64;
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
    let mut guided = None;
    let mut retry = serde_json::Value::Null;
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
            deadline: (line.t as f64 + 500.0 - (map.intercept_ms + begin as f64 * map.slope_ms))
                / (ratio * map.slope_ms),
            penalty_per_frame: (8.0 * ratio * map.slope_ms / 1000.0) as f32,
        };
        if let Ok(candidate) = ctc_spans_with_prior(logp, CLASSES, &plan.targets, Some(prior)) {
            let accepted = supported_entry_retry(&spans, &candidate, first)
                || (knobs.connected_retry
                    && connected_entry_retry(&spans, &candidate, first, frame_ms));
            if diagnostics {
                retry = serde_json::json!({
                    "entry": to_time(begin as f64 + candidate[first].start as f64 * ratio),
                    "support": candidate[first].confidence,
                    "original_support": spans[first].confidence,
                    "accepted": accepted,
                });
            }
            if accepted {
                spans = candidate;
                guided = Some(prior);
            }
        }
    }
    let mut support = Vec::new();
    if knobs.review_below > 0.0 {
        let radius = (REVIEW_RADIUS_MS / frame_ms).round() as usize;
        match review::Posterior::new(logp, CLASSES, &plan.targets, guided) {
            Ok(posterior) => {
                support = plan
                    .ranges
                    .iter()
                    .flatten()
                    .map(|r| posterior.start_mass(r.start, spans[r.start].start, radius))
                    .collect();
            }
            Err(e) => log::warn!("karaoke: review skipped a line ({e})"),
        }
    }
    Ok(LineDecode {
        spans,
        guided,
        retry,
        original_entry,
        ratio,
        support,
        logp: emissions,
    })
}

/// Both ears' emissions for the same window as an even mixture, frame by
/// frame: a letter either ear heard keeps at least half its probability,
/// so a path can take each stretch from whichever ear heard it. (A product
/// instead, where both must hear it, did worse than either ear alone.)
fn mix_ears(a: &[f32], b: &[f32]) -> Result<Vec<f32>> {
    if a.len() != b.len() || a.is_empty() || !a.len().is_multiple_of(CLASSES) {
        return Err("ears disagree on the window's frames".into());
    }
    let mut out = Vec::with_capacity(a.len());
    let mut row = [0f32; CLASSES];
    let (fa, fb) = (a.as_chunks::<CLASSES>().0, b.as_chunks::<CLASSES>().0);
    for (fa, fb) in fa.iter().zip(fb) {
        for ((cell, &x), &y) in row.iter_mut().zip(fa).zip(fb) {
            let m = x.max(y);
            *cell = m + ((x - m).exp() + (y - m).exp()).ln();
        }
        // Renormalized, which also takes out the halving.
        let m = row.iter().fold(f32::NEG_INFINITY, |m, &v| m.max(v));
        let z = m + row.iter().map(|v| (v - m).exp()).sum::<f32>().ln();
        out.extend(row.iter().map(|v| v - z));
    }
    Ok(out)
}

/// The song is looked for again at most this many times: a first move
/// can only see as far as the old windows reached.
const RECENTRE_ROUNDS: usize = 2;
/// How many lines it takes to say a song's stamps run late.
const RECENTRE_MIN_LINES: usize = 8;

/// How far a song's lines start from their stamps: the median of
/// `offsets` (each line's first word minus its stamp, for lines whose
/// first word isn't doubted), when the lines start `late_ms` or more
/// before their stamps. That is a late LRC, not a singer: on on-time
/// stamps a line's first word sits 0.1 to 0.4 s after its stamp. A window
/// reaches only 500 ms before its stamp, so on such a song first words
/// pile up at the window's edge and the bad words double (9.6% to 19.5%
/// more than 300 ms off with stamps 600 ms late, 14 JamendoLyrics songs).
pub(crate) fn late_stamps(mut offsets: Vec<i64>, late_ms: i64) -> Option<i64> {
    if late_ms <= 0 || offsets.len() < RECENTRE_MIN_LINES {
        return None;
    }
    offsets.sort_unstable();
    let median = offsets[offsets.len() / 2];
    (median <= -late_ms).then_some(median)
}

/// How far to move every window (negative = earlier) when the first pass
/// says the stamps run late (`late_stamps`).
fn song_shift(work: &[LineWork], lines: &[TimedLine], map: &TimeMap, knobs: Knobs) -> Option<i64> {
    let offsets = work
        .iter()
        .filter(|w| {
            w.decode
                .support
                .first()
                .is_none_or(|&p| p >= knobs.review_below)
        })
        .filter_map(|w| {
            let first = w.plan.ranges.iter().flatten().next()?.start;
            let sample = w.begin as f64 + w.decode.spans[first].start as f64 * w.decode.ratio;
            Some((map.intercept_ms + sample * map.slope_ms).round() as i64 - lines[w.li].t)
        })
        .collect();
    late_stamps(offsets, knobs.recentre_ms)
}

/// Which of `candidates` should replace `own`: the one the model is surest
/// of, if it is strictly surer than `own`. Support is the only judge. A
/// rule that also kept every word the mix was sure of in place blocked as
/// many right fixes (a mix sure of the wrong copy of "wrong concept wrong
/// concept") as wrong ones.
fn surest(own: &LineDecode, candidates: &[LineDecode]) -> Option<usize> {
    let mut best = (own.score(), None);
    for (i, c) in candidates.iter().enumerate() {
        if c.score() > best.0 {
            best = (c.score(), Some(i));
        }
    }
    best.1
}

/// What the second ear hears: `asks` holds each doubted line's score and
/// window. Least sure first, a line joins while the passes still fit
/// `budget` (0 = no limit). Windows that overlap, or that cost no more
/// heard as one, share a stretch, so neighbouring doubted lines don't each
/// pay the separator's padding and part-filled passes. Returns each
/// stretch with the indices of its lines, the stretch holding the least
/// sure line first, so an ear that stops early has heard the worst.
fn plan_second_look(
    asks: &[(f32, Range<usize>)],
    budget: usize,
    passes: impl Fn(Range<usize>) -> usize,
) -> Vec<(Range<usize>, Vec<usize>)> {
    let stretches = |chosen: &[usize]| {
        let mut sorted = chosen.to_vec();
        sorted.sort_by_key(|&i| asks[i].1.start);
        let mut out: Vec<(Range<usize>, Vec<usize>)> = Vec::new();
        for i in sorted {
            let w = asks[i].1.clone();
            if let Some((r, members)) = out.last_mut() {
                let joined = r.start..r.end.max(w.end);
                if w.start <= r.end
                    || passes(joined.clone()) <= passes(r.clone()) + passes(w.clone())
                {
                    *r = joined;
                    members.push(i);
                    continue;
                }
            }
            out.push((w, vec![i]));
        }
        out
    };
    let mut order: Vec<usize> = (0..asks.len()).collect();
    order.sort_by(|&a, &b| asks[a].0.total_cmp(&asks[b].0));
    let mut chosen = Vec::new();
    let mut plan = Vec::new();
    for i in order {
        chosen.push(i);
        let trial = stretches(&chosen);
        if budget == 0 || trial.iter().map(|(r, _)| passes(r.clone())).sum::<usize>() <= budget {
            plan = trial;
        } else {
            chosen.pop();
        }
    }
    let least_sure = |members: &[usize]| {
        members
            .iter()
            .map(|&i| asks[i].0)
            .fold(f32::INFINITY, f32::min)
    };
    plan.sort_by(|a, b| least_sure(&a.1).total_cmp(&least_sure(&b.1)));
    plan
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
        let plan = plan_tokens(
            vec!["(hallway!) ".into()],
            &romanizer,
            Numerals::English,
            &[],
        )
        .unwrap();
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
        let plan = plan_tokens(tokenize("안녕!"), &roman, Numerals::English, &[]).unwrap();
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
        let quotes = plan_tokens(tokenize("'안녕'"), &roman, Numerals::English, &[]).unwrap();
        assert!(quotes.ranges.first().unwrap().is_none());
        assert!(quotes.ranges.last().unwrap().is_none());
        let contraction =
            plan_tokens(vec!["don't".into()], &roman, Numerals::English, &[]).unwrap();
        assert!(contraction.targets.contains(&25));
        let lead = plan_tokens(
            vec!["!".into(), "hello".into()],
            &roman,
            Numerals::English,
            &[],
        )
        .unwrap();
        let frames = word_frames(&lead.ranges, &spans);
        assert_eq!(frames[0].0, frames[1].0);
        assert!(
            plan_tokens(vec!["!".into()], &roman, Numerals::English, &[])
                .unwrap()
                .ranges
                .iter()
                .all(Option::is_none)
        );
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
        // Index 2 is the trailing wildcard here: nothing sung follows.
        assert!(!connected_entry_retry(&original, &far, 1, 20.0));
        let mut sung = far.to_vec();
        sung.push(TokenSpan {
            start: 900,
            end: 910,
            confidence: 0.9,
        });
        assert!(connected_entry_retry(&original, &sung, 1, 20.0));
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
                ..Default::default()
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
        let romanizer = uroman::Uroman::new();
        let count = |text: &str| sung_syllables(&tokenize(text), &romanizer, &[]);
        assert_eq!(count("intentando destacar"), 7);
        assert_eq!(count("will burn"), 2);
        assert_eq!(count("rhythm"), 1);
        assert_eq!(count("mmm"), 1);
        assert_eq!(count("canción él"), 3);
        assert_eq!(count("사랑해"), 3);
        assert_eq!(count("здравствуйте"), 3);
        assert_eq!(count("καλημέρα"), 4);
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
        // These values ARE recipe mms-int8/4; changing one changes RECIPE.
        // The pass budget is the exception: lifting it only re-hears lines
        // the cap skipped, and karaoke's detail_v 5 relearns those saves.
        let tuned = Knobs {
            guide: true,
            guide_late_ms: 700,
            guide_last: true,
            cap_per_syllable_ms: 400,
            cap_base_ms: 3000,
            vocable_tolerance_ms: 1500,
            connected_retry: true,
            review_below: 0.5,
            review_blend: true,
            review_passes: 0,
            // Not in the recipe either: it changes nothing for a song
            // whose stamps are on time, and karaoke's detail_v 6 relearns
            // the saves whose stamps run late.
            recentre_ms: 250,
        };
        assert_eq!(Knobs::MIX, tuned);
        assert_eq!(Knobs::TRIAL, tuned);
        assert_eq!(RECIPE, "mms-int8/4");
    }

    #[test]
    fn a_song_is_late_when_its_lines_mostly_start_before_their_stamps() {
        // On-time stamps: lines start a little after them, one early.
        let on_time = vec![140, 60, 380, -420, 150, 20, 260, 90, 110];
        assert_eq!(late_stamps(on_time, 250), None);
        // A late LRC: most first words sit at the window's edge.
        let late = vec![-480, -480, -470, -300, -480, 120, -480, -460, -480];
        assert_eq!(late_stamps(late, 250), Some(-480));
        // Too few lines to say, and off.
        assert_eq!(late_stamps(vec![-480; 7], 250), None);
        assert_eq!(late_stamps(vec![-480; 9], 0), None);
    }

    fn decode(starts: &[usize], support: &[f32]) -> LineDecode {
        LineDecode {
            spans: starts
                .iter()
                .map(|&start| TokenSpan {
                    start,
                    end: start + 1,
                    confidence: 1.0,
                })
                .collect(),
            guided: None,
            retry: serde_json::Value::Null,
            original_entry: 0,
            // 320 samples a frame: 20ms at 16kHz.
            ratio: 320.0,
            support: support.to_vec(),
            logp: Vec::new(),
        }
    }

    #[test]
    fn the_surest_decode_keeps_the_line() {
        // Three one-letter words; frames are 20ms.
        let plan = TokenPlan {
            tokens: vec!["a ".into(), "b ".into(), "c".into()],
            targets: vec![STAR, 1, 2, 3, STAR],
            ranges: vec![Some(1..2), Some(2..3), Some(3..4)],
        };
        let map = TimeMap::from_origin(0, 16_000);
        let mix = decode(&[0, 10, 200, 0, 0], &[0.95, 0.2, 0.9]);
        assert_eq!(mix.onsets(&plan, 0, &map), [200, 4000, 0]);
        let vocals = decode(&[0, 12, 40, 1, 0], &[0.9, 0.95, 0.9]);
        let blended = decode(&[0, 12, 40, 1, 0], &[0.95, 0.95, 0.95]);
        let unsure = decode(&[0, 10, 60, 0, 0], &[0.9, 0.1, 0.9]);
        assert_eq!(surest(&mix, std::slice::from_ref(&unsure)), None);
        assert_eq!(surest(&mix, std::slice::from_ref(&vocals)), Some(0));
        assert_eq!(surest(&mix, &[vocals, blended]), Some(1));
        // A tie keeps what the mix decoded.
        let same = decode(&[0, 12, 40, 1, 0], &[0.95, 0.2, 0.9]);
        assert_eq!(surest(&mix, &[same]), None);
        // One lost word weighs more than a mean would give it.
        assert!(decode(&[0], &[0.6, 0.6]).score() > decode(&[0], &[1.0, 0.01]).score());
    }

    #[test]
    fn the_second_look_spends_its_passes_on_the_least_sure_lines() {
        // A pass covers 10 samples and the ear pads each stretch by one
        // sample a side, as the separator pads by a second.
        let passes = |r: Range<usize>| (r.len() + 2) / 10 + 1;
        // The least sure line, its overlapping neighbour, a far line.
        let asks = [(-3.0, 0..8), (-1.0, 7..15), (-2.0, 40..48)];
        assert_eq!(
            plan_second_look(&asks, 0, passes),
            [(0..15, vec![0, 1]), (40..48, vec![2])]
        );
        // The stretch with the least sure line is heard first.
        let later = [(-1.0, 0..8), (-3.0, 40..48)];
        assert_eq!(
            plan_second_look(&later, 0, passes),
            [(40..48, vec![1]), (0..8, vec![0])]
        );
        // Two passes: the far line no longer fits, but the neighbour
        // shares the first line's passes for free.
        assert_eq!(plan_second_look(&asks, 2, passes), [(0..15, vec![0, 1])]);
        assert!(plan_second_look(&asks, 1, passes).is_empty());
        // A gap is heard along when that costs nothing more...
        let near = [(-1.0, 0..3), (-1.0, 5..8)];
        assert_eq!(plan_second_look(&near, 0, passes), [(0..8, vec![0, 1])]);
        // ...and skipped when it would.
        let far = [(-1.0, 0..8), (-1.0, 30..38)];
        assert_eq!(plan_second_look(&far, 0, passes).len(), 2);
    }

    #[test]
    fn blended_ears_keep_what_either_heard() {
        let frame = |hot: usize| -> Vec<f32> {
            let mut row = vec![-20.0f32; CLASSES];
            row[hot] = 0.0;
            row
        };
        let (a, b) = (frame(1), frame(2));
        let mixed = mix_ears(&a, &b).unwrap();
        let p: Vec<f32> = mixed.iter().map(|v| v.exp()).collect();
        assert!((p.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        assert!((p[1] - 0.5).abs() < 1e-3 && (p[2] - 0.5).abs() < 1e-3);
        assert!(mix_ears(&a, &b[..CLASSES - 1]).is_err());
        assert!(mix_ears(&[], &[]).is_err());
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
        let plan = plan_tokens(tokens, &roman, Numerals::English, &[]).unwrap();
        assert_eq!(plan.tokens, original);
        let range = plan.ranges[3].clone().unwrap();
        let spoken: String = plan.targets[range]
            .iter()
            .map(|&i| LABELS[i] as char)
            .collect();
        assert_eq!(spoken, "four");
        let slang = plan_tokens(vec!["4sho".into()], &roman, Numerals::English, &[]).unwrap();
        let spoken: String = slang.targets[slang.ranges[0].clone().unwrap()]
            .iter()
            .map(|&i| LABELS[i] as char)
            .collect();
        assert_eq!(spoken, "foursho");
        assert_eq!(slang.tokens[0], "4sho");
        assert_eq!(expand_digit("12"), "12");
        assert_eq!(expand_digit("hello"), "hello");
        // A digit is never dropped: glued to hangul it reads Sino-Korean,
        // alone it reads English.
        let spell = |plan: &TokenPlan, i: usize| -> String {
            plan.targets[plan.ranges[i].clone().unwrap()]
                .iter()
                .map(|&i| LABELS[i] as char)
                .collect()
        };
        let april = plan_tokens(tokenize("4월"), &roman, Numerals::Korean, &[]).unwrap();
        assert_eq!(spell(&april, 0), "sa");
        let four = plan_tokens(vec!["4".into()], &roman, Numerals::Korean, &[]).unwrap();
        assert_eq!(spell(&four, 0), "four");
    }

    #[test]
    fn japanese_marks_and_numerals_spell_instead_of_failing() {
        // あぶく (Yorushika) never synced: ー, 々 and 一 each failed the song.
        let roman = uroman::Uroman::new();
        let ja = Numerals::Japanese;
        let plan = plan_tokens(tokenize("トートロジー握った"), &roman, ja, &[]).unwrap();
        let spelled: Vec<String> = plan
            .ranges
            .iter()
            .map(|r| {
                plan.targets[r.clone().unwrap()]
                    .iter()
                    .map(|&i| LABELS[i] as char)
                    .collect()
            })
            .collect();
        assert_eq!(spelled[..6], ["to", "o", "to", "ro", "ji", "i"]);
        let plan = plan_tokens(tokenize("悠々悠々夢を焼け"), &roman, ja, &[]).unwrap();
        assert!(plan.ranges.iter().all(Option::is_some));
        assert_eq!(
            plan.ranges[1].clone().unwrap().len(),
            plan.ranges[0].clone().unwrap().len()
        );
        assert!(plan_tokens(tokenize("一人を竹刀で"), &roman, ja, &[]).is_ok());
        assert!(plan_tokens(tokenize("夕夕夕夕夕、目を焼け"), &roman, ja, &[]).is_ok());
        // Still nothing to say: the line is skipped by the caller, not the song.
        let err = plan_tokens(vec!["㐂".into()], &roman, ja, &[])
            .err()
            .unwrap();
        assert!(err.starts_with(UNREPRESENTABLE));
    }

    #[test]
    fn japanese_lines_spell_as_sung_without_the_dictionary() {
        // Spelled one character at a time, っ said "tsu" and きょ "kiyo".
        let roman = uroman::Uroman::new();
        let ja = Numerals::Japanese;
        let said = |text: &str| -> Vec<String> {
            let tokens = tokenize(text);
            let letters = japanese_letters(&roman, None, &tokens);
            let plan = plan_tokens(tokens, &roman, ja, &letters).unwrap();
            plan.ranges
                .iter()
                .map(|r| {
                    plan.targets[r.clone().unwrap()]
                        .iter()
                        .map(|&i| LABELS[i] as char)
                        .collect()
                })
                .collect()
        };
        assert_eq!(said("待ってきょう"), ["dai", "t", "te", "k", "yo", "u"]);
        assert_eq!(said("トートロジー"), ["to", "o", "to", "ro", "ji", "i"]);
        // Latin words in a Japanese song keep their own spelling.
        assert_eq!(said("baby ねぇ"), ["baby", "ne", "e"]);
        // A line-final っ still says something, so the line keeps its words.
        assert_eq!(said("あっ"), ["a", "tsu"]);
    }
}

#[cfg(test)]
mod take_tests {
    use super::*;

    const HZ: usize = 16_000;

    /// Log-probabilities for `frames` frames: blank everywhere except each
    /// (letter, from, to) run, each frame 99% sure of its class.
    fn rows(frames: usize, marks: &[(u8, usize, usize)]) -> Vec<f32> {
        let sure = 0.99f32.ln();
        let rest = (0.01f32 / (CLASSES - 1) as f32).ln();
        let mut out = Vec::with_capacity(frames * CLASSES);
        for t in 0..frames {
            let class = marks
                .iter()
                .find(|&&(_, a, b)| (a..b).contains(&t))
                .map_or(0, |&(c, _, _)| LABELS.iter().position(|&l| l == c).unwrap());
            out.extend((0..CLASSES).map(|k| if k == class { sure } else { rest }));
        }
        out
    }

    fn song() -> (Vec<TimedLine>, TimeMap, usize) {
        let lines = vec![
            TimedLine {
                t: 1_000,
                text: "go now".into(),
            },
            TimedLine {
                t: 4_000,
                text: "stop".into(),
            },
        ];
        (lines, TimeMap::from_origin(0, HZ as u32), 6 * HZ)
    }

    /// What the live align would have kept: each line's window (500 ms
    /// either side of its stamps), one 20 ms frame per 320 samples.
    fn take(skip_second: bool, shift_second: usize) -> Take {
        let (_, map, len) = song();
        let first = recorded_window(len, &map, 1_000, 4_000).unwrap();
        let second = recorded_window(len, &map, 4_000, 6_000).unwrap();
        assert_eq!(
            (first.clone(), second.clone()),
            (8_000..72_000, 56_000..96_000)
        );
        let mut take = Take::new(len, map);
        let go_now = [
            (b'g', 40, 42),
            (b'o', 42, 46),
            (b'n', 60, 62),
            (b'o', 62, 66),
            (b'w', 66, 69),
        ];
        take.record(
            Ear::Mix,
            first.start,
            first.len(),
            &rows(first.len() / 320, &go_now),
        );
        if !skip_second {
            let stop = [
                (b's', 20, 22),
                (b't', 22, 24),
                (b'o', 24, 28),
                (b'p', 28, 30),
            ];
            take.record(
                Ear::Mix,
                second.start + shift_second,
                second.len() - shift_second,
                &rows(second.len() / 320, &stop),
            );
        }
        take
    }

    #[test]
    fn a_saved_take_decodes_without_the_model() {
        let (lines, _, _) = song();
        let mut decoder = AcousticAligner::decoder_only(Path::new("no-such-dir"));
        let words = decoder.redecode(take(false, 0), &lines).unwrap();
        let onsets: Vec<(&str, i64)> = words.iter().map(|w| (w.text.trim(), w.t)).collect();
        // Frame f of a window from sample b is at (b + 320 f) / 16 ms.
        assert_eq!(onsets, [("go", 1_300), ("now", 1_700), ("stop", 3_900)]);
        assert!(words.iter().all(|w| w.support.is_some_and(|p| p > 0.9)));
        assert!(words.iter().all(|w| !w.reheard));
        // Decoding saved emissions is not a listen: nothing to keep.
        assert!(decoder.finish_take().is_none());
        // And with no model, nothing but a take can be decoded.
        let (lines, map, len) = song();
        assert!(decoder.align(&vec![0; len], &lines, &map).is_err());
    }

    #[test]
    fn a_window_the_take_lacks_fails_the_redecode() {
        let (lines, _, _) = song();
        let mut decoder = AcousticAligner::decoder_only(Path::new("no-such-dir"));
        let missing = decoder.redecode(take(true, 0), &lines).unwrap_err();
        assert_eq!(missing, NOT_SAVED);
        // A window that starts one sample elsewhere is a different window.
        let moved = decoder.redecode(take(false, 1), &lines).unwrap_err();
        assert_eq!(moved, NOT_SAVED);
        // The decoder recovers for the next song.
        assert!(decoder.redecode(take(false, 0), &lines).is_ok());
    }
}
