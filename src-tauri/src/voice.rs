//! The singer's voice on held notes: pitch and loudness of the separated
//! vocal every 10 ms, saved with each held word so the lyric highlight can
//! show vibrato, slides and swells (src/lib/voiceMotion.ts). Thien,
//! 2026-11-08: "emphasize ... emotion or dragged notes or vibrato ... make
//! the karaoke lyrics feel like the singer is really singing."
//!
//! Only held words (`HELD_MS` or longer) get a track: on a short syllable
//! the frontend never follows pitch, because that would bring back the
//! syllable-rate jitter this work removes. Pitch needs the separated vocal:
//! on the full mix a pitch tracker agreed with the vocal's pitch on 62% of
//! voiced frames for a sparse ballad (Laufey "Haunted") but 12% for K-pop
//! (tripleS "Baby Flower"). So the worker separates just the stretches
//! around held notes with the review's second ear, longest notes first,
//! within a pass budget. Words left over keep the plain lift.
//!
//! docs/specs/2026-11-08-karaoke-sung-highlight.md
use crate::{
    acoustic::SecondEar,
    align::{TimeMap, Word},
};
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Sung at least this long: a held note. Must match src/lib/voiceMotion.ts HELD_MS.
pub const HELD_MS: i64 = 450;
/// Frame step of a track.
pub const HOP_MS: i64 = 10;
/// A track runs past the word's end by the frontend's release.
const TAIL_MS: i64 = 120;
/// Separator passes the voice may spend per song (~6.4s each on Thien's PC).
pub const PASS_BUDGET: usize = 10;
/// Unvoiced frame in `c`.
pub const UNVOICED: i16 = i16::MIN;
/// Pitch search range (Hz) and YIN threshold.
const F0_MIN: f64 = 70.0;
const F0_MAX: f64 = 1000.0;
const YIN_THRESHOLD: f64 = 0.15;
/// Analysis windows in samples at 16 kHz: 64 ms for pitch, 40 ms loudness.
const PITCH_WIN: usize = 1024;
const LOUD_WIN: usize = 640;

/// One held word's voice, from its onset, every `HOP_MS`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct VoiceTrack {
    /// Pitch in cents from A440, smoothed; `UNVOICED` where none.
    pub c: Vec<i16>,
    /// Loudness 0-100 against the song's held notes, smoothed. Saved for
    /// later use; the Rise highlight reads only pitch.
    pub l: Vec<u8>,
}

pub fn is_held(w: &Word) -> bool {
    w.end.is_some_and(|end| end - w.t >= HELD_MS)
}

/// Per-frame raw analysis before the song-wide loudness scale.
struct Raw {
    word: usize,
    db: Vec<f64>,
    f0: Vec<f64>,
}

