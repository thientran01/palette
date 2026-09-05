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

/// Sample-index → track-position map for one recording: a least-squares
/// line through the anchors the media loop's (position_ms, position_at_ms)
/// pairs give while recording (`fit`), or a single projected origin at the
/// nominal rate when there aren't enough (`from_origin`). Never UI-visible —
/// the frontend clock owns display position; this maps captured samples
/// onto the player's own timeline for alignment (the same carve-out as
/// history's ms_listened and upnext's feed estimate).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TimeMap {
    pub intercept_ms: f64,
    /// ms per output sample.
    pub slope_ms: f64,
    pub nominal_slope_ms: f64,
    /// The fitted slope left the clamp and was replaced by nominal.
    pub clamped: bool,
    pub residual_rms_ms: f64,
    pub n_anchors: usize,
    /// Jitter outliers dropped before the final fit.
    pub dropped: usize,
    pub from_origin: bool,
}

/// Shared-mode WASAPI shares the player's engine clock, so real drift is
/// ~0: a slope this far off nominal means bad anchors, not a fast clock.
const SLOPE_CLAMP: f64 = 0.005;
/// Anchor residual treated as jitter (Apple Music floors positions to
/// whole seconds): dropped, the line refit once.
pub const OUTLIER_MS: f64 = 1200.0;
/// Beyond this a pair is a seek, not jitter — past Apple Music's 2s band.
/// karaoke.rs aborts the recording on it.
pub const SEEK_RESIDUAL_MS: f64 = 2500.0;

impl TimeMap {
    pub fn from_origin(origin_ms: i64, sample_rate: u32) -> Self {
        let nominal = 1000.0 / sample_rate.max(1) as f64;
        Self {
            intercept_ms: origin_ms as f64,
            slope_ms: nominal,
            nominal_slope_ms: nominal,
            clamped: false,
            residual_rms_ms: 0.0,
            n_anchors: 0,
            dropped: 0,
            from_origin: true,
        }
    }

    /// `anchors` = (sample_index, position_ms). Fewer than two degrade to
    /// `from_origin`.
    pub fn fit(anchors: &[(usize, i64)], sample_rate: u32, origin_ms: i64) -> Self {
        let nominal = 1000.0 / sample_rate.max(1) as f64;
        if anchors.len() < 2 {
            return Self::from_origin(origin_ms, sample_rate);
        }
        let mut pts: Vec<(f64, f64)> = anchors.iter().map(|&(i, p)| (i as f64, p as f64)).collect();
        let mut map = Self::solve(&pts, nominal);
        let keep: Vec<(f64, f64)> = pts
            .iter()
            .copied()
            .filter(|&(x, y)| (map.predict(x) - y).abs() <= OUTLIER_MS)
            .collect();
        let dropped = pts.len() - keep.len();
        if dropped > 0 && keep.len() >= 2 {
            pts = keep;
            map = Self::solve(&pts, nominal);
            map.dropped = dropped;
        }
        map
    }

    fn solve(pts: &[(f64, f64)], nominal: f64) -> Self {
        let n = pts.len() as f64;
        let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
        let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
        let sxx: f64 = pts.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
        let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
        let mut slope = if sxx > 0.0 { sxy / sxx } else { nominal };
        let mut clamped = false;
        if ((slope - nominal) / nominal).abs() > SLOPE_CLAMP {
            slope = nominal;
            clamped = true;
        }
        let intercept = my - slope * mx;
        let rss: f64 = pts
            .iter()
            .map(|p| {
                let r = intercept + slope * p.0 - p.1;
                r * r
            })
            .sum();
        Self {
            intercept_ms: intercept,
            slope_ms: slope,
            nominal_slope_ms: nominal,
            clamped,
            residual_rms_ms: (rss / n).sqrt(),
            n_anchors: pts.len(),
            dropped: 0,
            from_origin: false,
        }
    }

    fn predict(&self, idx: f64) -> f64 {
        self.intercept_ms + self.slope_ms * idx
    }

