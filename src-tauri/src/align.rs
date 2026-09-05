//! Forced alignment of known lyric tokens onto recorded PCM.
//!
//! Pipeline, all at 10ms hops over the 16kHz mono recording:
//!
//! 1. `analyze` — one FFT pass produces a SuperFlux-style onset strength
//!    (positive log-magnitude spectral flux in the vocal band, against a
//!    frequency-max-filtered frame two hops back so vibrato and sustained
//!    notes don't read as attacks) plus a vocal-band energy envelope. The
//!    old first-difference RMS envelope was a crude high-pass: it heard
//!    cymbals and hi-hats louder than the singer.
//! 2. `global_offset` — the recording's origin came from a coarse SMTC
//!    position (Apple Music floors to whole seconds); LRCLIB's line stamps
//!    are the time base the line karaoke already trusts. One offset that
//!    maximizes onset strength at the stamps anchors the two, so words
//!    land in the same frame the lines highlight in.
//! 3. `align_line` — per line, onsets are peak-picked with an adaptive
//!    threshold and a monotonic DP hands each token an onset or none,
//!    scored against a syllable-weighted prior. Unassigned tokens
//!    interpolate between their assigned neighbours. A sung phrase with
//!    no gaps (the common case) therefore still snaps every word that has
//!    an audible attack; the old code only saw rises out of silence and
//!    fell back to spreading tokens by cumulative energy.

use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};

const HOP_MS: i64 = 10;
/// Vocal band for both the flux and the energy envelope. Below ~150Hz is
/// kick + bass; above ~5kHz is cymbals and air — neither says "syllable".
const BAND_LO_HZ: f32 = 150.0;
const BAND_HI_HZ: f32 = 5000.0;
/// Log compression gain on the (full-scale-normalized) magnitude — brings
/// quiet consonant attacks up toward loud vowels so both register as flux.
const LOG_GAIN: f32 = 100.0;
/// Magnitudes under this (−54dBFS) read as the floor: without it the log
/// amplifies noise-floor flicker and spectral splatter across a hundred
/// bins into a flux "peak" that outweighs the actual attack.
const MAG_FLOOR: f32 = 0.002;
/// Flux lag in frames (SuperFlux μ): comparing against a frame 20ms back
/// widens the rise a real attack produces; the max filter across ±1 bin
/// absorbs pitch drift.
const FLUX_LAG: usize = 2;
/// Onset strength is normalized by the local max over ~±this many frames
/// so a loud chorus and a quiet verse pick peaks by the same rule.
const NORM_BLOCK: usize = 150;
/// Peak picking: strict local max over ±2 frames, at least this fraction
/// of the local max, and above the local mean by a margin.
const PEAK_MIN: f32 = 0.10;
const PEAK_MEAN_W: usize = 40;
const PEAK_MEAN_MUL: f32 = 1.4;
const PEAK_MEAN_ADD: f32 = 0.05;
const ONSET_MIN_GAP: usize = 5;
/// Global offset search ±1.5s; needs at least this many in-range lines to
/// be trusted at all, and pays a mild penalty per ms so noise never picks
/// a large shift over a near-zero one. Each stamp votes with a triangular
/// kernel ±OFFSET_REACH frames wide (a flat window left every offset that
/// merely contained the attack tied, and the penalty then chose the
/// earliest — a 150ms bias); the kernel makes centring on the attack win.
const OFFSET_RANGE: i64 = 150;
const OFFSET_REACH: i64 = 12;
const OFFSET_MIN_LINES: usize = 4;
const OFFSET_PENALTY: f32 = 0.10;
/// A line's search window opens this far before its (offset-corrected)
/// stamp — stamps are hand-placed and a singer routinely leads them.
const LINE_PRE_MS: i64 = 200;
const LINE_POST_MS: i64 = 20;
const MIN_SPAN_MS: i64 = 80;
/// Vocal-band energy floor (relative to the window's peak) that bounds the
/// sung island; absolute floor below which the window is silence.
const PEAK_FLOOR: f32 = 1e-3;
const VOICE_REL: f32 = 0.12;
/// DP costs: distance from the prior in σ units, minus the onset's
/// strength (0..1); a token with no convincing onset pays FREE and is
/// interpolated instead.
const FREE_COST: f32 = 1.3;
const SIGMA_MIN_FRAMES: f32 = 20.0;
/// The last word's end: the first run of this many frames under the voice
/// floor after its onset.
const END_QUIET_FRAMES: usize = 3;

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