/// Attach voice tracks to held words, hearing at most `budget` separator
/// passes. Returns how many words got one. A failed or late `hear` (the
/// ear's own deadline) leaves the rest without, never fails the sync.
pub fn listen(
    words: &mut [Word],
    map: &TimeMap,
    len: usize,
    hz: u32,
    ear: &mut dyn SecondEar,
    budget: usize,
) -> usize {
    let span = |w: &Word| -> Range<usize> {
        let a = map.sample_at(w.t).min(len);
        let b = map.sample_at(w.end.unwrap_or(w.t) + TAIL_MS).min(len);
        a..b
    };
    let held: Vec<usize> = (0..words.len())
        .filter(|&i| is_held(&words[i]) && !span(&words[i]).is_empty())
        .collect();
    // Neighbouring notes share one stretch when their separator windows
    // (a second of context each side) would overlap anyway. A note the ear
    // already heard (the review's stretches) costs nothing and stays alone.
    let mut stretches: Vec<(Range<usize>, Vec<usize>, i64)> = Vec::new();
    for &i in &held {
        let r = span(&words[i]);
        let sung = words[i].end.unwrap_or(words[i].t) - words[i].t;
        if ear.passes(r.clone()) == 0 {
            stretches.push((r, vec![i], sung));
            continue;
        }
        match stretches.last_mut() {
            Some((last, members, total))
                if r.start <= last.end + 2 * hz as usize && ear.passes(last.clone()) > 0 =>
            {
                last.end = last.end.max(r.end);
                members.push(i);
                *total += sung;
            }
            _ => stretches.push((r, vec![i], sung)),
        }
    }
    // Free notes, then most held singing per pass.
    stretches.sort_by(|a, b| {
        let per = |s: &(Range<usize>, Vec<usize>, i64)| match ear.passes(s.0.clone()) {
            0 => f64::INFINITY,
            n => s.2 as f64 / n as f64,
        };
        per(b).total_cmp(&per(a))
    });
    let mut spent = 0;
    let mut raws: Vec<Raw> = Vec::new();
    for (range, members, _) in stretches {
        let cost = ear.passes(range.clone());
        if spent + cost > budget {
            continue;
        }
        let vocals = match ear.hear(range.clone()) {
            Ok(v) if v.len() == range.len() => v,
            Ok(_) => continue,
            Err(e) => {
                log::info!("karaoke: voice stopped ({e})");
                break;
            }
        };
        spent += cost;
        let x: Vec<f64> = vocals.iter().map(|&v| v as f64 / 32768.0).collect();
        for i in members {
            let w = &words[i];
            let frames = ((w.end.unwrap_or(w.t) + TAIL_MS - w.t) / HOP_MS).max(1) as usize;
            let mut db = Vec::with_capacity(frames);
            let mut f0 = Vec::with_capacity(frames);
            for k in 0..frames {
                let at = map.sample_at(w.t + k as i64 * HOP_MS);
                let c = at.saturating_sub(range.start);
                db.push(loudness_db(&x, c));
                f0.push(pitch_hz(&x, c, hz));
            }
            raws.push(Raw { word: i, db, f0 });
        }
    }
    finish(words, raws)
}

/// Song-wide loudness scale (20th..95th percentile of voiced frames), then
/// smoothing, then cents.
fn finish(words: &mut [Word], raws: Vec<Raw>) -> usize {
    let mut voiced: Vec<f64> = raws
        .iter()
        .flat_map(|r| {
            r.db.iter()
                .zip(&r.f0)
                .filter(|(_, &f)| f > 0.0)
                .map(|(&d, _)| d)
        })
        .collect();
    if voiced.is_empty() {
        return 0;
    }
    voiced.sort_by(f64::total_cmp);
    let lo = voiced[voiced.len() * 20 / 100];
    let hi = voiced[(voiced.len() * 95 / 100).min(voiced.len() - 1)];
    let range = (hi - lo).max(1.0);
    let mut count = 0;
    for r in raws {
        let mut level = 0.0;
        let l: Vec<u8> =
            r.db.iter()
                .map(|&d| {
                    // Quick attack, slow release: a swell reads, a consonant
                    // dip doesn't.
                    let x = ((d - lo) / range).clamp(0.0, 1.0);
                    level += (x - level) * if x > level { 0.35 } else { 0.06 };
                    (level * 100.0).round() as u8
                })
                .collect();
        let cents = fold_octaves(
            r.f0.iter()
                .map(|&f| (f > 0.0).then(|| 1200.0 * (f / 440.0).log2()))
                .collect(),
        );
        let c: Vec<i16> = (0..cents.len())
            .map(|k| {
                let Some(mid) = cents[k] else {
                    return UNVOICED;
                };
                // 30 ms mean over voiced neighbours.
                let around = [k.checked_sub(1), Some(k), Some(k + 1)];
                let vals: Vec<f64> = around
                    .iter()
                    .filter_map(|j| j.and_then(|j| cents.get(j).copied().flatten()))
                    .collect();
                let mean = if vals.is_empty() {
                    mid
                } else {
                    vals.iter().sum::<f64>() / vals.len() as f64
                };
                mean.round().clamp(-6000.0, 6000.0) as i16
            })
            .collect();
        if c.iter().any(|&v| v != UNVOICED) {
            words[r.word].voice = Some(VoiceTrack { c, l });
            count += 1;
        }
    }
    count
}

/// YIN sometimes locks onto an octave above or below the voice. Within one
/// held note, a frame more than a tritone from the note's median is moved an
/// octave toward it, and dropped if that doesn't bring it close; otherwise a
/// one-frame octave error would read as a 2px jolt.
fn fold_octaves(cents: Vec<Option<f64>>) -> Vec<Option<f64>> {
    let mut voiced: Vec<f64> = cents.iter().flatten().copied().collect();
    if voiced.is_empty() {
        return cents;
    }
    voiced.sort_by(f64::total_cmp);
    let median = voiced[voiced.len() / 2];
    cents
        .into_iter()
        .map(|c| {
            let c = c?;
            let folded = c - 1200.0 * ((c - median) / 1200.0).round();
            ((folded - median).abs() <= 600.0).then_some(folded)
        })
        .collect()
}

