//! Audio-reactive core: WASAPI loopback capture → FFT → smoothed band
//! energies emitted as "audio-bands" events (~30Hz).
//!
//! Capture is PROCESS-SCOPED when it can be (loopback.rs: the playing app's
//! process tree via the process-loopback virtual device) so the bars ride
//! the SONG — device-wide loopback heard Discord voice and game SFX too,
//! and the auto-gain danced to whoever was loudest. The device-wide cpal
//! stream survives as the fallback when the AUMID→PID join misses (unknown
//! player, pre-2004 Windows), with a periodic upgrade retry.
//!
//! Lifecycle discipline (plan M4): capture runs ONLY while a Pulse window
//! is visible (the main widget OR the focus takeover — lib.rs widens the
//! gate) AND something is playing. The owner thread opens/drops the capture
//! on demand — dropping releases the device/stream entirely, so a hidden or
//! paused app costs zero audio work.
//!
//! Emits target the main and focus windows only. Search and prefs never
//! render the waveform. A silence latch throttles the FFT and stops emits once
//! smoothed output has sat below ZERO_EPS for LATCH_AFTER_TICKS (~396ms):
//! one terminating zero payload (the same `Bands::default()` the switch-off
//! arm already sends), then cheap raw-RMS peeks with an FFT recheck every
//! ~264ms. Quiet intros can resume below the raw fast-wake threshold.
//! Process-path staleness still zeros the ring at 250ms so the bars fall;
//! the latch arms on that fallen envelope and does not replace the zeroing.

use crate::loopback;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const FFT_SIZE: usize = 2048;
const EMIT_INTERVAL: Duration = Duration::from_millis(33);
/// Band edges in Hz: bass, mid, high.
const BANDS: [(f32, f32); 3] = [(30.0, 150.0), (150.0, 2000.0), (2000.0, 8000.0)];
/// Log-spaced fine bins for the now-playing waveform separator's bars.
const SPECTRUM_BINS: usize = 16;
const SPECTRUM_LO_HZ: f32 = 40.0;
const SPECTRUM_HI_HZ: f32 = 8000.0;
const ATTACK: f32 = 0.55;
/// Release fast enough that a bar has visibly fallen before the next beat
/// lands (~0.3 → ~2.5 ticks to halve ≈ 80ms) — 0.12 blurred adjacent kicks
/// into one sway.
const RELEASE: f32 = 0.3;
/// Auto-gain reference decays slowly so quiet and loud tracks both animate.
const GAIN_DECAY: f32 = 0.995;
/// Broadband-RMS reference for the dynamics factor decays slower than the
/// per-bin gain (~11s half-life vs ~5s) so a quiet bridge stays visibly
/// quiet instead of re-gaining to full height mid-section.
const RMS_DECAY: f32 = 0.998;
/// How short a silent-adjacent passage can render: the dynamics factor
/// scales bar targets into [DYN_FLOOR, 1] so quiet sections still animate,
/// just visibly smaller.
const DYN_FLOOR: f32 = 0.35;
/// The dynamics factor tracks SECTIONS (verse vs drop), so its release is
/// deliberately much slower than the per-bin RELEASE (~760ms half-life vs
/// ~80ms): broadband RMS dips in the gaps between beats, and a fast release
/// here would duck every bar in lockstep — the pump the staggered bins
/// exist to avoid. Attack reuses ATTACK so a drop lands immediately.
const DYN_RELEASE: f32 = 0.03;
/// Consecutive quiet ticks before Live → Latched. 12 × 33ms ≈ 396ms,
/// inside the 250–500ms arm window: long enough that process-path
/// staleness (250ms zero) plus envelope release can fall, short enough
/// that a remote-device silence does not keep FFTing for a second.
const LATCH_AFTER_TICKS: u8 = 12;
/// Visual silence on the 0–1 smoothed payload. Sits between Waveform's
/// IDLE_EPS (0.004) and WAKE_LEVEL (0.02), so the latch arms after the
/// bars have already gone visually dead, not mid-fall.
const ZERO_EPS: f32 = 0.01;
/// Raw sample-RMS fast wake. Different unit from the auto-gained ZERO_EPS;
/// quiet music can fall below this, so it cannot be the only resume path.
const RAW_WAKE: f32 = 1e-3;
/// Bound the delay for music below RAW_WAKE without running the FFT at 30Hz
/// through sustained silence. Eight owner ticks are ~264ms.
const SOFT_WAKE_PROBE_TICKS: u8 = 8;

#[derive(Serialize, Clone, Copy, Default)]
pub struct Bands {
    pub bass: f32,
    pub mid: f32,
    pub high: f32,
    /// Overall level 0..1 (auto-gained RMS).
    pub level: f32,
    /// Log-spaced bins bass→high, each auto-gained/smoothed like the bands.
    pub spectrum: [f32; SPECTRUM_BINS],
}