/// Rough syllable count: one per CJK/hangul glyph, ~1 + a little per extra
/// letter for Latin words ("a" ≈ 1, "beautiful" ≈ 2).
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

/// Per-frame features over the whole recording. Frame `i` starts at sample
/// `i * hop`; its time in recording ms is `i * HOP_MS`.
struct Feat {
    /// Onset strength normalized to the local max (0..1).
    norm: Vec<f32>,
    /// Prefix sum of `norm`, for the peak picker's local mean.
    cum: Vec<f32>,
    /// Vocal-band energy envelope (full-scale sine at one bin ≈ 0.08).
    band: Vec<f32>,
}

impl Feat {
    fn len(&self) -> usize {
        self.norm.len()
    }

    fn mean(&self, lo: usize, hi: usize) -> f32 {
        let hi = hi.min(self.len());
        if hi <= lo {
            return 0.0;
        }
        (self.cum[hi] - self.cum[lo]) / (hi - lo) as f32
    }
}

pub fn align(pcm: &[f32], sample_rate: u32, lines: &[TimedLine], origin_ms: i64) -> Vec<Word> {
    if sample_rate == 0 || pcm.is_empty() || lines.is_empty() {
        return Vec::new();
    }
    let Some(feat) = analyze(pcm, sample_rate) else {
        return Vec::new();
    };
    let offset = global_offset(&feat, lines, origin_ms);
    let pcm_end_ms = origin_ms + feat.len() as i64 * HOP_MS;
    let mut words = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let next_t = lines.get(i + 1).map(|n| n.t).unwrap_or(i64::MAX);
        if let Some(ws) = align_line(&feat, line, next_t, origin_ms, offset, pcm_end_ms) {
            words.extend(ws);
        }
    }
    words
}

