//! Native MDX-Net vocal separation for the development-only vocal trial
//! (`vocal_preview.rs`). Runs Kim Vocal 2 through the ONNX Runtime the
//! acoustic aligner already loads, so no Python or torch is involved.
//!
//! The model wants 44.1kHz stereo. Loopback captures 48kHz stereo, so the
//! trial records that alongside the usual 16kHz mono and resamples here.
//! A mono 16kHz recording (the cpal fallback path) still works: it is
//! upsampled and duplicated, which can't restore what the downmix lost.
//!
//! Pipeline per 5.9s chunk, as in the reference MDX-Net demixer: STFT
//! (n_fft 7680, hop 1024, periodic Hann, centered with reflect padding),
//! keep the lowest 3072 bins, zero the bottom three, run the model, inverse
//! STFT, keep the chunk's middle (n_fft/2 trimmed from each side).
use ort::{session::Session, value::Tensor};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::{path::Path, sync::Arc};

type Result<T> = std::result::Result<T, String>;

/// UVR's Kim Vocal 2 export. Downloaded locally, never bundled.
pub const MODEL_FILE: &str = "Kim_Vocal_2.onnx";
pub const MODEL_SHA256: &str = "ce74ef3b6a6024ce44211a07be9cf8bc6d87728cc852a68ab34eb8e58cde9c8b";

pub const MODEL_RATE: u32 = 44_100;
const N_FFT: usize = 7680;
const HOP: usize = 1024;
const DIM_F: usize = 3072;
const DIM_T: usize = 256;
const N_BINS: usize = N_FFT / 2 + 1;
const TRIM: usize = N_FFT / 2;
const CHUNK: usize = HOP * (DIM_T - 1);
const GEN: usize = CHUNK - 2 * TRIM;
/// Model-specific output gain from UVR's model table for this checksum.
const COMPENSATE: f32 = 1.009;
/// Eight minutes, matching the recorder's MAX_SAMPLES bound.
const MAX_SECONDS: usize = 8 * 60;

/// Interleaved stereo at the capture rate, as recorded by the trial.
#[derive(Clone, Debug, PartialEq)]
pub struct Stereo {
    pub interleaved: Vec<i16>,
    pub rate: u32,
}

impl Stereo {
    pub fn frames(&self) -> usize {
        self.interleaved.len() / 2
    }
}

pub struct VocalSeparator {
    session: Session,
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
}

impl VocalSeparator {
    /// The ONNX Runtime must already be initialized by
    /// `AcousticAligner::load`; the worker always loads that first.
    pub fn load(model: &Path) -> Result<Self> {
        if !model.is_file() {
            return Err("vocal separation model missing".into());
        }
        crate::acoustic::verify_asset(model, MODEL_SHA256)?;
        let session = Session::builder()
            .map_err(|e| e.to_string())?
            .with_intra_threads(2)
            .map_err(|e| e.to_string())?
            .with_inter_threads(1)
            .map_err(|e| e.to_string())?
            .commit_from_file(model)
            .map_err(|e| e.to_string())?;
        let mut planner = FftPlanner::new();
        Ok(Self {
            session,
            forward: planner.plan_fft_forward(N_FFT),
            inverse: planner.plan_fft_inverse(N_FFT),
            window: periodic_hann(N_FFT),
        })
    }

    /// Mono vocals at 16kHz with exactly `mono16k_len` samples, on the same
    /// sample grid as the recorder's 16kHz PCM. Uses the stereo capture when
    /// present, else the mono recording itself.
    pub fn vocals_16k(
        &mut self,
        stereo: Option<&Stereo>,
        mono16k: &[i16],
        target_hz: u32,
    ) -> Result<Vec<i16>> {
        let vocals = match stereo {
            Some(s) => {
                if s.rate == 0 || s.interleaved.len() % 2 != 0 {
                    return Err("invalid stereo capture".into());
                }
                let to_f = |v: i16| v as f32 / 32768.0;
                let left: Vec<f32> = s.interleaved.iter().step_by(2).map(|&v| to_f(v)).collect();
                let right: Vec<f32> = s
                    .interleaved
                    .iter()
                    .skip(1)
                    .step_by(2)
                    .map(|&v| to_f(v))
                    .collect();
                let mono = self.separate(&left, &right, s.rate)?;
                to_target_grid(&mono, s.rate, target_hz)
            }
            None => {
                let mono: Vec<f32> = mono16k.iter().map(|&v| v as f32 / 32768.0).collect();
                self.separate(&mono, &mono, target_hz)?
            }
        };
        if vocals.len() != mono16k.len() {
            return Err(format!(
                "separated vocals have {} samples, recording has {}",
                vocals.len(),
                mono16k.len()
            ));
        }
        Ok(to_i16_normalized(&vocals))
    }