/// Log-spaced bin edges: edge(i) = LO * (HI/LO)^(i/N).
fn spectrum_edges() -> [f32; SPECTRUM_BINS + 1] {
    let ratio = SPECTRUM_HI_HZ / SPECTRUM_LO_HZ;
    let mut edges = [0.0f32; SPECTRUM_BINS + 1];
    for (i, e) in edges.iter_mut().enumerate() {
        *e = SPECTRUM_LO_HZ * ratio.powf(i as f32 / SPECTRUM_BINS as f32);
    }
    edges
}

/// Latest samples ring (mono-mixed), written by whichever capture path is
/// live (the cpal callback or loopback.rs's process capture thread).
pub(crate) struct Ring {
    buf: Vec<f32>,
    pos: usize,
}

impl Ring {
    pub(crate) fn new() -> Self {
        Ring {
            buf: vec![0.0; FFT_SIZE],
            pos: 0,
        }
    }
    pub(crate) fn push_frame(&mut self, frame_mean: f32) {
        // Sanitize at the one ingest point both capture paths share: a single
        // non-finite sample (a driver/format edge case can emit NaN/Inf)
        // propagates through the FFT into smoothed[] and STICKS there — serde
        // renders a NaN band as `null` and the bars freeze until the next pause
        // resets the envelope.
        self.buf[self.pos] = if frame_mean.is_finite() {
            frame_mean
        } else {
            0.0
        };
        self.pos = (self.pos + 1) % self.buf.len();
    }
    /// Snapshot in chronological order into `out`. `out` must be `buf.len()`.
    pub(crate) fn snapshot_into(&self, out: &mut [f32]) {
        let n = self.buf.len();
        debug_assert_eq!(out.len(), n);
        let tail = n - self.pos;
        out[..tail].copy_from_slice(&self.buf[self.pos..]);
        out[tail..].copy_from_slice(&self.buf[..self.pos]);
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> Vec<f32> {
        let mut out = vec![0.0; self.buf.len()];
        self.snapshot_into(&mut out);
        out
    }

    fn rms(&self) -> f32 {
        let n = self.buf.len().max(1) as f32;
        (self.buf.iter().map(|s| s * s).sum::<f32>() / n).sqrt()
    }
}

/// Owner-loop silence latch. Live FFTs and emits; Latched mostly peeks raw
/// RMS, periodically checking the actual visual output for quiet music.
/// Smoothed output below ZERO_EPS for LATCH_AFTER_TICKS arms it; raw RMS
/// above RAW_WAKE or a non-quiet FFT probe wakes it. Silent probes emit nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SilenceLatch {
    Live { quiet_ticks: u8 },
    Latched { probe_ticks: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LatchEmit {
    Bands,
    Zero,
    None,
}

impl SilenceLatch {
    const fn new() -> Self {
        Self::Live { quiet_ticks: 0 }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn is_latched(&self) -> bool {
        matches!(self, Self::Latched { .. })
    }

    /// Latched peek. `true` means fall through and run the FFT this tick.
    fn consider_wake(&mut self, raw_awake: bool) -> bool {
        match self {
            Self::Latched { .. } if raw_awake => {
                *self = Self::Live { quiet_ticks: 0 };
                true
            }
            Self::Latched { probe_ticks } => {
                *probe_ticks += 1;
                if *probe_ticks >= SOFT_WAKE_PROBE_TICKS {
                    *probe_ticks = 0;
                    true // Probe while still latched; after_fft decides whether to wake.
                } else {
                    false
                }
            }
            Self::Live { .. } => true,
        }
    }

    fn after_fft(&mut self, quiet: bool) -> LatchEmit {
        match *self {
            Self::Latched { .. } if quiet => LatchEmit::None,
            Self::Latched { .. } => {
                *self = Self::new();
                LatchEmit::Bands
            }
            Self::Live { quiet_ticks } => {
                if quiet {
                    let n = quiet_ticks.saturating_add(1);
                    if n >= LATCH_AFTER_TICKS {
                        *self = Self::Latched { probe_ticks: 0 };
                        LatchEmit::Zero
                    } else {
                        *self = Self::Live { quiet_ticks: n };
                        LatchEmit::Bands
                    }
                } else {
                    *self = Self::Live { quiet_ticks: 0 };
                    LatchEmit::Bands
                }
            }
        }
    }
}

fn bands_quiet(b: &Bands) -> bool {
    b.bass <= ZERO_EPS
        && b.mid <= ZERO_EPS
        && b.high <= ZERO_EPS
        && b.level <= ZERO_EPS
        && b.spectrum.iter().all(|&s| s <= ZERO_EPS)
}

/// The live normalization and silence state, shared with the regression
/// tests so a quiet-resume check cannot bypass retained peaks or smoothing.
struct BandEnvelope {
    smoothed: [f32; 3],
    gain_ref: [f32; 3],
    spec_edges: [f32; SPECTRUM_BINS + 1],
    smoothed_spec: [f32; SPECTRUM_BINS],
    gain_spec: [f32; SPECTRUM_BINS],
    rms_ref: f32,
    smoothed_dyn: f32,
    latch: SilenceLatch,
}

impl BandEnvelope {
    fn new() -> Self {
        Self {
            smoothed: [0.0; 3],
            gain_ref: [1e-4; 3],
            spec_edges: spectrum_edges(),
            smoothed_spec: [0.0; SPECTRUM_BINS],
            gain_spec: [1e-4; SPECTRUM_BINS],
            rms_ref: 1e-4,
            smoothed_dyn: 0.0,
            latch: SilenceLatch::new(),
        }
    }

    fn reset_capture(&mut self) {
        self.smoothed = [0.0; 3];
        self.smoothed_spec = [0.0; SPECTRUM_BINS];
        self.smoothed_dyn = 0.0;
        self.latch.reset();
    }

    fn after_fft(&mut self, fft: &[Complex<f32>], rate: f32, rms: f32) -> Option<Bands> {
        let raw = band_energies(fft, rate);
        // Keep the broadband dynamics reference across silence: a quiet
        // return should still draw shorter bars than the preceding loud song.
        self.rms_ref = (self.rms_ref * RMS_DECAY).max(rms).max(1e-4);
        let dyn_target = (rms / self.rms_ref).clamp(0.0, 1.0).sqrt();
        let dk = if dyn_target > self.smoothed_dyn {
            ATTACK
        } else {
            DYN_RELEASE
        };
        self.smoothed_dyn += (dyn_target - self.smoothed_dyn) * dk;
        let dyn_scale = DYN_FLOOR + (1.0 - DYN_FLOOR) * self.smoothed_dyn;

        let mut norm = [0.0f32; 3];
        for i in 0..3 {
            self.gain_ref[i] = (self.gain_ref[i] * GAIN_DECAY).max(raw[i]).max(1e-4);
            let target = (raw[i] / self.gain_ref[i]).clamp(0.0, 1.0);
            let k = if target > self.smoothed[i] {
                ATTACK
            } else {
                RELEASE
            };
            self.smoothed[i] += (target - self.smoothed[i]) * k;
            norm[i] = self.smoothed[i];
        }
        let mut spectrum = [0.0f32; SPECTRUM_BINS];
        for (i, value) in spectrum.iter_mut().enumerate() {
            let raw_e = range_energy(fft, rate, self.spec_edges[i], self.spec_edges[i + 1]);
            self.gain_spec[i] = (self.gain_spec[i] * GAIN_DECAY).max(raw_e).max(1e-4);
            let target = (raw_e / self.gain_spec[i]).clamp(0.0, 1.0);
            let k = if target > self.smoothed_spec[i] {
                ATTACK
            } else {
                RELEASE
            };
            self.smoothed_spec[i] += (target - self.smoothed_spec[i]) * k;
            *value = self.smoothed_spec[i];
        }
        // Unscaled level drives wake/sleep; dynamics affect only bar height.
        let bands = Bands {
            bass: norm[0] * dyn_scale,
            mid: norm[1] * dyn_scale,
            high: norm[2] * dyn_scale,
            level: (norm[0] * 0.5 + norm[1] * 0.35 + norm[2] * 0.15).clamp(0.0, 1.0),
            spectrum: spectrum.map(|s| s * dyn_scale),
        };
        match self.latch.after_fft(bands_quiet(&bands)) {
            LatchEmit::Bands => Some(bands),
            LatchEmit::Zero => {
                // A prior loud peak must not keep a new quiet intro asleep.
                // Recalibrate frequency gain after sustained visual silence;
                // retain rms_ref so quiet sections still draw shorter bars.
                self.gain_ref = [1e-4; 3];
                self.gain_spec = [1e-4; SPECTRUM_BINS];
                Some(Bands::default())
            }
            LatchEmit::None => None,
        }
    }
}

/// Search and prefs never consume this event. emit_to on a missing focus
/// window (create-on-open) filters to zero listeners and returns Ok.
fn emit_bands(app: &AppHandle, bands: Bands) {
    for label in ["main", "focus"] {
        let _ = app.emit_to(label, "audio-bands", bands);
    }
}

/// The AUMID of the GSMTC session the media loop is currently riding — the
/// process-scoped capture's target. Written by the media loop every beat
/// (same cadence as the capture switch); read by the owner thread when it
/// opens capture and each health check, so a player change re-scopes.
static TARGET_AUMID: Mutex<String> = Mutex::new(String::new());

pub fn set_target(aumid: &str) {
    let mut t = TARGET_AUMID
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *t != aumid {
        aumid.clone_into(&mut t);
    }
}

fn target_aumid() -> String {
    TARGET_AUMID
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn open_loopback(
    ring: Arc<Mutex<Ring>>,
    frames: Arc<std::sync::atomic::AtomicU64>,
) -> Option<(cpal::Stream, f32)> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let config = device.default_output_config().ok()?;
    // cpal panics INSIDE the audio callback on a type mismatch — degrade to
    // "no visuals" instead for the rare non-F32 shared-mode mix format.
    if config.sample_format() != cpal::SampleFormat::F32 {
        log::warn!(
            "audio loopback: unsupported sample format {:?}",
            config.sample_format()
        );
        return None;
    }
    let sample_rate = config.sample_rate().0 as f32;
    let channels = config.channels() as usize;
    // Building an INPUT stream on an OUTPUT device = WASAPI loopback.
    // The Mutex hold inside the callback is a few µs (push into a fixed ring);
    // snapshot() on the reader side is equally short — contention risk is
    // negligible at 30Hz reads.
    // Downmixed block, reused across callbacks (no allocation on the audio
    // thread) — the karaoke recorder takes it as one push per callback.
    let mut mono: Vec<f32> = Vec::with_capacity(4096);
    let stream = device
        .build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                frames.fetch_add(1, Ordering::Relaxed);
                mono.clear();
                for frame in data.chunks(channels.max(1)) {
                    mono.push(frame.iter().copied().sum::<f32>() / frame.len().max(1) as f32);
                }
                {
                    let mut ring = match ring.lock() {
                        Ok(r) => r,
                        Err(p) => p.into_inner(),
                    };
                    for &m in &mono {
                        ring.push_frame(m);
                    }
                }
                crate::karaoke::push_frames(&mono, None, sample_rate as u32);
            },
            |e| log::warn!("audio loopback stream error: {e}"),
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some((stream, sample_rate))
}

/// RMS energy of one frequency range. At high sample rates (96/192kHz) the
/// narrow low log bins fall below the FFT's resolution — clamping `hi_bin`
/// up to `lo_bin` merges them into the nearest real bin (neighbors read the
/// same energy) instead of leaving them permanently dead-zero.
fn range_energy(spectrum: &[Complex<f32>], sample_rate: f32, lo_hz: f32, hi_hz: f32) -> f32 {
    let bin_hz = sample_rate / FFT_SIZE as f32;
    let lo_bin = ((lo_hz / bin_hz) as usize).clamp(1, FFT_SIZE / 2 - 1);
    let hi_bin = ((hi_hz / bin_hz) as usize).clamp(lo_bin, FFT_SIZE / 2 - 1);
    let sum: f32 = spectrum[lo_bin..=hi_bin].iter().map(|c| c.norm_sqr()).sum();
    (sum / (hi_bin - lo_bin + 1) as f32).sqrt()
}

fn band_energies(spectrum: &[Complex<f32>], sample_rate: f32) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for (i, (lo, hi)) in BANDS.iter().enumerate() {
        out[i] = range_energy(spectrum, sample_rate, *lo, *hi);
    }
    out
}

/// A live capture, either scope. Process is the wanted path (only the
/// player's audio); Device is the whole-mix fallback (the pre-scoping
/// behavior) when the AUMID→PID join can't land.
enum Capture {
    /// The capture plus the AUMID it was scoped to — a player change
    /// (Spotify → Apple Music) must re-resolve, not keep riding the old app.
    Process(loopback::ProcessCapture, String),
    /// The stream is never read — held for its Drop (drop = stop capture).
    Device(#[allow(dead_code)] cpal::Stream),
}

/// Process-path staleness horizon: no packets for this long renders as
/// silence (a process-loopback stream legitimately delivers nothing while
/// the target is quiet — FFT'ing the stale ring would freeze the bars at
/// their last heights instead of letting them fall).
const SILENCE_AFTER_MS: u64 = 250;
/// While on the Device fallback, retry the process-scoped upgrade this
/// often — the player's audio session often appears a beat after playback
/// starts, and a missed join at open time shouldn't stick for the session.
const UPGRADE_RETRY: Duration = Duration::from_secs(5);
/// Device-fallback stall watchdog: first reopen after this long without a
/// callback frame — the one-off "default device changed / stream silently
/// died" case recovers at this latency.
const STALL_BASE: Duration = Duration::from_secs(2);
/// A reopen the endpoint answers with ZERO frames is FRUITLESS, and WASAPI
/// device loopback legitimately delivers no callbacks while nothing renders
/// locally (GSMTC playing with the audio on a phone/speaker — the DeviceTag
/// case): reopening can't help, so consecutive fruitless reopens double the
/// next stall threshold (2s → 4s → 8s …) up to this cap instead of cycling
/// the device open/close every 2s all session. Any real frame progress, a
/// target change, or a landed process join resets to STALL_BASE, so genuine
/// device-swap recovery keeps its first-reopen latency.
const STALL_CAP: Duration = Duration::from_secs(60);
/// A process capture that has NEVER delivered a packet WITH SIGNAL this long
/// into playback either joined the wrong process (multi-profile browsers can
/// alias an AUMID) or joined a session that only renders SILENCE while the
/// audible audio goes elsewhere (a spatial mixer, or a sibling process —
/// loopback.rs's second quirk). The right join delivers real audio within the
/// first beat of a playing track. Demote that AUMID to the Device fallback,
/// stickily (re-resolving would pick the same silent PID and strand the bars at
/// zero again), until the playing app changes — the whole-mix fallback captures
/// the endpoint IF the audio is in the shared mix at all. A capture that has
/// delivered signal is never demoted: its later silence is the target really
/// rendering nothing.
const DEMOTE_AFTER_MS: u64 = 10_000;
/// A demote is sticky but NOT permanent (it used to clear only on an AUMID
/// change — so a single transient activation blip, or >10s of digital silence
/// at open, stranded a player on whole-mix Discord-bleed capture for the whole
/// session). After this BASE window the AUMID is retried with a process-scoped
/// join; it's wide enough that a genuinely-uncapturable target (exclusive mode,
/// sibling-process render) isn't re-resolved every loop.
const DEMOTE_EXPIRY: Duration = Duration::from_secs(90);
/// Backoff ceiling for a PERMANENT mis-join. Without it, a target whose process
/// capture activates but never delivers packets retries every DEMOTE_EXPIRY
/// forever — each retry opens the silent capture and flattens the waveform for
/// DEMOTE_AFTER_MS (~10s flat every 90s). Each consecutive silent re-demote of
/// the same AUMID instead doubles its retry window (90s → 180s → … → this cap;
/// stamp_demote), so a permanent mis-join settles to one brief flat per ~10min.
/// A target that changes, or a retry that actually delivers signal, resets to
/// the base window — so transient failures still recover at DEMOTE_EXPIRY.
const DEMOTE_EXPIRY_CAP: Duration = Duration::from_secs(600);

/// True while `aumid` is under an UNEXPIRED demote — its stored window (which
/// grows with the backoff, see stamp_demote) hasn't elapsed. Expired → false,
/// so the next open retries the process join; the stamp is deliberately LEFT in
/// place (not cleared) so a re-demote reads its window as the doubling base. A
/// target change or a genuine recovery clears it (see the demoted decl).
fn is_demoted(demoted: &Option<(String, Instant, Duration)>, aumid: &str) -> bool {
    match demoted {
        Some((d, since, expiry)) => d == aumid && since.elapsed() < *expiry,
        None => false,
    }
}

/// Stamp (or re-stamp) the sticky demote for `aumid`. Re-demoting the SAME
/// target — a consecutive expiry-retry that opened the process capture and it
/// still never delivered — doubles the retry window (capped at
/// DEMOTE_EXPIRY_CAP); a first demote, or one after the target changed/
/// recovered (`prev` None or a different AUMID), starts at the base window.
fn stamp_demote(
    prev: &Option<(String, Instant, Duration)>,
    aumid: String,
) -> (String, Instant, Duration) {
    let expiry = match prev {
        Some((d, _, e)) if *d == aumid => (*e * 2).min(DEMOTE_EXPIRY_CAP),
        _ => DEMOTE_EXPIRY,
    };
    (aumid, Instant::now(), expiry)
}

/// Owner thread: opens/drops the capture as the switch flips, runs the
/// FFT + smoothing + emit loop while on.
pub fn spawn(app: AppHandle, switch: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("audio-owner".into())
        .spawn(move || {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        // Hann window, precomputed.
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                let x = (i as f32 / (FFT_SIZE - 1) as f32) * std::f32::consts::TAU;
                0.5 * (1.0 - x.cos())
            })
            .collect();

        // COM for loopback.rs's session enumeration on this thread (cpal
        // inits its own MTA per stream thread; S_FALSE double-init is fine).
        let com = unsafe {
            windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            )
        };
        let _ = com; // held for the thread's lifetime (it never exits)

