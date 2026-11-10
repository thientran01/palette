//! A listen's acoustic model output ("emissions"), kept beside its saved
//! sync so a change that only touches the decoder can re-decode the song
//! without waiting for its next full listen. RECIPE bumps 2, 3 and 4 were
//! mostly decoder changes, and each one relearned every saved song one
//! listen at a time.
//!
//! A take is what one listen's aligner saw: every line window's emissions
//! on the mix and, for lines the review doubted, on the second ear's
//! separated vocals. Both ears' blend is derived from those two, so it is
//! not kept. Windows are keyed by ear and their exact sample range, so a
//! decoder change that also moves a window (a different margin, a new
//! syllable cap) finds it missing and the song relearns by listening, as
//! before. Nothing here ever guesses emissions.
//!
//! docs/specs/2026-11-10-karaoke-saved-emissions.md
use crate::align::TimeMap;
use serde::{Deserialize, Serialize};
use std::path::Path;

type Result<T> = std::result::Result<T, String>;

/// The model's classes per frame (acoustic.rs CLASSES).
pub const CLASSES: usize = 29;
/// Codes per nat: a log-probability is kept to within 1/2000 nat, from 0
/// down to -65.5 (the int8 model's output never goes below about -21). On
/// five of Thien's captures (1,380 words) a re-decode put every onset and
/// re-heard line where the live listen did; one word's end moved a frame
/// and 12 supports by 0.01. 8 bits, even spent mostly near 0, moved up to
/// 3% of words, one by 3s.
const SCALE: f32 = 1000.0;
const MAGIC: &[u8; 4] = b"PTK1";

/// Which ear a window's emissions came from.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Ear {
    Mix,
    Vocals,
}

/// One line window's emissions as the aligner decoded them.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub ear: Ear,
    /// First recording sample of the window (16 kHz).
    pub begin: usize,
    /// Window length in samples.
    pub len: usize,
    /// `CLASSES` codes per frame (`encode`).
    pub codes: Vec<u16>,
}

#[derive(Serialize, Deserialize)]
struct WindowHeader {
    ear: Ear,
    begin: usize,
    len: usize,
    frames: usize,
}

/// Everything one listen's decode depends on besides the lyrics: the
/// recording's length and time map, and its emissions.
#[derive(Clone, Debug, PartialEq)]
pub struct Take {
    pub samples: usize,
    pub map: TimeMap,
    pub windows: Vec<Window>,
    /// A partial listen's: only lines it held whole were decoded
    /// (`acoustic::EdgeCut`), and a re-decode leaves the rest the same way.
    pub whole_windows: bool,
}

#[derive(Serialize, Deserialize)]
struct TakeHeader {
    samples: usize,
    map: TimeMap,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    whole_windows: bool,
    windows: Vec<WindowHeader>,
}

impl Take {
    pub fn new(samples: usize, map: TimeMap) -> Self {
        Self {
            samples,
            map,
            windows: Vec::new(),
            whole_windows: false,
        }
    }

    /// Keep a window's emissions. A window decoded twice (the same line
    /// heard again) keeps the first: both came from the same model input.
    pub fn record(&mut self, ear: Ear, begin: usize, len: usize, logp: &[f32]) {
        if self.find(ear, begin, len).is_none() {
            self.windows.push(Window {
                ear,
                begin,
                len,
                codes: encode(logp),
            });
        }
    }

    fn find(&self, ear: Ear, begin: usize, len: usize) -> Option<&Window> {
        self.windows
            .iter()
            .find(|w| w.ear == ear && w.begin == begin && w.len == len)
    }

    /// The emissions saved for exactly this window, if any.
    pub fn emissions(&self, ear: Ear, begin: usize, len: usize) -> Option<Vec<f32>> {
        self.find(ear, begin, len).map(|w| decode(&w.codes))
    }

    pub fn bytes(&self) -> usize {
        self.windows.iter().map(|w| w.codes.len() * 2).sum()
    }
}