    /// Mono vocal estimate at `rate`, same length as the input channels.
    pub fn separate(&mut self, left: &[f32], right: &[f32], rate: u32) -> Result<Vec<f32>> {
        if left.len() != right.len() || left.is_empty() || rate == 0 {
            return Err("invalid separation input".into());
        }
        if left.len() > rate as usize * MAX_SECONDS {
            return Err("separation input exceeds eight minutes".into());
        }
        let l = resample(left, rate, MODEL_RATE);
        let r = resample(right, rate, MODEL_RATE);
        let (vl, vr) = self.demix(&l, &r)?;
        let mono: Vec<f32> = vl.iter().zip(&vr).map(|(a, b)| (a + b) * 0.5).collect();
        let mut back = resample(&mono, MODEL_RATE, rate);
        back.resize(left.len(), 0.0);
        Ok(back)
    }

    /// Stereo 44.1kHz mix → stereo vocals, same length.
    fn demix(&mut self, left: &[f32], right: &[f32]) -> Result<(Vec<f32>, Vec<f32>)> {
        let n = left.len();
        let pad = GEN - n % GEN;
        let padded = |x: &[f32]| {
            let mut p = vec![0.0f32; TRIM + n + pad + TRIM];
            p[TRIM..TRIM + n].copy_from_slice(x);
            p
        };
        let (pl, pr) = (padded(left), padded(right));
        let mut out_l = Vec::with_capacity(n + pad);
        let mut out_r = Vec::with_capacity(n + pad);
        let mut spec = vec![0.0f32; 4 * DIM_F * DIM_T];
        for start in (0..n + pad).step_by(GEN) {
            spec.fill(0.0);
            let (fwd, win) = (&*self.forward, &self.window);
            stft_into(fwd, win, &pl[start..start + CHUNK], &mut spec, 0);
            stft_into(fwd, win, &pr[start..start + CHUNK], &mut spec, 2);
            let tensor = Tensor::from_array(([1usize, 4, DIM_F, DIM_T], spec.clone()))
                .map_err(|e| e.to_string())?;
            let output = self
                .session
                .run(ort::inputs!["input" => tensor])
                .map_err(|e| e.to_string())?;
            let (shape, pred) = output
                .get("output")
                .ok_or("missing separation output")?
                .try_extract_tensor::<f32>()
                .map_err(|e| e.to_string())?;
            if shape.iter().map(|&d| d as usize).collect::<Vec<_>>() != [1, 4, DIM_F, DIM_T]
                || pred.iter().any(|v| !v.is_finite())
            {
                return Err("unexpected separation output".into());
            }
            let (inv, win) = (&*self.inverse, &self.window);
            out_l.extend_from_slice(&istft(inv, win, pred, 0)[TRIM..CHUNK - TRIM]);
            out_r.extend_from_slice(&istft(inv, win, pred, 2)[TRIM..CHUNK - TRIM]);
        }
        out_l.truncate(n);
        out_r.truncate(n);
        for v in out_l.iter_mut().chain(out_r.iter_mut()) {
            *v *= COMPENSATE;
        }
        Ok((out_l, out_r))
    }
}

/// Writes one channel's real/imag planes at `plane`, `plane + 1`.
fn stft_into(forward: &dyn Fft<f32>, window: &[f32], x: &[f32], spec: &mut [f32], plane: usize) {
    debug_assert_eq!(x.len(), CHUNK);
    let half = N_FFT / 2;
    // Centered STFT with reflect padding, as torch.stft(center=True).
    let reflect = |i: isize| -> f32 {
        let len = x.len() as isize;
        let j = if i < 0 {
            -i
        } else if i >= len {
            2 * (len - 1) - i
        } else {
            i
        };
        x[j as usize]
    };
    let mut buf = vec![Complex::new(0.0f32, 0.0); N_FFT];
    let plane_len = DIM_F * DIM_T;
    for t in 0..DIM_T {
        let origin = (t * HOP) as isize - half as isize;
        for (k, b) in buf.iter_mut().enumerate() {
            *b = Complex::new(reflect(origin + k as isize) * window[k], 0.0);
        }
        forward.process(&mut buf);
        // Bins 0..3 stay zero: the reference demixer drops them.
        for (f, c) in buf.iter().enumerate().take(DIM_F).skip(3) {
            spec[plane * plane_len + f * DIM_T + t] = c.re;
            spec[(plane + 1) * plane_len + f * DIM_T + t] = c.im;
        }
    }
}