fn analyze(pcm: &[f32], sample_rate: u32) -> Option<Feat> {
    let hop = (sample_rate as usize * HOP_MS as usize / 1000).max(1);
    let frame = (hop * 3).next_power_of_two().max(64);
    if pcm.len() < frame {
        return None;
    }
    let n_frames = (pcm.len() - frame) / hop + 1;
    let bin_hz = sample_rate as f32 / frame as f32;
    let lo = ((BAND_LO_HZ / bin_hz).round() as usize).max(1);
    let hi = ((BAND_HI_HZ / bin_hz).round() as usize).min(frame / 2 - 1);
    if hi < lo + 3 {
        return None;
    }
    let nb = hi - lo + 1;
    let window: Vec<f32> = (0..frame)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / frame as f32).cos())
        .collect();
    // Hann-windowed full-scale sine peaks at frame/4 in its bin.
    let mag_scale = 4.0 / frame as f32;
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(frame);
    let mut buf = vec![Complex::new(0.0f32, 0.0); frame];
    let mut scratch = vec![Complex::new(0.0f32, 0.0); fft.get_inplace_scratch_len()];
    // Ring of the last FLUX_LAG+1 log spectra; `cur` is filled fresh and
    // swapped in after the flux against frame i-FLUX_LAG is taken.
    let mut ring: Vec<Vec<f32>> = (0..=FLUX_LAG).map(|_| vec![0.0f32; nb]).collect();
    let mut cur = vec![0.0f32; nb];
    let mut osf = Vec::with_capacity(n_frames);
    let mut band = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let start = i * hop;
        for (k, slot) in buf.iter_mut().enumerate() {
            *slot = Complex::new(pcm[start + k] * window[k], 0.0);
        }
        fft.process_with_scratch(&mut buf, &mut scratch);
        let mut energy = 0.0f32;
        for (b, k) in (lo..=hi).enumerate() {
            let m = buf[k].norm() * mag_scale;
            energy += m * m;
            cur[b] = (1.0 + LOG_GAIN * m.max(MAG_FLOOR)).ln();
        }
        band.push((energy / nb as f32).sqrt());
        let flux = if i >= FLUX_LAG {
            let prev = &ring[(i - FLUX_LAG) % (FLUX_LAG + 1)];
            let mut sum = 0.0f32;
            for b in 0..nb {
                let p_lo = if b == 0 { prev[0] } else { prev[b - 1] };
                let p_hi = if b + 1 < nb {
                    prev[b + 1]
                } else {
                    prev[nb - 1]
                };
                let p = prev[b].max(p_lo).max(p_hi);
                sum += (cur[b] - p).max(0.0);
            }
            sum
        } else {
            0.0
        };
        osf.push(flux);
        std::mem::swap(&mut ring[i % (FLUX_LAG + 1)], &mut cur);
    }
    // Local-max normalization: block maxima, each frame sees its block and
    // both neighbours (a ~4.5s reach at 10ms hops).
    let n_blocks = n_frames.div_ceil(NORM_BLOCK);
    let mut block_max = vec![0.0f32; n_blocks];
    for (i, &v) in osf.iter().enumerate() {
        let b = i / NORM_BLOCK;
        block_max[b] = block_max[b].max(v);
    }
    let mut norm = Vec::with_capacity(n_frames);
    for (i, &v) in osf.iter().enumerate() {
        let b = i / NORM_BLOCK;
        let mut m = block_max[b];
        if b > 0 {
            m = m.max(block_max[b - 1]);
        }
        if b + 1 < n_blocks {
            m = m.max(block_max[b + 1]);
        }
        norm.push(if m > 1e-6 { v / m } else { 0.0 });
    }
    let mut cum = Vec::with_capacity(n_frames + 1);
    let mut acc = 0.0f32;
    cum.push(0.0);
    for &v in &norm {
        acc += v;
        cum.push(acc);
    }
    Some(Feat { norm, cum, band })
}

/// Recording-time offset of the line stamps: vocals for stamp `s` sit at
/// `s + offset` in the recording. 0 when there aren't enough lines in the
/// recording to vote.
fn global_offset(feat: &Feat, lines: &[TimedLine], origin_ms: i64) -> i64 {
    let n = feat.len() as i64;
    let frames: Vec<i64> = lines
        .iter()
        .map(|l| (l.t - origin_ms) / HOP_MS)
        .filter(|&f| f - OFFSET_RANGE - OFFSET_REACH >= 0 && f + OFFSET_RANGE + OFFSET_REACH < n)
        .collect();
    if frames.len() < OFFSET_MIN_LINES {
        return 0;
    }
    let mut best = (0i64, f32::MIN);
    for off in -OFFSET_RANGE..=OFFSET_RANGE {
        let mut score = 0.0f32;
        for &f in &frames {
            let c = f + off;
            let mut vote = 0.0f32;
            for d in -OFFSET_REACH..=OFFSET_REACH {
                let w = 1.0 - 0.6 * d.abs() as f32 / OFFSET_REACH as f32;
                vote = vote.max(feat.norm[(c + d) as usize] * w);
            }
            score += vote;
        }
        score -= OFFSET_PENALTY * frames.len() as f32 * off.abs() as f32 / OFFSET_RANGE as f32;
        if score > best.1 {
            best = (off, score);
        }
    }
    best.0 * HOP_MS
}