    pub fn pos_ms(&self, idx: usize) -> i64 {
        self.predict(idx as f64).round() as i64
    }

    pub fn sample_at(&self, ms: i64) -> usize {
        ((ms as f64 - self.intercept_ms) / self.slope_ms)
            .round()
            .max(0.0) as usize
    }

    /// Signed prediction error for a fresh pair: predicted − reported.
    pub fn residual_ms(&self, idx: usize, position_ms: i64) -> f64 {
        self.predict(idx as f64) - position_ms as f64
    }
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

/// Which refinement rungs run on top of the prior. Every rung is
/// switchable so the offline scorer can print what each one buys; the
/// shipped set is the highest rung that won on BOTH truth songs
/// (docs/specs/2026-09-04-karaoke-aligner-ladder.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartDetector {
    /// First rise of the HP-RMS envelope above the voice floor.
    EnergyRise,
    /// Strongest spectral-flux peak inside the start window.
    Flux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stages {
    pub start: Option<StartDetector>,
    pub end: bool,
}

impl Stages {
    pub const PRIOR_ONLY: Stages = Stages {
        start: None,
        end: false,
    };

    /// The set the app runs. Measured on Blur + a second song via
    /// `karaoke_score matrix`; update this AND the spec together.
    pub fn shipped() -> Stages {
        Stages::PRIOR_ONLY
    }

    /// Every rung the scorer reports, in ladder order.
    pub fn ladder() -> [(&'static str, Stages); 5] {
        [
            ("prior", Stages::PRIOR_ONLY),
            (
                "prior+energy",
                Stages {
                    start: Some(StartDetector::EnergyRise),
                    end: false,
                },
            ),
            (
                "prior+flux",
                Stages {
                    start: Some(StartDetector::Flux),
                    end: false,
                },
            ),
            (
                "prior+energy+end",
                Stages {
                    start: Some(StartDetector::EnergyRise),
                    end: true,
                },
            ),
            (
                "prior+flux+end",
                Stages {
                    start: Some(StartDetector::Flux),
                    end: true,
                },
            ),
        ]
    }
}

// ── Stage 0: the prior. Fitted on the Blur truth set (2026-09-04): the
// first sung word sits a median 330ms after its stamp, the sung span
// fills ~74% of the stamp window, and a flat syllable-weighted spread
// inside the true span scores 44ms — the audio envelope inside a line
// scored 127ms, so nothing here consults audio between the endpoints.
const LEAD_MS: i64 = 330;
const RATE_SYL_S: f32 = 3.1;
const FILL: f32 = 0.9;
/// The last word holds this long past its start before the row's end.
const LAST_HOLD_MS: i64 = 250;
/// A final line has no next stamp; give it a plausible window.
const LAST_LINE_WINDOW_MS: i64 = 8_000;
// ── Stage 1: start refinement window around the stamp (truth: p10 −71,
// p90 +536) and the flux strength a peak needs to be believed.
const START_PRE_MS: i64 = 300;
const START_POST_MS: i64 = 900;
const START_MIN_STRENGTH: f32 = 0.35;
// ── Stage 2: end refinement looks this fraction of the span either side
// of the prior end for a decay into quiet.
const END_SEARCH_FRAC: f32 = 0.4;
const END_QUIET_FRAMES: usize = 3;

pub fn align(pcm: &[f32], sample_rate: u32, lines: &[TimedLine], map: &TimeMap) -> Vec<Word> {
    align_with(pcm, sample_rate, lines, map, &Stages::shipped())
}

pub fn align_with(
    pcm: &[f32],
    sample_rate: u32,
    lines: &[TimedLine],
    map: &TimeMap,
    stages: &Stages,
) -> Vec<Word> {
    if sample_rate == 0 || pcm.is_empty() || lines.is_empty() {
        return Vec::new();
    }
    let pcm_end_ms = map.pos_ms(pcm.len());
    let flux = if stages.start == Some(StartDetector::Flux) {
        flux_features(pcm, sample_rate)
    } else {
        None
    };
    let ctx = Ctx {
        pcm,
        sample_rate,
        map,
        pcm_end_ms,
        flux: flux.as_ref(),
    };
    let mut words = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let next_t = lines
            .get(i + 1)
            .map(|n| n.t)
            .unwrap_or((line.t + LAST_LINE_WINDOW_MS).min(pcm_end_ms));
        if let Some(ws) = align_line(&ctx, line, next_t, stages) {
            words.extend(ws);
        }
    }
    words
}

struct Ctx<'a> {
    pcm: &'a [f32],
    sample_rate: u32,
    map: &'a TimeMap,
    pcm_end_ms: i64,
    flux: Option<&'a Flux>,
}