/// One code per log-probability (`SCALE`); -inf and NaN keep the floor.
pub fn encode(logp: &[f32]) -> Vec<u16> {
    let floor = u16::MAX as f32;
    logp.iter()
        .map(|&x| {
            let q = if x.is_finite() { -x * SCALE } else { floor };
            q.round().clamp(0.0, floor) as u16
        })
        .collect()
}

/// Log-probabilities back from `encode`'s codes. Every value is finite.
pub fn decode(codes: &[u16]) -> Vec<f32> {
    codes.iter().map(|&q| -(q as f32) / SCALE).collect()
}

/// A saved sync's takes, one per listen merged into it, oldest first, and
/// what they are valid for.
#[derive(Clone, Debug, PartialEq)]
pub struct TakeFile {
    /// `acoustic::EMISSIONS` when they were heard: the model and its input.
    pub emissions: String,
    /// `lyrics_id` of the synced lyrics they were aligned to.
    pub lyrics: u64,
    /// The saved sync's `listens` these takes produced.
    pub listens: u32,
    /// For the karaoke-ready event a re-decode sends.
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration_ms: i64,
    pub takes: Vec<Take>,
}

#[derive(Serialize, Deserialize)]
struct FileHeader {
    emissions: String,
    lyrics: u64,
    listens: u32,
    artist: String,
    title: String,
    album: String,
    duration_ms: i64,
    takes: Vec<TakeHeader>,
}