/// Inverse of `stft_into` for one channel, CHUNK samples long. Bins above
/// DIM_F are zero; overlap-add is normalized by the summed squared window.
fn istft(inverse: &dyn Fft<f32>, window: &[f32], spec: &[f32], plane: usize) -> Vec<f32> {
    let plane_len = DIM_F * DIM_T;
    let total = CHUNK + N_FFT;
    let mut acc = vec![0.0f32; total];
    let mut norm = vec![0.0f32; total];
    let mut buf = vec![Complex::new(0.0f32, 0.0); N_FFT];
    for t in 0..DIM_T {
        buf.fill(Complex::new(0.0, 0.0));
        for f in 0..DIM_F {
            let c = Complex::new(
                spec[plane * plane_len + f * DIM_T + t],
                spec[(plane + 1) * plane_len + f * DIM_T + t],
            );
            buf[f] = c;
            // Hermitian mirror; DC and Nyquist (unused here) are their own.
            if f > 0 && f < N_BINS - 1 {
                buf[N_FFT - f] = c.conj();
            }
        }
        buf[0].im = 0.0;
        inverse.process(&mut buf);
        let at = t * HOP;
        for k in 0..N_FFT {
            let w = window[k];
            acc[at + k] += buf[k].re / N_FFT as f32 * w;
            norm[at + k] += w * w;
        }
    }
    let half = N_FFT / 2;
    (half..half + CHUNK)
        .map(|i| {
            if norm[i] > 1e-11 {
                acc[i] / norm[i]
            } else {
                acc[i]
            }
        })
        .collect()
}

fn periodic_hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()) as f32)
        .collect()
}

/// Box-decimate onto `target` Hz exactly as the recorder does (karaoke.rs
/// `Rec::push`), so separated vocals land on the recorded PCM's grid.
pub fn to_target_grid(samples: &[f32], rate_in: u32, target: u32) -> Vec<f32> {
    let mut out = Vec::with_capacity(samples.len() * target as usize / rate_in.max(1) as usize + 1);
    let (mut phase, mut acc, mut n) = (0u32, 0.0f32, 0u32);
    for &s in samples {
        let s = if s.is_finite() { s } else { 0.0 };
        phase = phase.saturating_add(target);
        acc += s;
        n += 1;
        if phase >= rate_in {
            phase -= rate_in;
            out.push(acc / n.max(1) as f32);
            acc = 0.0;
            n = 0;
        }
    }
    out
}