/// Where a line's words go: `start` of the first token and the `span` from
/// it to the LAST token's start.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LinePrior {
    start: i64,
    span: i64,
}

fn prior(line_t: i64, next_t: i64, weights: &[f32]) -> LinePrior {
    let start = line_t + LEAD_MS;
    LinePrior {
        start,
        span: span_for(start, line_t, next_t, weights),
    }
}

/// Syllable-rate span, capped so the line ends by FILL of its window
/// whatever the start moved to.
fn span_for(start: i64, line_t: i64, next_t: i64, weights: &[f32]) -> i64 {
    let s = placed_weight(weights);
    let window = (next_t - line_t).max(0) as f32;
    let by_rate = (s / RATE_SYL_S * 1000.0) as i64;
    let by_window = (FILL * window) as i64 - (start - line_t);
    by_rate.min(by_window).max(MIN_SPAN_MS)
}

/// Total weight ahead of the last token — what the span is spread over.
fn placed_weight(weights: &[f32]) -> f32 {
    let total: f32 = weights.iter().sum();
    (total - weights.last().copied().unwrap_or(0.0)).max(0.0)
}

fn spread(tokens: &[String], weights: &[f32], p: LinePrior, next_t: i64) -> Vec<Word> {
    let s = placed_weight(weights);
    let n = tokens.len();
    let mut starts: Vec<i64> = Vec::with_capacity(n);
    let mut acc = 0.0f32;
    for (i, w) in weights.iter().enumerate() {
        let u = if s > 0.0 && i > 0 { acc / s } else { 0.0 };
        let mut t = p.start + (p.span as f32 * u).round() as i64;
        if let Some(&prev) = starts.last() {
            t = t.max(prev + HOP_MS);
        }
        starts.push(t);
        acc += *w;
    }
    let last_end = (p.start + p.span + LAST_HOLD_MS)
        .min(next_t)
        .max(starts[n - 1] + HOP_MS);
    tokens
        .iter()
        .enumerate()
        .map(|(i, tok)| Word {
            t: starts[i],
            text: tok.clone(),
            end: Some(if i + 1 < n { starts[i + 1] } else { last_end }),
        })
        .collect()
}

fn align_line(ctx: &Ctx, line: &TimedLine, next_t: i64, stages: &Stages) -> Option<Vec<Word>> {
    let tokens = tokenize(&line.text);
    if tokens.is_empty() || next_t <= line.t {
        return None;
    }
    let weights: Vec<f32> = tokens.iter().map(|t| token_weight(t)).collect();
    let mut p = prior(line.t, next_t, &weights);
    if let Some(det) = stages.start {
        if let Some(start) = detect_start(ctx, det, line.t, next_t) {
            p.start = start;
            p.span = span_for(start, line.t, next_t, &weights);
        }
    }
    if stages.end {
        if let Some(end) = detect_end(ctx, p, next_t) {
            p.span = (end - p.start).max(MIN_SPAN_MS);
        }
    }
    Some(spread(&tokens, &weights, p, next_t))
}

// ── Stage 1 ──