/// RMS level in dB of the 40 ms window centered on sample `c`.
fn loudness_db(x: &[f64], c: usize) -> f64 {
    let a = c.saturating_sub(LOUD_WIN / 2).min(x.len());
    let b = (a + LOUD_WIN).min(x.len());
    if b <= a {
        return -120.0;
    }
    let ms = x[a..b].iter().map(|v| v * v).sum::<f64>() / (b - a) as f64;
    10.0 * (ms + 1e-12).log10()
}

/// YIN fundamental (Hz) of the window centered on sample `c`, or 0.
fn pitch_hz(x: &[f64], c: usize, hz: u32) -> f64 {
    let sr = hz as f64;
    let tmin = (sr / F0_MAX).floor() as usize;
    let tmax = (sr / F0_MIN).ceil() as usize;
    let Some(a) = c.checked_sub(PITCH_WIN / 2) else {
        return 0.0;
    };
    if a + PITCH_WIN + tmax > x.len() {
        return 0.0;
    }
    let frame = &x[a..a + PITCH_WIN + tmax];
    let energy = frame[..PITCH_WIN].iter().map(|v| v * v).sum::<f64>() / PITCH_WIN as f64;
    if energy < 1e-6 {
        return 0.0;
    }
    // Difference function and its cumulative-mean normalization.
    let mut cmnd = vec![1.0; tmax + 1];
    let mut running = 0.0;
    for tau in 1..=tmax {
        let d: f64 = (0..PITCH_WIN)
            .map(|j| {
                let e = frame[j] - frame[j + tau];
                e * e
            })
            .sum();
        running += d;
        cmnd[tau] = if running > 0.0 {
            d * tau as f64 / running
        } else {
            1.0
        };
    }
    let Some(mut tau) = (tmin..=tmax).find(|&t| cmnd[t] < YIN_THRESHOLD) else {
        return 0.0;
    };
    while tau < tmax && cmnd[tau + 1] < cmnd[tau] {
        tau += 1;
    }
    // Parabolic refinement.
    let refined = if tau > 0 && tau < tmax {
        let (p, q, r) = (cmnd[tau - 1], cmnd[tau], cmnd[tau + 1]);
        let den = p - 2.0 * q + r;
        if den.abs() > 1e-12 {
            tau as f64 + 0.5 * (p - r) / den
        } else {
            tau as f64
        }
    } else {
        tau as f64
    };
    sr / refined
}

#[cfg(test)]
mod tests {
    use super::*;

    const HZ: u32 = 16_000;

    /// A sung tone with vibrato: `f` Hz, ±`depth` cents at 5.5 Hz.
    fn tone(f: f64, depth: f64, secs: f64, amp: f64) -> Vec<i16> {
        let n = (secs * HZ as f64) as usize;
        let mut phase = 0.0f64;
        (0..n)
            .map(|i| {
                let t = i as f64 / HZ as f64;
                let cents = depth * (2.0 * std::f64::consts::PI * 5.5 * t).sin();
                phase += 2.0 * std::f64::consts::PI * f * 2f64.powf(cents / 1200.0) / HZ as f64;
                // A little harmonic content, like a voice.
                ((phase.sin() + 0.4 * (2.0 * phase).sin()) * amp * 20000.0) as i16
            })
            .collect()
    }

    struct Ear {
        audio: Vec<i16>,
        heard: usize,
        /// Already separated, as the review leaves `VocalEar`: free.
        kept: Option<Range<usize>>,
    }
    impl Ear {
        fn new(audio: Vec<i16>) -> Self {
            Self {
                audio,
                heard: 0,
                kept: None,
            }
        }
        fn free(&self, range: &Range<usize>) -> bool {
            self.kept
                .as_ref()
                .is_some_and(|k| k.start <= range.start && range.end <= k.end)
        }
    }
    impl SecondEar for Ear {
        fn hear(&mut self, range: Range<usize>) -> Result<Vec<i16>, String> {
            if !self.free(&range) {
                self.heard += 1;
            }
            Ok(self.audio[range].to_vec())
        }
        fn passes(&self, range: Range<usize>) -> usize {
            if self.free(&range) {
                return 0;
            }
            crate::separation::passes(range.len() + 2 * HZ as usize, HZ)
        }
    }