        let mut active: Option<(Capture, f32, Arc<Mutex<Ring>>)> = None;
        let mut last_upgrade = std::time::Instant::now();
        // The one AUMID currently demoted to Device capture: (aumid, stamped-at,
        // retry window that applies). The window is the base DEMOTE_EXPIRY,
        // doubled per consecutive silent re-demote up to DEMOTE_EXPIRY_CAP
        // (stamp_demote). Cleared when the target moves off it (open branch) or
        // a retry genuinely recovers (the Process health arm's has_data reset).
        // An EXPIRED stamp is left in place — its window is the next backoff's
        // base, and is_demoted already reads it as not-demoted.
        let mut demoted: Option<(String, Instant, Duration)> = None;
        let mut visual = BandEnvelope::new();
        let mut scratch = vec![Complex::default(); FFT_SIZE];
        let mut snap = vec![0.0f32; FFT_SIZE];
        // Stream-health watchdog: the callback bumps `frames`; if it stalls
        // while we're supposedly capturing (default device changed, stream
        // silently died — cpal never signals this), drop and reopen.
        let frames = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mut last_frames = 0u64;
        let mut last_progress = std::time::Instant::now();
        // Fruitless-reopen backoff (see STALL_CAP) + one-warn-per-episode
        // gates: a silent endpoint otherwise wrote the stall warn AND the
        // no-join warn every ~2s cycle (~3.6k lines/hour). An "episode" ends
        // when frames progress, the target AUMID moves, or a process-scoped
        // join lands — each resets all three so the next episode warns once
        // again at Warn level and starts at the base first-reopen latency.
        let mut stall_threshold = STALL_BASE;
        let mut stall_warned = false;
        let mut fallback_warned = false;
        let mut stall_target = String::new();