fn detect_start(ctx: &Ctx, det: StartDetector, line_t: i64, next_t: i64) -> Option<i64> {
    let lo = (line_t - START_PRE_MS).max(ctx.map.pos_ms(0));
    let hi = (line_t + START_POST_MS).min(next_t).min(ctx.pcm_end_ms);
    if hi - lo < MIN_SPAN_MS {
        return None;
    }
    match det {
        StartDetector::EnergyRise => {
            let start = ctx.map.sample_at(lo);
            let end = ctx.map.sample_at(hi);
            let env = envelope(ctx.pcm, ctx.sample_rate, start, end)?;
            let peak = env.values.iter().copied().fold(0.0f32, f32::max);
            if peak < PEAK_FLOOR {
                return None;
            }
            let floor = peak * VOICE_REL;
            // A rise: below the floor, then at or above it. Loud from the
            // first frame says nothing about where the voice began.
            let f = (1..env.values.len())
                .find(|&f| env.values[f] >= floor && env.values[f - 1] < floor)?;
            Some(ctx.map.pos_ms(start + f * env.hop))
        }
        StartDetector::Flux => {
            let flux = ctx.flux?;
            let fa = flux.frame_at(ctx.map.sample_at(lo));
            let fb = flux.frame_at(ctx.map.sample_at(hi)).min(flux.norm.len());
            if fb <= fa + 2 {
                return None;
            }
            let mut best: Option<(usize, f32)> = None;
            for f in fa..fb {
                let v = flux.norm[f];
                if v < START_MIN_STRENGTH {
                    continue;
                }
                let l = f.saturating_sub(2);
                let r = (f + 2).min(flux.norm.len() - 1);
                if flux.norm[l..=r].iter().any(|&x| x > v) {
                    continue;
                }
                if best.map(|(_, b)| v > b).unwrap_or(true) {
                    best = Some((f, v));
                }
            }
            best.map(|(f, _)| ctx.map.pos_ms(f * flux.hop))
        }
    }
}

// ── Stage 2 ──

fn detect_end(ctx: &Ctx, p: LinePrior, next_t: i64) -> Option<i64> {
    let e = p.start + p.span;
    let reach = (p.span as f32 * END_SEARCH_FRAC) as i64;
    let lo = (e - reach).max(p.start);
    let hi = (e + reach).min(next_t).min(ctx.pcm_end_ms);
    if hi <= lo {
        return None;
    }
    let start = ctx.map.sample_at(p.start);
    let end = ctx.map.sample_at(hi);
    let env = envelope(ctx.pcm, ctx.sample_rate, start, end)?;
    let peak = env.values.iter().copied().fold(0.0f32, f32::max);
    if peak < PEAK_FLOOR {
        return None;
    }
    let floor = peak * VOICE_REL;
    let from = ((ctx.map.sample_at(lo).saturating_sub(start)) / env.hop).min(env.values.len());
    let mut quiet = 0usize;
    for f in from..env.values.len() {
        if env.values[f] < floor {
            quiet += 1;
            if quiet >= END_QUIET_FRAMES {
                return Some(ctx.map.pos_ms(start + (f + 1 - quiet) * env.hop));
            }
        } else {
            quiet = 0;
        }
    }
    None
}

// ── Envelope (Stage 1 EnergyRise, Stage 2) ──

struct Env {
    /// Hop in samples — frame `f` starts at `start + f * hop`.
    hop: usize,
    values: Vec<f32>,
}

fn envelope(pcm: &[f32], sample_rate: u32, start: usize, end: usize) -> Option<Env> {
    let sr = sample_rate as i64;
    let hop = (sr * HOP_MS / 1000).max(1) as usize;
    let win = hop * 2;
    let end = end.min(pcm.len());
    if end <= start + win {
        return None;
    }
    let mut values = Vec::new();
    let mut prev = 0.0f32;
    let mut i = start;
    while i + win <= end {
        let mut sum = 0.0f32;
        for &x in &pcm[i..i + win] {
            let hp = x - prev;
            prev = x;
            sum += hp * hp;
        }
        values.push((sum / win as f32).sqrt());
        i += hop;
    }
    if values.is_empty() {
        return None;
    }
    Some(Env { hop, values })
}