/// One global gain (peak 0.95), so chunk-to-chunk levels are preserved.
fn to_i16_normalized(x: &[f32]) -> Vec<i16> {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let gain = (0.95 / peak.max(1e-8)).min(1.0);
    x.iter()
        .map(|v| (v * gain * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect()
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    while term > 1e-12 * sum {
        term *= (x / (2.0 * k)).powi(2);
        sum += term;
        k += 1.0;
    }
    sum
}

/// Rational polyphase resampler: Kaiser-windowed sinc (beta 8.6, 32 zero
/// crossings), cutoff at 95% of the lower Nyquist. Zero group delay, so
/// output sample i sits at input time i × from/to.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let g = gcd(from as u64, to as u64);
    let (up, down) = ((to as u64 / g) as usize, (from as u64 / g) as usize);
    let cutoff = 0.95 * (up as f64 / down as f64).min(1.0);
    let half = (32.0 / cutoff).ceil() as isize;
    const BETA: f64 = 8.6;
    let i0b = bessel_i0(BETA);
    let taps = (2 * half) as usize;
    let mut table = vec![0.0f32; up * taps];
    for p in 0..up {
        let frac = p as f64 / up as f64;
        for (j, slot) in table[p * taps..(p + 1) * taps].iter_mut().enumerate() {
            let d = frac - (j as isize - half + 1) as f64;
            let r = d / half as f64;
            if r.abs() >= 1.0 {
                continue;
            }
            let arg = std::f64::consts::PI * cutoff * d;
            let sinc = if arg.abs() < 1e-12 {
                1.0
            } else {
                arg.sin() / arg
            };
            *slot = (cutoff * sinc * bessel_i0(BETA * (1.0 - r * r).sqrt()) / i0b) as f32;
        }
    }
    let out_len = (x.len() * up).div_ceil(down);
    let len = x.len() as isize;
    (0..out_len)
        .map(|i| {
            let pos = i * down;
            let base = (pos / up) as isize;
            let row = &table[(pos % up) * taps..(pos % up + 1) * taps];
            let mut acc = 0.0f32;
            for (j, &h) in row.iter().enumerate() {
                let k = base + j as isize - half + 1;
                if (0..len).contains(&k) {
                    acc += x[k as usize] * h;
                }
            }
            acc
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f64, rate: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / rate as f64).sin() as f32)
            .collect()
    }

    #[test]
    fn resample_preserves_a_tone_and_its_timing() {
        for (from, to) in [(48_000, 44_100), (44_100, 48_000), (16_000, 44_100)] {
            let x = tone(440.0, from, from as usize);
            let y = resample(&x, from, to);
            assert_eq!(y.len(), (x.len() * to as usize).div_ceil(from as usize));
            let expect = tone(440.0, to, y.len());
            // Away from the zero-padded edges the tone matches in phase.
            let mid = y.len() / 4..3 * y.len() / 4;
            let err = mid
                .clone()
                .map(|i| (y[i] - expect[i]).abs())
                .fold(0.0f32, f32::max);
            assert!(err < 2e-3, "{from}->{to}: max error {err}");
        }
    }

    #[test]
    fn resample_removes_content_above_the_new_nyquist() {
        let x = tone(23_000.0, 48_000, 48_000);
        let y = resample(&x, 48_000, 44_100);
        let mid = &y[y.len() / 4..3 * y.len() / 4];
        let rms = (mid.iter().map(|v| v * v).sum::<f32>() / mid.len() as f32).sqrt();
        assert!(rms < 1e-3, "aliased rms {rms}");
    }

    #[test]
    fn resample_round_trip_keeps_length_alignment() {
        let x = tone(1000.0, 48_000, 48_123);
        let mut back = resample(&resample(&x, 48_000, 44_100), 44_100, 48_000);
        back.resize(x.len(), 0.0);
        let mid = x.len() / 4..3 * x.len() / 4;
        let err = mid.map(|i| (back[i] - x[i]).abs()).fold(0.0f32, f32::max);
        assert!(err < 3e-3, "round trip error {err}");
    }

    #[test]
    fn target_grid_matches_recorder_lengths() {
        assert_eq!(
            to_target_grid(&vec![0.0; 48_000], 48_000, 16_000).len(),
            16_000
        );
        assert_eq!(
            to_target_grid(&vec![0.0; 44_100], 44_100, 16_000).len(),
            16_000
        );
        let ramp: Vec<f32> = (0..6).map(|i| i as f32).collect();
        assert_eq!(to_target_grid(&ramp, 48_000, 16_000), [1.0, 4.0]);
    }

    #[test]
    fn stft_round_trip_reconstructs_the_band_the_model_sees() {
        // No model: istft(stft(x)) must return x for content inside the
        // kept bins (below DIM_F, above bin 3), away from the chunk edges.
        let mut planner = FftPlanner::new();
        let (forward, inverse) = (
            planner.plan_fft_forward(N_FFT),
            planner.plan_fft_inverse(N_FFT),
        );
        let window = periodic_hann(N_FFT);
        let x: Vec<f32> = tone(440.0, MODEL_RATE, CHUNK)
            .iter()
            .zip(tone(3_000.0, MODEL_RATE, CHUNK))
            .map(|(a, b)| 0.5 * a + 0.25 * b)
            .collect();
        let mut spec = vec![0.0f32; 4 * DIM_F * DIM_T];
        stft_into(&*forward, &window, &x, &mut spec, 0);
        let y = istft(&*inverse, &window, &spec, 0);
        let err = (TRIM..CHUNK - TRIM)
            .map(|i| (y[i] - x[i]).abs())
            .fold(0.0f32, f32::max);
        assert!(err < 1e-3, "stft round trip error {err}");
    }

    #[test]
    fn normalization_uses_one_global_gain() {
        assert_eq!(to_i16_normalized(&[0.0, 2.0, -1.0]), [0, 31130, -15565]);
        assert_eq!(to_i16_normalized(&[0.1, -0.2]), [3277, -6554]);
    }
}