/// FNV-1a over the synced lyrics: whether a take was aligned to the lyrics
/// a saved sync carries.
pub fn lyrics_id(synced: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in synced.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl TakeFile {
    /// Magic, header length, JSON header, then every window's codes
    /// (little-endian u16) in header order.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let header = FileHeader {
            emissions: self.emissions.clone(),
            lyrics: self.lyrics,
            listens: self.listens,
            artist: self.artist.clone(),
            title: self.title.clone(),
            album: self.album.clone(),
            duration_ms: self.duration_ms,
            takes: self
                .takes
                .iter()
                .map(|t| TakeHeader {
                    samples: t.samples,
                    whole_windows: t.whole_windows,
                    map: t.map.clone(),
                    windows: t
                        .windows
                        .iter()
                        .map(|w| WindowHeader {
                            ear: w.ear,
                            begin: w.begin,
                            len: w.len,
                            frames: w.codes.len() / CLASSES,
                        })
                        .collect(),
                })
                .collect(),
        };
        let json = serde_json::to_vec(&header).map_err(|e| e.to_string())?;
        let payload: usize = self.takes.iter().map(Take::bytes).sum();
        let mut out = Vec::with_capacity(8 + json.len() + payload);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&json);
        for take in &self.takes {
            for w in &take.windows {
                out.extend(w.codes.iter().flat_map(|q| q.to_le_bytes()));
            }
        }
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let malformed = || "malformed take file".to_string();
        if bytes.len() < 8 || &bytes[..4] != MAGIC {
            return Err(malformed());
        }
        let n = u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| malformed())?) as usize;
        let json = bytes.get(8..8 + n).ok_or_else(malformed)?;
        let header: FileHeader = serde_json::from_slice(json).map_err(|e| e.to_string())?;
        let mut at = 8 + n;
        let mut takes = Vec::with_capacity(header.takes.len());
        for t in header.takes {
            let mut windows = Vec::with_capacity(t.windows.len());
            for w in t.windows {
                let size = w.frames.checked_mul(CLASSES * 2).ok_or_else(malformed)?;
                let codes = bytes
                    .get(at..at + size)
                    .ok_or_else(malformed)?
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&b| u16::from_le_bytes(b))
                    .collect();
                if w.frames == 0 {
                    return Err(malformed());
                }
                at += size;
                windows.push(Window {
                    ear: w.ear,
                    begin: w.begin,
                    len: w.len,
                    codes,
                });
            }
            takes.push(Take {
                samples: t.samples,
                whole_windows: t.whole_windows,
                map: t.map,
                windows,
            });
        }
        if at != bytes.len() {
            return Err(malformed());
        }
        Ok(Self {
            emissions: header.emissions,
            lyrics: header.lyrics,
            listens: header.listens,
            artist: header.artist,
            title: header.title,
            album: header.album,
            duration_ms: header.duration_ms,
            takes,
        })
    }

    pub fn read(path: &Path) -> Option<Self> {
        Self::from_bytes(&std::fs::read(path).ok()?).ok()
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        crate::settings::write_atomic(path, &self.to_bytes()?).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> TimeMap {
        TimeMap::from_origin(1_000, 16_000)
    }

    #[test]
    fn codes_keep_log_probabilities_to_half_a_step() {
        let logp = [0.0, -0.0004, -0.01, -0.1, -1.0, -5.0, -20.0, -65.0];
        let back = decode(&encode(&logp));
        for (&x, &y) in logp.iter().zip(&back) {
            assert!((x - y).abs() <= 0.5 / SCALE + 1e-6, "{x} came back {y}");
        }
        let floor = -(u16::MAX as f32) / SCALE;
        assert_eq!(
            decode(&encode(&[f32::NEG_INFINITY, f32::NAN, -90.0, 0.5])),
            [floor, floor, floor, 0.0]
        );
    }

    #[test]
    fn take_files_round_trip_and_find_exact_windows() {
        let mut take = Take::new(160_000, map());
        let logp: Vec<f32> = (0..CLASSES * 3).map(|i| -(i as f32) / 10.0).collect();
        take.record(Ear::Mix, 8_000, 32_000, &logp);
        take.record(Ear::Vocals, 8_000, 32_000, &logp[..CLASSES * 2]);
        // The same window again keeps the first.
        take.record(Ear::Mix, 8_000, 32_000, &logp[..CLASSES]);
        assert_eq!(take.windows.len(), 2);
        let file = TakeFile {
            emissions: "e/1".into(),
            lyrics: lyrics_id("[00:01.00]hello"),
            listens: 2,
            artist: "a".into(),
            title: "t".into(),
            album: "".into(),
            duration_ms: 10_000,
            takes: vec![take.clone(), Take::new(5, map()), {
                // A partial listen's take says so.
                let mut partial = Take::new(7, map());
                partial.whole_windows = true;
                partial
            }],
        };
        let back = TakeFile::from_bytes(&file.to_bytes().unwrap()).unwrap();
        assert_eq!(back, file);
        let mix = back.takes[0].emissions(Ear::Mix, 8_000, 32_000).unwrap();
        assert_eq!(mix.len(), CLASSES * 3);
        assert!(back.takes[0].emissions(Ear::Mix, 8_000, 32_001).is_none());
        assert!(back.takes[0]
            .emissions(Ear::Vocals, 8_001, 32_000)
            .is_none());
        assert_eq!(
            back.takes[0]
                .emissions(Ear::Vocals, 8_000, 32_000)
                .unwrap()
                .len(),
            CLASSES * 2
        );
    }

    #[test]
    fn damaged_take_files_are_refused() {
        let mut take = Take::new(16_000, map());
        take.record(Ear::Mix, 0, 16_000, &[-1.0; CLASSES * 4]);
        let file = TakeFile {
            emissions: "e/1".into(),
            lyrics: 1,
            listens: 1,
            artist: String::new(),
            title: String::new(),
            album: String::new(),
            duration_ms: 0,
            takes: vec![take],
        };
        let bytes = file.to_bytes().unwrap();
        assert!(TakeFile::from_bytes(&bytes[..bytes.len() - 1]).is_err());
        let mut long = bytes.clone();
        long.push(0);
        assert!(TakeFile::from_bytes(&long).is_err());
        assert!(TakeFile::from_bytes(b"PTK0\0\0\0\0").is_err());
        assert!(TakeFile::from_bytes(&[]).is_err());
    }
}