        loop {
            let want = switch.load(Ordering::Relaxed);
            if !want {
                if active.is_some() {
                    crate::karaoke::on_capture_stop(&app);
                    active = None; // drops the capture, releases the device
                    visual.reset_capture();
                    emit_bands(&app, Bands::default());
                }
            } else if active.is_none() {
                // Open: process-scoped first (the playing app's tree only),
                // whole-mix device loopback as the fallback.
                let ring = Arc::new(Mutex::new(Ring::new()));
                let aumid = target_aumid();
                if demoted.as_ref().is_some_and(|(d, _, _)| *d != aumid) {
                    demoted = None; // target moved off the demoted AUMID
                }
                if aumid != stall_target {
                    // A fresh target opening = a fresh episode: base stall
                    // latency, warns re-armed. (The Device arm's identical
                    // reset covers a target that moves mid-capture; this one
                    // covers an episode that STARTS here — it must not
                    // inherit the previous target's backoff.)
                    stall_target = aumid.clone();
                    stall_threshold = STALL_BASE;
                    stall_warned = false;
                    fallback_warned = false;
                }
                let process = if aumid.is_empty() || is_demoted(&demoted, &aumid) {
                    None
                } else {
                    loopback::resolve_target(&aumid).and_then(|t| {
                        loopback::ProcessCapture::open(t, ring.clone(), frames.clone())
                    })
                };
                if let Some(cap) = process {
                    let rate = cap.sample_rate;
                    // A landed join ends any stall episode: the Device-arm
                    // resets can't run while a Process capture holds the
                    // slot, so without this a later same-AUMID fall back to
                    // Device inherits a capped threshold + pre-suppressed
                    // warns from BEFORE the interlude.
                    stall_threshold = STALL_BASE;
                    stall_warned = false;
                    fallback_warned = false;
                    active = Some((Capture::Process(cap, aumid), rate, ring));
                } else if let Some((stream, rate)) = open_loopback(ring.clone(), frames.clone()) {
                    if !aumid.is_empty() {
                        // Warn once per fallback episode: a silent-endpoint
                        // reopen cycle re-enters here every pass, and each
                        // cycle re-missing the same join is not news.
                        if !fallback_warned {
                            fallback_warned = true;
                            log::warn!(
                                "audio: no process-loopback join for {aumid:?} — device-mix fallback"
                            );
                        } else {
                            log::debug!(
                                "audio: still no process-loopback join for {aumid:?} — device-mix fallback"
                            );
                        }
                    }
                    last_upgrade = std::time::Instant::now();
                    active = Some((Capture::Device(stream), rate, ring));
                } else {
                    // Device unavailable — retry lazily, don't spin.
                    std::thread::sleep(Duration::from_secs(5));
                }
                if active.is_some() {
                    last_frames = frames.load(Ordering::Relaxed);
                    last_progress = std::time::Instant::now();
                }
            } else {
                // Health check on the live capture. Decide with a short
                // borrow, act after it drops.
                enum Act {
                    Keep,
                    Reopen,
                    Demote(String),
                    TryUpgrade(String),
                }
                let act = match &active.as_ref().expect("checked some").0 {
                    // NO stall watchdog here: a quiet target legitimately
                    // delivers nothing (module docs). Reopen only on real
                    // endings — capture thread died, target process exited,
                    // or the playing app changed out from under the scope —
                    // plus the wrong-join demote (a real player is never
                    // THIS silent mid-playback).
                    Capture::Process(p, aumid) => {
                        // A capture now delivering signal is a genuine recovery
                        // — drop any lingering demote for this target so a
                        // future mis-join retries at the base window (the
                        // backoff is for CONSECUTIVE silent re-demotes only).
                        if p.has_data() && demoted.as_ref().is_some_and(|(d, _, _)| d == aumid) {
                            demoted = None;
                        }
                        // A capture that died having NEVER delivered is a
                        // broken join — demote, don't reopen: a plain reopen
                        // would re-run the full resolution+activation at
                        // loop cadence against the same broken target.
                        if p.done() && !p.has_data() {
                            Act::Demote(aumid.clone())
                        } else if p.done() || !p.target_alive() || *aumid != target_aumid() {
                            Act::Reopen
                        } else if !p.has_data() && p.ms_since_data() > DEMOTE_AFTER_MS {
                            Act::Demote(aumid.clone())
                        } else {
                            Act::Keep
                        }
                    }
                    Capture::Device(_) => {
                        let now_frames = frames.load(Ordering::Relaxed);
                        if now_frames != last_frames {
                            last_frames = now_frames;
                            last_progress = std::time::Instant::now();
                            // Real packets: the endpoint is audible — restore
                            // the first-reopen latency and re-arm the
                            // per-episode warns.
                            stall_threshold = STALL_BASE;
                            stall_warned = false;
                            fallback_warned = false;
                        }
                        let aumid = target_aumid();
                        if aumid != stall_target {
                            // Target moved (player change): a fresh endpoint
                            // deserves the base latency and a fresh warn.
                            stall_target = aumid.clone();
                            stall_threshold = STALL_BASE;
                            stall_warned = false;
                            fallback_warned = false;
                        }
                        if last_progress.elapsed() > stall_threshold {
                            // Fire the reopen and pre-double the next wait:
                            // a reopen that delivers frames resets to base
                            // above, so only a FRUITLESS one (silent
                            // endpoint) keeps the doubled threshold — the
                            // cadence decays 2s→4s→…→cap instead of
                            // churning.
                            stall_threshold = (stall_threshold * 2).min(STALL_CAP);
                            if !stall_warned {
                                stall_warned = true;
                                log::warn!(
                                    "audio loopback stalled — reopening against current default device"
                                );
                            } else {
                                log::debug!(
                                    "audio loopback still silent — reopen backoff now {}s",
                                    stall_threshold.as_secs()
                                );
                            }
                            Act::Reopen
                        } else if last_upgrade.elapsed() > UPGRADE_RETRY {
                            last_upgrade = std::time::Instant::now();
                            if aumid.is_empty() || is_demoted(&demoted, &aumid) {
                                Act::Keep
                            } else {
                                Act::TryUpgrade(aumid)
                            }
                        } else {
                            Act::Keep
                        }
                    }
                };
                match act {
                    Act::Keep => {}
                    Act::Reopen => {
                        active = None; // next iteration reopens with fresh resolution
                        continue;
                    }
                    Act::Demote(aumid) => {
                        log::warn!("audio: process capture for {aumid:?} never delivered — device-mix fallback");
                        demoted = Some(stamp_demote(&demoted, aumid));
                        active = None; // reopens demoted (Device) next iteration
                        continue;
                    }
                    Act::TryUpgrade(aumid) => {
                        // The player's session may have appeared since we fell
                        // back — swap up to the scoped capture when it has.
                        // A no-session miss keeps retrying (resolution is a
                        // cheap enumeration); a join that resolved but FAILED
                        // to activate demotes stickily — retrying it would
                        // stutter the working Device stream with a blocking
                        // multi-second activation attempt every 5s.
                        match loopback::resolve_target(&aumid) {
                            None => {}
                            Some(t) => {
                                let ring = Arc::new(Mutex::new(Ring::new()));
                                if let Some(cap) =
                                    loopback::ProcessCapture::open(t, ring.clone(), frames.clone())
                                {
                                    let rate = cap.sample_rate;
                                    active = Some((Capture::Process(cap, aumid), rate, ring));
                                    last_frames = frames.load(Ordering::Relaxed);
                                    last_progress = std::time::Instant::now();
                                    // Same episode-end as the open-branch
                                    // join: the upgrade proves the target
                                    // deliverable, so a later Device
                                    // fallback is a NEW episode.
                                    stall_threshold = STALL_BASE;
                                    stall_warned = false;
                                    fallback_warned = false;
                                } else {
                                    log::warn!("audio: process activation failed for {aumid:?} — staying on device mix");
                                    demoted = Some(stamp_demote(&demoted, aumid));
                                }
                            }
                        }
                    }
                }
            }

            let Some((cap, rate, ring)) = &active else {
                std::thread::sleep(Duration::from_millis(250));
                continue;
            };

            // Process-path staleness reads as SILENCE (zero samples), so the
            // bars fall instead of freezing on the last-heard spectrum.
            let stale =
                matches!(cap, Capture::Process(p, _) if p.ms_since_data() > SILENCE_AFTER_MS);

            if visual.latch.is_latched() {
                // Stale process-path packets leave the last-heard samples
                // in the ring. Peeking that RMS would unlatch forever.
                let raw_awake = if stale {
                    false
                } else {
                    let ring = ring
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    ring.rms() > RAW_WAKE
                };
                if !visual.latch.consider_wake(raw_awake) {
                    std::thread::sleep(EMIT_INTERVAL);
                    continue;
                }
            }

            if stale {
                snap.fill(0.0);
            } else {
                let ring = ring
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                ring.snapshot_into(&mut snap);
            }
            for (i, s) in snap.iter().enumerate() {
                scratch[i] = Complex::new(s * window[i], 0.0);
            }
            fft.process(&mut scratch);
            let rms = (snap.iter().map(|s| s * s).sum::<f32>() / snap.len() as f32).sqrt();
            if let Some(bands) = visual.after_fft(&scratch, *rate, rms) {
                emit_bands(&app, bands);
            }
            std::thread::sleep(EMIT_INTERVAL);
        }
        })
        .expect("spawn audio-owner thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(latch: &mut SilenceLatch, quiet: bool, raw_awake: bool) -> LatchEmit {
        if latch.is_latched() && !latch.consider_wake(raw_awake) {
            return LatchEmit::None;
        }
        latch.after_fft(quiet)
    }

    #[test]
    fn silence_emits_one_zero_then_stops() {
        let mut latch = SilenceLatch::new();
        let mut saw = Vec::new();
        // Include several periodic probes: silence must not re-emit zeros.
        for _ in 0..LATCH_AFTER_TICKS as usize + 24 {
            saw.push(step(&mut latch, true, false));
        }
        let live = LATCH_AFTER_TICKS as usize - 1;
        assert!(
            saw[..live].iter().all(|e| *e == LatchEmit::Bands),
            "expected {live} live emits before the latch, got {saw:?}"
        );
        assert_eq!(saw[live], LatchEmit::Zero);
        assert!(
            saw[live + 1..].iter().all(|e| *e == LatchEmit::None),
            "expected no further emits after the zero, got {saw:?}"
        );
        assert!(latch.is_latched());
    }

    #[test]
    fn energy_resumes_immediately() {
        let mut latch = SilenceLatch::new();
        for _ in 0..LATCH_AFTER_TICKS {
            step(&mut latch, true, false);
        }
        assert!(latch.is_latched());
        assert_eq!(step(&mut latch, false, true), LatchEmit::Bands);
        assert!(!latch.is_latched());
    }

    /// A quiet intro can be audible and strong enough for the auto-gained
    /// waveform while its raw RMS is below the latch's fast-wake threshold.
    /// The live Spotify probe measured RMS around 0.00013; use a tone at that
    /// amplitude to reconcile the raw gate with the real FFT input.
    #[test]
    fn quiet_intro_resumes_after_silence() {
        assert_quiet_intro_resumes(false);
    }

    #[test]
    fn quiet_intro_resumes_after_loud_song() {
        assert_quiet_intro_resumes(true);
    }

    fn tone_fft(rms: f32) -> (Vec<Complex<f32>>, f32) {
        let mut ring = Ring::new();
        for i in 0..FFT_SIZE {
            let t = i as f32 / 48_000.0;
            ring.push_frame(rms * 2.0f32.sqrt() * (t * 440.0 * std::f32::consts::TAU).sin());
        }
        let measured_rms = ring.rms();
        let mut fft = ring
            .snapshot()
            .into_iter()
            .enumerate()
            .map(|(i, sample)| {
                let hann =
                    0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / (FFT_SIZE - 1) as f32).cos());
                Complex::new(sample * hann, 0.0)
            })
            .collect::<Vec<_>>();
        FftPlanner::<f32>::new()
            .plan_fft_forward(FFT_SIZE)
            .process(&mut fft);
        (fft, measured_rms)
    }

    fn visual_tick(visual: &mut BandEnvelope, fft: &[Complex<f32>], rms: f32) -> Option<Bands> {
        if !visual.latch.consider_wake(rms > RAW_WAKE) {
            return None;
        }
        visual.after_fft(fft, 48_000.0, rms)
    }

    fn assert_quiet_intro_resumes(after_loud_song: bool) {
        let mut visual = BandEnvelope::new();
        let mut loud_height = None;
        if after_loud_song {
            let (loud, rms) = tone_fft(0.1);
            for _ in 0..150 {
                loud_height = visual_tick(&mut visual, &loud, rms).map(|b| b.mid);
            }
        }
        let zero = vec![Complex::default(); FFT_SIZE];
        let mut zero_emits = 0;
        for _ in 0..160 {
            let was_latched = visual.latch.is_latched();
            let output = visual_tick(&mut visual, &zero, 0.0);
            if was_latched {
                assert!(output.is_none(), "silent probes must not emit");
            } else if visual.latch.is_latched() {
                assert_eq!(output.unwrap().level, 0.0);
                zero_emits += 1;
            }
        }
        assert!(visual.latch.is_latched());
        assert_eq!(
            zero_emits, 1,
            "silence emits one terminating zero, even across probes"
        );

        let (quiet, rms) = tone_fft(0.00013);
        assert!(rms > 0.0001 && rms < RAW_WAKE);
        let mut wake = None;
        assert!(
            (0..8).any(|_| visual_tick(&mut visual, &quiet, rms).is_some_and(|b| {
                wake = Some(b);
                b.level > 0.02
            })),
            "quiet music must deliver a frontend wake payload within eight ticks"
        );
        assert!(!visual.latch.is_latched());
        if let Some(loud_height) = loud_height {
            assert!(
                wake.unwrap().mid < loud_height,
                "quiet music must still draw shorter bars"
            );
        }
    }

    #[test]
    fn brief_quiet_does_not_latch() {
        let mut latch = SilenceLatch::new();
        for _ in 0..LATCH_AFTER_TICKS - 1 {
            assert_eq!(step(&mut latch, true, false), LatchEmit::Bands);
        }
        assert_eq!(step(&mut latch, false, true), LatchEmit::Bands);
        assert!(!latch.is_latched());
    }

    #[test]
    fn snapshot_into_is_chronological() {
        let mut ring = Ring::new();
        for i in 0..FFT_SIZE {
            ring.push_frame(i as f32);
        }
        let mut out = vec![0.0; FFT_SIZE];
        ring.snapshot_into(&mut out);
        for (i, v) in out.iter().enumerate() {
            assert_eq!(*v, i as f32);
        }
        ring.push_frame(999.0);
        ring.snapshot_into(&mut out);
        assert_eq!(out[0], 1.0);
        assert_eq!(out[FFT_SIZE - 1], 999.0);
    }
}
