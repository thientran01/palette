//! Audio-reactive core: WASAPI loopback capture of the default output device
//! → FFT → smoothed band energies emitted as "audio-bands" events (~30Hz).
//!
//! Lifecycle discipline (plan M4): capture runs ONLY while the widget is
//! visible AND something is playing. The owner thread opens/drops the cpal
//! stream on demand — dropping releases the device entirely, so a hidden or
//! paused widget costs zero audio work.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const FFT_SIZE: usize = 2048;
const EMIT_INTERVAL: Duration = Duration::from_millis(33);
/// Band edges in Hz: bass, mid, high.
const BANDS: [(f32, f32); 3] = [(30.0, 150.0), (150.0, 2000.0), (2000.0, 8000.0)];
const ATTACK: f32 = 0.55;
const RELEASE: f32 = 0.12;
/// Auto-gain reference decays slowly so quiet and loud tracks both animate.
const GAIN_DECAY: f32 = 0.995;

#[derive(Serialize, Clone, Copy, Default)]
pub struct Bands {
    pub bass: f32,
    pub mid: f32,
    pub high: f32,
    /// Overall level 0..1 (auto-gained RMS).
    pub level: f32,
}

/// Latest samples ring (mono-mixed), written by the cpal callback.
struct Ring {
    buf: Vec<f32>,
    pos: usize,
}

impl Ring {
    fn new() -> Self {
        Ring { buf: vec![0.0; FFT_SIZE], pos: 0 }
    }
    fn push_frame(&mut self, frame_mean: f32) {
        self.buf[self.pos] = frame_mean;
        self.pos = (self.pos + 1) % self.buf.len();
    }
    /// Snapshot in chronological order.
    fn snapshot(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.buf.len());
        out.extend_from_slice(&self.buf[self.pos..]);
        out.extend_from_slice(&self.buf[..self.pos]);
        out
    }
}

fn open_loopback(ring: Arc<Mutex<Ring>>) -> Option<(cpal::Stream, f32)> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let config = device.default_output_config().ok()?;
    let sample_rate = config.sample_rate().0 as f32;
    let channels = config.channels() as usize;
    // Building an INPUT stream on an OUTPUT device = WASAPI loopback.
    let stream = device
        .build_input_stream(
            &config.into(),
            move |data: &[f32], _| {
                let mut ring = match ring.lock() {
                    Ok(r) => r,
                    Err(p) => p.into_inner(),
                };
                for frame in data.chunks(channels.max(1)) {
                    let mean = frame.iter().copied().sum::<f32>() / frame.len().max(1) as f32;
                    ring.push_frame(mean);
                }
            },
            |e| eprintln!("audio loopback stream error: {e}"),
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some((stream, sample_rate))
}

fn band_energies(spectrum: &[Complex<f32>], sample_rate: f32) -> [f32; 3] {
    let bin_hz = sample_rate / FFT_SIZE as f32;
    let mut out = [0.0f32; 3];
    for (i, (lo, hi)) in BANDS.iter().enumerate() {
        let lo_bin = ((lo / bin_hz) as usize).max(1);
        let hi_bin = ((hi / bin_hz) as usize).min(FFT_SIZE / 2 - 1);
        if hi_bin <= lo_bin {
            continue;
        }
        let sum: f32 = spectrum[lo_bin..=hi_bin].iter().map(|c| c.norm_sqr()).sum();
        out[i] = (sum / (hi_bin - lo_bin + 1) as f32).sqrt();
    }
    out
}

/// Owner thread: opens/drops the loopback stream as the switch flips, runs the
/// FFT + smoothing + emit loop while on.
pub fn spawn(app: AppHandle, switch: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        // Hann window, precomputed.
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                let x = (i as f32 / (FFT_SIZE - 1) as f32) * std::f32::consts::TAU;
                0.5 * (1.0 - x.cos())
            })
            .collect();

        let mut active: Option<(cpal::Stream, f32, Arc<Mutex<Ring>>)> = None;
        let mut smoothed = [0.0f32; 3];
        let mut gain_ref = [1e-4f32; 3];
        let mut scratch = vec![Complex::default(); FFT_SIZE];

        loop {
            let want = switch.load(Ordering::Relaxed);
            match (&active, want) {
                (None, true) => {
                    let ring = Arc::new(Mutex::new(Ring::new()));
                    if let Some((stream, rate)) = open_loopback(ring.clone()) {
                        active = Some((stream, rate, ring));
                    } else {
                        // Device unavailable — retry lazily, don't spin.
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
                (Some(_), false) => {
                    active = None; // drops the stream, releases the device
                    smoothed = [0.0; 3];
                    let _ = app.emit("audio-bands", Bands::default());
                }
                _ => {}
            }

            let Some((_, rate, ring)) = &active else {
                std::thread::sleep(Duration::from_millis(250));
                continue;
            };

            let samples = {
                let ring = ring.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                ring.snapshot()
            };
            for (i, s) in samples.iter().enumerate() {
                scratch[i] = Complex::new(s * window[i], 0.0);
            }
            fft.process(&mut scratch);
            let raw = band_energies(&scratch, *rate);

            let mut norm = [0.0f32; 3];
            for i in 0..3 {
                gain_ref[i] = (gain_ref[i] * GAIN_DECAY).max(raw[i]).max(1e-4);
                let target = (raw[i] / gain_ref[i]).clamp(0.0, 1.0);
                let k = if target > smoothed[i] { ATTACK } else { RELEASE };
                smoothed[i] += (target - smoothed[i]) * k;
                norm[i] = smoothed[i];
            }
            let bands = Bands {
                bass: norm[0],
                mid: norm[1],
                high: norm[2],
                level: (norm[0] * 0.5 + norm[1] * 0.35 + norm[2] * 0.15).clamp(0.0, 1.0),
            };
            let _ = app.emit("audio-bands", bands);
            std::thread::sleep(EMIT_INTERVAL);
        }
    });
}