fn align_line(
    feat: &Feat,
    line: &TimedLine,
    next_t: i64,
    origin_ms: i64,
    offset: i64,
    pcm_end_ms: i64,
) -> Option<Vec<Word>> {
    let tokens = tokenize(&line.text);
    if tokens.is_empty() {
        return None;
    }
    let win_lo = (line.t + offset - LINE_PRE_MS).max(origin_ms);
    let win_hi = next_t
        .saturating_add(offset)
        .saturating_sub(LINE_POST_MS)
        .min(pcm_end_ms);
    if win_hi - win_lo < MIN_SPAN_MS {
        return None;
    }
    let fa = ((win_lo - origin_ms) / HOP_MS) as usize;
    let fb = (((win_hi - origin_ms) / HOP_MS) as usize).min(feat.len());
    if fb <= fa + 1 {
        return None;
    }
    let band = &feat.band[fa..fb];
    let peak = band.iter().copied().fold(0.0f32, f32::max);
    if peak < PEAK_FLOOR {
        return None;
    }
    let floor = peak * VOICE_REL;
    let first = fa + band.iter().position(|&v| v >= floor)?;
    let last = fa + band.iter().rposition(|&v| v >= floor).unwrap_or(0);
    if (last.saturating_sub(first) as i64) * HOP_MS < MIN_SPAN_MS {
        return None;
    }
    let onsets = pick_onsets(feat, fa, fb);
    let weights: Vec<f32> = tokens.iter().map(|t| token_weight(t)).collect();
    let starts = assign(&weights, &onsets, first, last);
    let to_ms = |f: usize| origin_ms + f as i64 * HOP_MS - offset;
    let mut words = Vec::with_capacity(tokens.len());
    for (i, tok) in tokens.iter().enumerate() {
        let t = to_ms(starts[i]);
        let end_frame = if i + 1 < starts.len() {
            starts[i + 1]
        } else {
            last_word_end(feat, starts[i], fb, floor)
        };
        words.push(Word {
            t,
            text: tok.clone(),
            end: Some(to_ms(end_frame).max(t + HOP_MS)),
        });
    }
    Some(words)
}

/// First run of END_QUIET_FRAMES under the voice floor after `from`, else
/// the window end.
fn last_word_end(feat: &Feat, from: usize, fb: usize, floor: f32) -> usize {
    let mut quiet = 0usize;
    for f in (from + 1)..fb {
        if feat.band[f] < floor {
            quiet += 1;
            if quiet >= END_QUIET_FRAMES {
                return f + 1 - quiet;
            }
        } else {
            quiet = 0;
        }
    }
    fb
}

/// Onset candidates in `[fa, fb)`: (frame, strength 0..1).
fn pick_onsets(feat: &Feat, fa: usize, fb: usize) -> Vec<(usize, f32)> {
    let n = feat.len();
    let mut out: Vec<(usize, f32)> = Vec::new();
    for i in fa..fb {
        let v = feat.norm[i];
        if v < PEAK_MIN {
            continue;
        }
        let lo = i.saturating_sub(2);
        let hi = (i + 2).min(n - 1);
        let local_max = feat.norm[lo..=hi].iter().copied().fold(0.0, f32::max);
        if v < local_max {
            continue;
        }
        // Strict on the left so a plateau yields one peak, not two.
        if i > 0 && feat.norm[i - 1] >= v {
            continue;
        }
        let mean = feat.mean(i.saturating_sub(PEAK_MEAN_W), i + PEAK_MEAN_W);
        if v < mean * PEAK_MEAN_MUL + PEAK_MEAN_ADD {
            continue;
        }
        if let Some(&(j, s)) = out.last() {
            if i - j < ONSET_MIN_GAP {
                if v > s {
                    out.pop();
                } else {
                    continue;
                }
            }
        }
        out.push((i, v));
    }
    out
}

