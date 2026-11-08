//! The review's checker: how sure the model is of each decoded word.
//!
//! Viterbi keeps the single best path. Where two placements of a word score
//! almost alike (a faint first copy of a repeated phrase against a louder
//! later one, a held vowel, a backing-vocal adlib), it commits to one of
//! them on a hair: the same recording decoded on two CPUs moved such words
//! by up to three seconds (Thien's dumps, 2026-11-08). CTC forward-backward
//! over the same emissions gives, for every letter, the probability over
//! all paths that it starts on each frame. The share of that probability
//! near the frame Viterbi chose is the word's support.
//!
//! On JamendoLyrics (mix, 21,580 words) support predicts error well: words
//! under 0.3 were more than 300 ms off 55% of the time, words over 0.9
//! (84% of all) 0.8% of the time. Moving a doubted word to its posterior
//! median instead made the set slightly worse (p90 125 → 133 ms), so the
//! checker only flags; the second ear (`acoustic::SecondEar`) repairs.
//! docs/specs/2026-11-08-karaoke-sync-review.md.
use super::{EntryPrior, Result};

const NEG: f32 = f32::NEG_INFINITY;

fn lse(a: f32, b: f32) -> f32 {
    if a == NEG {
        return b;
    }
    if b == NEG {
        return a;
    }
    let m = a.max(b);
    m + ((a - m).exp() + (b - m).exp()).ln()
}

/// `start[k * frames + t]`: the probability, over all CTC paths, that
/// target k's first frame is t.
pub(super) struct Posterior {
    frames: usize,
    start: Vec<f32>,
}

impl Posterior {
    /// The same lattice and entry prior as `ctc_spans_with_prior`.
    pub(super) fn new(
        logp: &[f32],
        classes: usize,
        targets: &[usize],
        prior: Option<EntryPrior>,
    ) -> Result<Self> {
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
        let states = targets.len() * 2 + 1;
        let cells = frames
            .checked_mul(states)
            .filter(|&n| n <= 4_000_000)
            .ok_or("CTC window exceeds memory bound")?;
        let label = |s: usize| {
            if s.is_multiple_of(2) {
                0
            } else {
                targets[s / 2]
            }
        };
        let emit = |t: usize, s: usize| logp[t * classes + label(s)];
        // Entering target state s at frame t pays the lateness cost the
        // Viterbi charges, once, on entry.
        let entry = |s: usize, t: usize| match prior {
            Some(p) if s == p.target * 2 + 1 => {
                ((t as f64 - p.deadline).max(0.0) as f32) * p.penalty_per_frame
            }
            _ => 0.0,
        };
        // Skipping the blank between two targets needs them to differ.
        let skip = |s: usize| s > 1 && s % 2 == 1 && targets[s / 2] != targets[s / 2 - 1];

        let mut alpha = vec![NEG; cells];
        alpha[0] = emit(0, 0);
        alpha[1] = emit(0, 1);
        for t in 1..frames {
            let (done, row) = alpha.split_at_mut(t * states);
            let prev = &done[(t - 1) * states..];
            for (s, cell) in row[..states].iter_mut().enumerate() {
                let mut a = prev[s];
                if s > 0 {
                    a = lse(a, prev[s - 1] - entry(s, t));
                }
                if skip(s) {
                    a = lse(a, prev[s - 2] - entry(s, t));
                }
                *cell = a + emit(t, s);
            }
        }
        let last = &alpha[(frames - 1) * states..];
        let z = lse(last[states - 1], last[states - 2]);
        if !z.is_finite() {
            return Err("no CTC path".into());
        }

        let n = targets.len();
        let mut start = vec![0f32; n * frames];
        let mut beta_next = vec![NEG; states];
        let mut beta = vec![NEG; states];
        for t in (0..frames).rev() {
            if t + 1 == frames {
                beta.fill(NEG);
                beta[states - 1] = 0.0;
                beta[states - 2] = 0.0;
            } else {
                for s in 0..states {
                    let mut b = emit(t + 1, s) + beta_next[s];
                    if s + 1 < states {
                        b = lse(
                            b,
                            emit(t + 1, s + 1) + beta_next[s + 1] - entry(s + 1, t + 1),
                        );
                    }
                    if s + 2 < states && skip(s + 2) {
                        b = lse(
                            b,
                            emit(t + 1, s + 2) + beta_next[s + 2] - entry(s + 2, t + 1),
                        );
                    }
                    beta[s] = b;
                }
            }
            for k in 0..n {
                let s = 2 * k + 1;
                // Starts at t: in s now, somewhere else a frame earlier.
                let into = if t == 0 {
                    if s == 1 {
                        alpha[s] + beta[s]
                    } else {
                        NEG
                    }
                } else {
                    let prev = &alpha[(t - 1) * states..t * states];
                    let mut a = prev[s - 1] - entry(s, t);
                    if skip(s) {
                        a = lse(a, prev[s - 2] - entry(s, t));
                    }
                    a + emit(t, s) + beta[s]
                };
                start[k * frames + t] = (into - z).exp();
            }
            std::mem::swap(&mut beta, &mut beta_next);
        }
        Ok(Self { frames, start })
    }