// ── Spectral flux (Stage 1 Flux): SuperFlux-style onset strength over the
// whole recording, normalized to the local max. Only ever consulted inside
// a line's start window — over the whole line it latched onto drums
// (2026-09-04, reverted).

struct Flux {
    hop: usize,
    norm: Vec<f32>,
}

impl Flux {
    fn frame_at(&self, sample: usize) -> usize {
        sample / self.hop
    }
}

const FLUX_BAND_LO_HZ: f32 = 150.0;
const FLUX_BAND_HI_HZ: f32 = 5000.0;
const FLUX_LOG_GAIN: f32 = 100.0;
const FLUX_MAG_FLOOR: f32 = 0.002;
const FLUX_LAG: usize = 2;
const FLUX_NORM_BLOCK: usize = 150;

fn flux_features(pcm: &[f32], sample_rate: u32) -> Option<Flux> {
    use rustfft::{num_complex::Complex, FftPlanner};
    let hop = (sample_rate as usize * HOP_MS as usize / 1000).max(1);
    let frame = (hop * 3).next_power_of_two().max(64);
    if pcm.len() < frame {
        return None;
    }
    let n_frames = (pcm.len() - frame) / hop + 1;
    let bin_hz = sample_rate as f32 / frame as f32;
    let lo = ((FLUX_BAND_LO_HZ / bin_hz).round() as usize).max(1);
    let hi = ((FLUX_BAND_HI_HZ / bin_hz).round() as usize).min(frame / 2 - 1);
    if hi < lo + 3 {
        return None;
    }
    let nb = hi - lo + 1;
    let window: Vec<f32> = (0..frame)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / frame as f32).cos())
        .collect();
    let mag_scale = 4.0 / frame as f32;
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(frame);
    let mut buf = vec![Complex::new(0.0f32, 0.0); frame];
    let mut scratch = vec![Complex::new(0.0f32, 0.0); fft.get_inplace_scratch_len()];
    let mut ring: Vec<Vec<f32>> = (0..=FLUX_LAG).map(|_| vec![0.0f32; nb]).collect();
    let mut cur = vec![0.0f32; nb];
    let mut osf = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let start = i * hop;
        for (k, slot) in buf.iter_mut().enumerate() {
            *slot = Complex::new(pcm[start + k] * window[k], 0.0);
        }
        fft.process_with_scratch(&mut buf, &mut scratch);
        for (b, k) in (lo..=hi).enumerate() {
            let m = buf[k].norm() * mag_scale;
            cur[b] = (1.0 + FLUX_LOG_GAIN * m.max(FLUX_MAG_FLOOR)).ln();
        }
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
                sum += (cur[b] - prev[b].max(p_lo).max(p_hi)).max(0.0);
            }
            sum
        } else {
            0.0
        };
        osf.push(flux);
        std::mem::swap(&mut ring[i % (FLUX_LAG + 1)], &mut cur);
    }
    let n_blocks = n_frames.div_ceil(FLUX_NORM_BLOCK);
    let mut block_max = vec![0.0f32; n_blocks];
    for (i, &v) in osf.iter().enumerate() {
        let b = i / FLUX_NORM_BLOCK;
        block_max[b] = block_max[b].max(v);
    }
    let norm = osf
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let b = i / FLUX_NORM_BLOCK;
            let mut m = block_max[b];
            if b > 0 {
                m = m.max(block_max[b - 1]);
            }
            if b + 1 < n_blocks {
                m = m.max(block_max[b + 1]);
            }
            if m > 1e-6 {
                v / m
            } else {
                0.0
            }
        })
        .collect();
    Some(Flux { hop, norm })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sung-note stand-in: a sine with 8ms raised-cosine edges.
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

    const SR: u32 = 16_000;

    fn run(pcm: &[f32], lrc: &str, stages: Stages) -> Vec<Word> {
        align_with(
            pcm,
            SR,
            &parse_lrc(lrc),
            &TimeMap::from_origin(0, SR),
            &stages,
        )
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
    fn parse_lrc_skips_empty_markers() {
        let lines = parse_lrc("[00:01.00]verse\n[00:04.00] \n[00:08.00]next");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "verse");
        assert_eq!(lines[1].t, 8000);
    }

    #[test]
    fn prior_leads_the_stamp_and_spreads_by_weight() {
        // Equal-weight syllables: the span splits evenly, the last token
        // sits at start + span, ends chain to the next start.
        let pcm = vec![0.0f32; SR as usize * 6];
        let words = run(
            &pcm,
            "[00:01.00]가 나 다\n[00:05.00]next",
            Stages::PRIOR_ONLY,
        );
        assert_eq!(words.len(), 4, "{words:?}");
        let start = 1000 + LEAD_MS;
        assert_eq!(words[0].t, start);
        // S = 2 syllables → span = 2/3.1 s ≈ 645ms
        let span = words[2].t - words[0].t;
        assert!((span - 645).abs() <= 10, "{words:?}");
        assert!((words[1].t - words[0].t - span / 2).abs() <= 1, "{words:?}");
        assert_eq!(words[0].end, Some(words[1].t));
        assert_eq!(words[2].end, Some(words[2].t + LAST_HOLD_MS));
    }

    #[test]
    fn prior_span_is_capped_by_the_window() {
        // 30 syllables want ~9.4s; a 2s window allows 0.9×2000 − 330.
        let pcm = vec![0.0f32; SR as usize * 5];
        let line = "가".repeat(30);
        let words = run(
            &pcm,
            &format!("[00:01.00]{line}\n[00:03.00]next"),
            Stages::PRIOR_ONLY,
        );
        assert_eq!(words.len(), 31);
        let span = words[29].t - words[0].t;
        assert!((span - (1800 - 330)).abs() <= 30, "span {span}");
        assert!(words[29].end.unwrap() <= 3000, "{:?}", words[29]);
    }

    #[test]
    fn prior_never_spreads_into_a_gap() {
        // The 2026-09-04 worst class: a 20s gap after a short line.
        let pcm = vec![0.0f32; SR as usize * 25];
        let words = run(
            &pcm,
            "[00:01.00]one two three\n[00:21.00]next",
            Stages::PRIOR_ONLY,
        );
        assert!(words[2].t < 2500, "{words:?}");
    }

    #[test]
    fn silence_still_gets_the_prior() {
        // karaoke.rs aborts silent recordings before align; align itself
        // always has a prior to fall back on.
        let pcm = vec![0.0f32; SR as usize * 3];
        let words = run(
            &pcm,
            "[00:01.00]hello world\n[00:02.00]next",
            Stages::shipped(),
        );
        // Both lines get prior words — the final line too, on its
        // LAST_LINE_WINDOW_MS window.
        assert_eq!(words.len(), 3, "{words:?}");
        assert_eq!(words[0].t, 1000 + LEAD_MS);
    }

    fn start_of(det: StartDetector, burst_at: i64) -> i64 {
        let mut pcm = vec![0.0f32; SR as usize * 5];
        tone(SR, burst_at, burst_at + 400, 1000.0, &mut pcm);
        let stages = Stages {
            start: Some(det),
            end: false,
        };
        run(&pcm, "[00:01.00]one two\n[00:04.00]next", stages)[0].t
    }

    #[test]
    fn start_refinement_snaps_to_a_burst_in_range() {
        for det in [StartDetector::EnergyRise, StartDetector::Flux] {
            let t = start_of(det, 1500);
            assert!((t - 1500).abs() <= 40, "{det:?}: {t}");
        }
    }

    #[test]
    fn start_refinement_ignores_a_burst_out_of_range() {
        for det in [StartDetector::EnergyRise, StartDetector::Flux] {
            let t = start_of(det, 2500);
            assert_eq!(t, 1000 + LEAD_MS, "{det:?}");
        }
    }

    #[test]
    fn start_refinement_keeps_the_prior_in_silence() {
        for det in [StartDetector::EnergyRise, StartDetector::Flux] {
            let pcm = vec![0.0f32; SR as usize * 5];
            let stages = Stages {
                start: Some(det),
                end: false,
            };
            let words = run(&pcm, "[00:01.00]one two\n[00:04.00]next", stages);
            assert_eq!(words[0].t, 1000 + LEAD_MS, "{det:?}");
        }
    }

    #[test]
    fn end_refinement_pulls_the_span_in_at_the_decay() {
        // Ten syllables: prior span 9/3.1 ≈ 2.9s; the voice actually stops
        // at 60% of that.
        let mut pcm = vec![0.0f32; SR as usize * 8];
        let start = 1000 + LEAD_MS;
        let prior_span = (9.0 / RATE_SYL_S * 1000.0) as i64;
        let stop = start + prior_span * 6 / 10;
        tone(SR, start, stop, 900.0, &mut pcm);
        let line = "가".repeat(10);
        let lrc = format!("[00:01.00]{line}\n[00:06.00]next");
        let with = run(
            &pcm,
            &lrc,
            Stages {
                start: None,
                end: true,
            },
        );
        let without = run(&pcm, &lrc, Stages::PRIOR_ONLY);
        let span_with = with[9].t - with[0].t;
        let span_without = without[9].t - without[0].t;
        assert!(span_with < span_without, "{span_with} vs {span_without}");
        assert!((with[9].t - stop).abs() <= 60, "{:?} stop {stop}", with[9]);
    }

    /// Anchors every 5s of a 16kHz recording whose true origin is 700ms,
    /// with the given per-anchor noise.
    fn anchors(noise: impl Fn(usize) -> i64) -> Vec<(usize, i64)> {
        (0..12)
            .map(|k| {
                let idx = k * 80_000;
                let truth = 700 + idx as i64 / 16;
                (idx, truth + noise(k))
            })
            .collect()
    }

    #[test]
    fn fit_recovers_offset_and_rate_through_jitter() {
        let map = TimeMap::fit(&anchors(|k| [17, -20, 5, -12][k % 4]), 16_000, 0);
        assert!(!map.from_origin);
        assert!((map.intercept_ms - 700.0).abs() < 25.0, "{map:?}");
        assert!((map.slope_ms - 1.0 / 16.0).abs() < 1e-4, "{map:?}");
        assert!((map.pos_ms(160_000) - 10_700).abs() <= 25, "{map:?}");
        assert!(map.residual_rms_ms < 30.0, "{map:?}");
    }

    #[test]
    fn floored_positions_average_out() {
        let map = TimeMap::fit(&anchors(|k| -((k as i64 * 337) % 1000)), 16_000, 0);
        let bias = map.intercept_ms - 700.0;
        assert!(bias > -800.0 && bias < -200.0, "{map:?}");
        assert!(!map.clamped, "{map:?}");
    }

    #[test]
    fn wild_slope_falls_back_to_nominal() {
        let bad: Vec<(usize, i64)> = (0..6)
            .map(|k| (k * 80_000, 700 + (k as i64 * 5000 * 103) / 100))
            .collect();
        let map = TimeMap::fit(&bad, 16_000, 0);
        assert!(map.clamped, "{map:?}");
        assert_eq!(map.slope_ms, map.nominal_slope_ms);
    }

    #[test]
    fn single_anchor_is_the_origin() {
        let map = TimeMap::fit(&[(0, 1234)], 16_000, 5555);
        assert!(map.from_origin);
        assert_eq!(map.pos_ms(0), 5555);
        assert_eq!(map.sample_at(6555), 16_000);
    }

    #[test]
    fn one_jitter_outlier_is_dropped() {
        let map = TimeMap::fit(&anchors(|k| if k == 5 { 1500 } else { 0 }), 16_000, 0);
        assert_eq!(map.dropped, 1, "{map:?}");
        assert!((map.intercept_ms - 700.0).abs() < 5.0, "{map:?}");
    }
}