    fn map() -> TimeMap {
        TimeMap::from_origin(0, HZ)
    }

    fn word(t: i64, end: i64) -> Word {
        Word {
            t,
            end: Some(end),
            text: "kiss".into(),
            ..Default::default()
        }
    }

    #[test]
    fn hears_pitch_and_vibrato_on_a_held_note() {
        let audio = tone(220.0, 70.0, 4.0, 0.5);
        let mut words = vec![word(500, 450 + 1600), word(2300, 2400)];
        let mut ear = Ear::new(audio);
        let n = listen(&mut words, &map(), 64_000, HZ, &mut ear, PASS_BUDGET);
        assert_eq!(n, 1);
        assert!(words[1].voice.is_none(), "short syllables get no track");
        let v = words[0].voice.as_ref().unwrap();
        assert_eq!(v.c.len(), v.l.len());
        assert_eq!(v.c.len(), ((1550 + TAIL_MS) / HOP_MS) as usize);
        // 220 Hz is -1200 cents from A440; the vibrato swings about ±70.
        let voiced: Vec<i16> = v.c.iter().copied().filter(|&c| c != UNVOICED).collect();
        assert!(voiced.len() > v.c.len() * 9 / 10);
        let lo = *voiced.iter().min().unwrap();
        let hi = *voiced.iter().max().unwrap();
        assert!((-1300..=-1240).contains(&lo), "{lo}");
        assert!((-1160..=-1100).contains(&hi), "{hi}");
    }

    #[test]
    fn folds_octave_slips_back_onto_the_note() {
        let mut c: Vec<Option<f64>> = vec![Some(-600.0); 20];
        c[5] = Some(600.0);
        c[9] = Some(-1790.0);
        c[12] = None;
        let f = fold_octaves(c);
        assert_eq!(f[5], Some(-600.0));
        assert_eq!(f[9], Some(-590.0));
        assert_eq!(f[12], None);
    }

    #[test]
    fn loudness_follows_a_swell() {
        let mut audio = tone(330.0, 0.0, 3.0, 1.0);
        for (i, s) in audio.iter_mut().enumerate() {
            *s = (*s as f64 * (0.1 + 0.9 * i as f64 / 48_000.0)) as i16;
        }
        let mut words = vec![word(300, 2700)];
        listen(
            &mut words,
            &map(),
            48_000,
            HZ,
            &mut Ear::new(audio),
            PASS_BUDGET,
        );
        let l = &words[0].voice.as_ref().unwrap().l;
        assert!(l[20] < l[120] && l[120] < l[220], "{:?}", &l[..]);
    }

    #[test]
    fn spends_no_more_than_the_budget_and_skips_silence() {
        let mut audio = tone(220.0, 0.0, 60.0, 0.5);
        audio[16_000 * 30..].fill(0);
        let mut words: Vec<Word> = (0..12)
            .map(|k| word(k * 5000 + 500, k * 5000 + 1500))
            .collect();
        let mut ear = Ear::new(audio);
        let n = listen(&mut words, &map(), 960_000, HZ, &mut ear, 4);
        assert!(ear.heard <= 4);
        assert!(n <= 4);
        // Silence has no pitch, so no track.
        assert!(words[11].voice.is_none());
    }

    #[test]
    fn notes_the_review_already_heard_cost_no_passes() {
        let audio = tone(220.0, 0.0, 60.0, 0.5);
        let mut words: Vec<Word> = (0..12)
            .map(|k| word(k * 5000 + 500, k * 5000 + 1500))
            .collect();
        // The review separated the first half minute: six notes.
        let mut ear = Ear::new(audio);
        ear.kept = Some(0..16_000 * 30);
        let n = listen(&mut words, &map(), 960_000, HZ, &mut ear, 4);
        assert!(ear.heard <= 4);
        assert_eq!(n, 10, "six free notes and four paid");
        assert!(words[..6].iter().all(|w| w.voice.is_some()));
    }
}