    /// Posterior mass of target k starting within `radius` frames of `frame`.
    pub(super) fn start_mass(&self, k: usize, frame: usize, radius: usize) -> f32 {
        let row = &self.start[k * self.frames..(k + 1) * self.frames];
        let lo = frame.saturating_sub(radius).min(self.frames);
        let hi = (frame + radius + 1).min(self.frames);
        row[lo..hi].iter().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acoustic::ctc_spans;

    fn scores(rows: &[&[f32]]) -> Vec<f32> {
        rows.iter()
            .flat_map(|r| {
                let z = r.iter().map(|x| x.exp()).sum::<f32>().ln();
                r.iter().map(move |x| x - z)
            })
            .collect()
    }

    fn total(post: &Posterior, k: usize) -> f32 {
        post.start_mass(k, 0, post.frames)
    }

    #[test]
    fn a_clear_path_holds_all_its_support() {
        // Blank, a, blank, b, blank with a clear winner every frame.
        let logp = scores(&[
            &[5.0, 0.0, 0.0],
            &[0.0, 5.0, 0.0],
            &[5.0, 0.0, 0.0],
            &[0.0, 0.0, 5.0],
            &[5.0, 0.0, 0.0],
        ]);
        let targets = [1, 2];
        let post = Posterior::new(&logp, 3, &targets, None).unwrap();
        let viterbi = ctc_spans(&logp, 3, &targets).unwrap();
        for (k, span) in viterbi.iter().enumerate() {
            assert!((total(&post, k) - 1.0).abs() < 1e-3);
            assert!(post.start_mass(k, span.start, 0) > 0.9);
        }
    }

    #[test]
    fn a_near_tie_splits_the_support() {
        // "a" is heard at frame 1 slightly better than at frame 4, with
        // silence between: Viterbi takes frame 1 and the posterior doubts it.
        let logp = scores(&[
            &[3.0, 0.0],
            &[0.0, 3.2],
            &[3.0, 0.0],
            &[3.0, 0.0],
            &[0.0, 3.0],
            &[3.0, 0.0],
        ]);
        let post = Posterior::new(&logp, 2, &[1], None).unwrap();
        assert_eq!(ctc_spans(&logp, 2, &[1]).unwrap()[0].start, 1);
        assert!((total(&post, 0) - 1.0).abs() < 1e-3);
        let here = post.start_mass(0, 1, 0);
        assert!(here > 0.3 && here < 0.8, "{here}");
        assert!(post.start_mass(0, 4, 0) > 0.2);
    }

    #[test]
    fn the_entry_prior_moves_support_early() {
        let logp = scores(&[
            &[3.0, 0.0],
            &[0.0, 3.0],
            &[3.0, 0.0],
            &[3.0, 0.0],
            &[0.0, 3.4],
            &[3.0, 0.0],
        ]);
        let free = Posterior::new(&logp, 2, &[1], None).unwrap();
        let prior = EntryPrior {
            target: 0,
            deadline: 2.0,
            penalty_per_frame: 1.0,
        };
        let guided = Posterior::new(&logp, 2, &[1], Some(prior)).unwrap();
        assert!(guided.start_mass(0, 1, 0) > free.start_mass(0, 1, 0));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Posterior::new(&[], 2, &[1], None).is_err());
        assert!(Posterior::new(&[0.0, f32::NAN], 2, &[1], None).is_err());
        assert!(Posterior::new(&[0.0, 0.0], 2, &[0], None).is_err());
        assert!(Posterior::new(&[0.0, 0.0], 2, &[2], None).is_err());
    }
}