/// Monotonic DP: token j takes onset m (cost = distance from its prior in σ
/// units minus the onset's strength) or no onset (FREE_COST). Returns one
/// start frame per token, strictly increasing, with free tokens
/// interpolated by weight between their assigned neighbours (or the sung
/// island's bounds).
fn assign(weights: &[f32], onsets: &[(usize, f32)], lo: usize, hi: usize) -> Vec<usize> {
    let n = weights.len();
    let total: f32 = weights.iter().sum::<f32>().max(1e-3);
    let span = (hi.saturating_sub(lo)).max(n) as f32;
    let mut prior = Vec::with_capacity(n);
    let mut acc = 0.0f32;
    for &w in weights {
        prior.push(lo as f32 + span * acc / total);
        acc += w;
    }
    let sigma = (span / n as f32).max(SIGMA_MIN_FRAMES);
    let k = onsets.len();
    let cost = |j: usize, m: usize| -> f32 {
        let (f, s) = onsets[m];
        (f as f32 - prior[j]).abs() / sigma - s
    };
    // dp[m]: best cost with the latest assigned onset index = m-1 (m = 0:
    // none yet). parent[j][m] = previous row's m; == m means token j free.
    let mut prev = vec![f32::INFINITY; k + 1];
    let mut parent = vec![vec![0usize; k + 1]; n];
    prev[0] = FREE_COST;
    for (m, slot) in prev.iter_mut().enumerate().skip(1) {
        *slot = cost(0, m - 1);
    }
    for (j, par) in parent.iter_mut().enumerate().skip(1) {
        let mut next = vec![f32::INFINITY; k + 1];
        // Prefix min of the previous row (best "latest onset < m").
        let mut pm = f32::INFINITY;
        let mut pm_at = 0usize;
        for m in 0..=k {
            // Free: keep the latest onset where it was.
            if prev[m] + FREE_COST < next[m] {
                next[m] = prev[m] + FREE_COST;
                par[m] = m;
            }
            if m >= 1 && pm.is_finite() {
                let c = pm + cost(j, m - 1);
                if c < next[m] {
                    next[m] = c;
                    par[m] = pm_at;
                }
            }
            if prev[m] < pm {
                pm = prev[m];
                pm_at = m;
            }
        }
        prev = next;
    }
    let mut m = (0..=k)
        .min_by(|&a, &b| {
            prev[a]
                .partial_cmp(&prev[b])
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);
    let mut assigned: Vec<Option<usize>> = vec![None; n];
    for j in (0..n).rev() {
        let p = if j == 0 { 0 } else { parent[j][m] };
        let free = if j == 0 { m == 0 } else { p == m };
        if !free {
            assigned[j] = Some(onsets[m - 1].0);
        }
        m = p;
    }
    // Interpolate the free runs by weight between anchors.
    let mut starts = vec![0usize; n];
    let mut j = 0;
    while j < n {
        if let Some(f) = assigned[j] {
            starts[j] = f;
            j += 1;
            continue;
        }
        let run_end = (j..n).find(|&x| assigned[x].is_some()).unwrap_or(n);
        let (ta, first_w) = if j == 0 {
            (lo as f32, 0.0)
        } else {
            (starts[j - 1] as f32, weights[j - 1])
        };
        let tb = if run_end < n {
            assigned[run_end].unwrap_or(hi) as f32
        } else {
            hi.max(lo + 1) as f32
        };
        let run_w: f32 = weights[j..run_end].iter().sum::<f32>() + first_w;
        let mut acc = first_w;
        for x in j..run_end {
            let u = if run_w > 0.0 { acc / run_w } else { 0.0 };
            starts[x] = (ta + (tb - ta).max(0.0) * u).round() as usize;
            acc += weights[x];
        }
        j = run_end;
    }
    for j in 1..n {
        if starts[j] <= starts[j - 1] {
            starts[j] = starts[j - 1] + 1;
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sung-note stand-in: a sine with 8ms raised-cosine edges. A
    /// hard-edged burst is a step, and a step's spectral splatter reads as
    /// a broadband "attack" at the note's END too — nothing a voice does.
    fn tone(sr: u32, start_ms: i64, end_ms: i64, hz: f32, out: &mut [f32]) {
        let sr_f = sr as f32;
        let lo = ((start_ms * sr as i64) / 1000) as usize;
        let hi = ((end_ms * sr as i64) / 1000).min(out.len() as i64) as usize;
        let ramp = (sr as usize * 8 / 1000).max(1);
        for (i, s) in out.iter_mut().enumerate().take(hi).skip(lo) {
            let t = i as f32 / sr_f;
            let edge = (i - lo).min(hi - 1 - i);
            let g = if edge < ramp {
                0.5 - 0.5 * (std::f32::consts::PI * edge as f32 / ramp as f32).cos()
            } else {
                1.0
            };
            *s = (t * hz * std::f32::consts::TAU).sin() * 0.4 * g;
        }
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
        for (w, want) in words.iter().zip([1000, 1300, 1600]) {
            assert!(
                (w.t - want).abs() <= 40,
                "{} at {}, want ~{want}",
                w.text,
                w.t
            );
        }
        assert!(
            words[2].end.unwrap() <= 1850,
            "last word must end with its burst: {:?}",
            words[2]
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
        assert!(words[0].t >= 980 && words[0].t <= 1120, "{words:?}");
        assert!(words[2].t <= 1920, "{words:?}");
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
        assert!((words[0].t - 1000).abs() <= 40, "{words:?}");
        assert!((words[1].t - 1600).abs() <= 40, "{words:?}");
    }

    #[test]
    fn extra_onsets_do_not_steal_words() {
        // Two words, four bursts: the prior keeps the words on the bursts
        // nearest their expected seats instead of the first two.
        let sr = 16_000u32;
        let mut pcm = vec![0.0f32; sr as usize * 3];
        tone(sr, 1000, 1150, 900.0, &mut pcm);
        tone(sr, 1200, 1350, 1100.0, &mut pcm);
        tone(sr, 1600, 1750, 1300.0, &mut pcm);
        tone(sr, 1800, 1950, 1500.0, &mut pcm);
        let lines = parse_lrc("[00:01.00]one two\n[00:02.50]next");
        let words = align(&pcm, sr, &lines, 0);
        assert_eq!(words.len(), 2, "{words:?}");
        assert!((words[0].t - 1000).abs() <= 40, "{words:?}");
        assert!((words[1].t - 1600).abs() <= 60, "{words:?}");
    }

    #[test]
    fn global_offset_rebases_onto_the_stamps() {
        // The recording's origin is 400ms early (an Apple Music floored
        // position): every burst sits 400ms after its stamp in recording
        // time. Words must still come out on the stamps.
        let sr = 16_000u32;
        let mut pcm = vec![0.0f32; sr as usize * 12];
        let mut lrc = String::new();
        for k in 0..6 {
            let stamp = 1000 + k * 1500;
            tone(
                sr,
                stamp + 400,
                stamp + 600,
                800.0 + k as f32 * 100.0,
                &mut pcm,
            );
            lrc.push_str(&format!(
                "[00:{:02}.{:02}]word\n",
                stamp / 1000,
                (stamp % 1000) / 10
            ));
        }
        let lines = parse_lrc(&lrc);
        let words = align(&pcm, sr, &lines, 0);
        assert_eq!(words.len(), 6, "{words:?}");
        for (w, line) in words.iter().zip(&lines) {
            assert!((w.t - line.t).abs() <= 40, "{w:?} vs stamp {}", line.t);
        }
    }

    #[test]
    fn parse_lrc_skips_empty_markers() {
        let lines = parse_lrc("[00:01.00]verse\n[00:04.00] \n[00:08.00]next");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "verse");
        assert_eq!(lines[1].t, 8000);
    }
}
