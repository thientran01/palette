//! One bounded acoustic worker, with an idle-expiring model. Requests already
//! run behind karaoke's ALIGNING guard; no inference touches the media thread.
use crate::{
    acoustic::{AcousticAligner, EdgeCut, Knobs},
    align::{TimeMap, TimedLine, Word},
    separation::{Stereo, VocalEar, VocalSeparator},
    takes::Take,
};
use std::{
    path::PathBuf,
    sync::{mpsc, Mutex, OnceLock},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, String>;
const IDLE: Duration = Duration::from_secs(5 * 60);
/// The review's second ear starts no stretch this long after the align
/// began. Every doubted line of a four-minute song is at most about 43
/// passes, roughly 275s on Thien's PC, so only a busy PC (a pass several
/// times slower) or a very long song reaches it; lines left keep the mix's
/// timing.
const EAR_DEADLINE: Duration = Duration::from_secs(360);
/// The voice pass on held notes (voice.rs) starts no stretch this long
/// after it began. Its budget is about 64s on Thien's PC.
const VOICE_DEADLINE: Duration = Duration::from_secs(90);
struct Request {
    pcm: Vec<i16>,
    /// Full-rate stereo kept for separation (the vocal trial, or the
    /// review's second ear); never aligned directly.
    stereo: Option<Stereo>,
    lines: Vec<TimedLine>,
    map: TimeMap,
    /// A partial listen: decode only lines it holds whole.
    whole_windows: bool,
    dir: PathBuf,
    reply: mpsc::SyncSender<Result<Aligned>>,
}

/// One listen's words, and the take (takes.rs) a decoder change can
/// re-decode them from. No take on the vocal trial, whose emissions came
/// from whole-song separated vocals.
pub struct Aligned {
    pub words: Vec<Word>,
    pub take: Option<Take>,
    /// Lines left because the recording didn't hold them whole.
    pub cut: EdgeCut,
}
struct Connection<T> {
    generation: u64,
    sender: Option<T>,
}
impl<T: Clone> Connection<T> {
    fn connect(&mut self, start: impl FnOnce() -> Result<T>) -> Result<(u64, T)> {
        if self.sender.is_none() {
            let sender = start()?; // Failed startup remains retryable.
            self.generation += 1;
            self.sender = Some(sender);
        }
        Ok((self.generation, self.sender.as_ref().unwrap().clone()))
    }
    fn disconnect(&mut self, generation: u64) {
        // A late failure from an old worker cannot clear its replacement.
        if self.generation == generation {
            self.sender = None;
        }
    }
}
static WORKER: OnceLock<Mutex<Connection<mpsc::SyncSender<Request>>>> = OnceLock::new();
fn connection() -> std::sync::MutexGuard<'static, Connection<mpsc::SyncSender<Request>>> {
    WORKER
        .get_or_init(|| {
            Mutex::new(Connection {
                generation: 0,
                sender: None,
            })
        })
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn align(
    pcm: Vec<i16>,
    stereo: Option<Stereo>,
    lines: Vec<TimedLine>,
    map: TimeMap,
    whole_windows: bool,
    dir: PathBuf,
) -> Result<Aligned> {
    let (generation, sender) = connection().connect(|| {
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("karaoke-model".into())
            .spawn(move || run(rx))
            .map_err(|e| e.to_string())?;
        Ok(tx)
    })?;
    let (reply, result) = mpsc::sync_channel(1);
    if sender
        .send(Request {
            pcm,
            stereo,
            lines,
            map,
            whole_windows,
            dir,
            reply,
        })
        .is_err()
    {
        connection().disconnect(generation);
        return Err("acoustic worker stopped".into());
    }
    match result.recv() {
        Ok(result) => result,
        Err(_) => {
            connection().disconnect(generation);
            Err("acoustic worker did not return a result".into())
        }
    }
}

// Generic idle loop lets tests exercise reuse/expiry without loading an ONNX
// model or sleeping for the production timeout.
fn cached_loop<J, M>(
    rx: mpsc::Receiver<J>,
    idle: Duration,
    mut job: impl FnMut(J, &mut Option<M>),
) {
    let mut model = None;
    loop {
        match rx.recv_timeout(idle) {
            Ok(request) => job(request, &mut model),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if model.take().is_some() {
                    log::info!("karaoke: released idle acoustic model");
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

struct Models {
    dir: PathBuf,
    aligner: AcousticAligner,
    /// The second ear's model failed to load: not retried every song.
    no_second_ear: bool,
}

fn run(rx: mpsc::Receiver<Request>) {
    cached_loop(rx, IDLE, |request, cached: &mut Option<Models>| {
        let started = Instant::now();
        let reused = cached
            .as_ref()
            .is_some_and(|models| models.dir == request.dir);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !reused {
                *cached = None;
                let model = AcousticAligner::load(
                    &request.dir.join("mms-fa-int8.onnx"),
                    &request.dir.join("onnxruntime.dll"),
                )?;
                *cached = Some(Models {
                    dir: request.dir.clone(),
                    aligner: model,
                    no_second_ear: false,
                });
            }
            let models = cached.as_mut().expect("model loaded");
            let preview = crate::vocal_preview::enabled();
            let separator_file = request.dir.join(crate::separation::MODEL_FILE);
            // The aligner initialized the ONNX Runtime the separator uses.
            // Outside the trial the separator is the review's optional
            // second ear: used when its model sits beside the aligner's.
            // It loads per job (about 0.2s) and goes with it, so its ~1.2 GB
            // of working memory is not held between songs.
            let second_ear = !models.no_second_ear && separator_file.is_file();
            let mut separator = None;
            if preview || second_ear {
                match VocalSeparator::load(&separator_file) {
                    Ok(loaded) => separator = Some(loaded),
                    Err(e) if preview => return Err(e),
                    Err(e) => {
                        models.no_second_ear = true;
                        log::warn!("karaoke: no second ear ({e})");
                    }
                }
            }
            let loaded = started.elapsed();
            let vocals = match separator.as_mut().filter(|_| preview) {
                Some(separator) => {
                    let vocals = separator.vocals_16k(
                        request.stereo.as_ref(),
                        &request.pcm,
                        crate::karaoke::TARGET_HZ,
                    )?;
                    log::info!(
                        "karaoke: separated {:.1}s of {} audio in {:.2}s",
                        request.pcm.len() as f64 / crate::karaoke::TARGET_HZ as f64,
                        if request.stereo.is_some() {
                            "stereo"
                        } else {
                            "mono"
                        },
                        started.elapsed().saturating_sub(loaded).as_secs_f64()
                    );
                    Some(vocals)
                }
                None => None,
            };
            let model = &mut models.aligner;
            let aligning = Instant::now();
            let hz = crate::karaoke::TARGET_HZ;
            // One ear for the review and the voice pass, so held notes in
            // stretches the review already separated cost nothing.
            let mut ear = separator.as_mut().filter(|_| !preview).map(|separator| {
                VocalEar::new(
                    separator,
                    request.stereo.as_ref(),
                    &request.pcm,
                    hz,
                    Some(aligning + EAR_DEADLINE),
                )
            });
            model.set_whole_windows(request.whole_windows);
            if vocals.is_none() {
                model.record_take(request.pcm.len(), &request.map);
            }
            let mut result = if let Some(vocals) = vocals.as_deref() {
                model.align_guided_entries(vocals, &request.lines, &request.map, None)
            } else if let Some(ear) = ear.as_mut() {
                model.align_reviewed(
                    &request.pcm,
                    &request.lines,
                    &request.map,
                    Knobs::MIX,
                    ear,
                    None,
                )
            } else {
                model.align(&request.pcm, &request.lines, &request.map)
            };
            let take = model.finish_take();
            let cut = model.take_cut();
            let aligned = aligning.elapsed();
            let (heard, passes) = ear.as_ref().map_or((0, 0), |e| (e.heard, e.passes));
            // The singer's voice on held notes, with the same separator.
            // The vocal trial (whole-song vocals) skips it: dev-only.
            if let (Ok(words), Some(ear)) = (result.as_mut(), ear.as_mut()) {
                let voicing = Instant::now();
                ear.deadline = Some(voicing + VOICE_DEADLINE);
                let voiced = crate::voice::listen(
                    words,
                    &request.map,
                    request.pcm.len(),
                    hz,
                    ear,
                    crate::voice::PASS_BUDGET,
                );
                log::info!(
                    "karaoke: voice on {} of {} held notes in {:.2}s ({} passes)",
                    voiced,
                    words.iter().filter(|w| crate::voice::is_held(w)).count(),
                    voicing.elapsed().as_secs_f64(),
                    ear.passes - passes,
                );
            }
            log::info!(
                "karaoke: model reused={}, load {:.2}s, align {:.2}s, second ear on {:.1}s of {:.1}s ({} passes, {}), take {} KB",
                reused,
                loaded.as_secs_f64(),
                aligned.as_secs_f64(),
                heard as f64 / hz as f64,
                request.pcm.len() as f64 / hz as f64,
                passes,
                if request.stereo.is_some() {
                    "stereo"
                } else {
                    "mono"
                },
                take.as_ref().map_or(0, |t| t.bytes() / 1024),
            );
            result.map(|words| Aligned { words, take, cut })
        }))
        .unwrap_or_else(|_| Err("acoustic worker panicked".into()));
        // Never reuse a failed or possibly poisoned runtime session.
        if result.is_err() {
            *cached = None;
        }
        let _ = request.reply.send(result);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Model(Arc<AtomicUsize>);
    impl Drop for Model {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn reuse_idle_release_and_shutdown_are_bounded() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let tracker = dropped.clone();
        let (tx, rx) = mpsc::sync_channel::<mpsc::SyncSender<usize>>(2);
        let (first_tx, first_rx) = mpsc::sync_channel(1);
        let (second_tx, second_rx) = mpsc::sync_channel(1);
        tx.send(first_tx).unwrap();
        tx.send(second_tx).unwrap();
        let thread = std::thread::spawn(move || {
            let mut loads = 0;
            cached_loop(rx, Duration::from_millis(30), |reply, model| {
                if model.is_none() {
                    loads += 1;
                    *model = Some(Model(tracker.clone()));
                }
                reply.send(loads).unwrap();
            });
        });
        let request = || {
            let (a, b) = mpsc::sync_channel(1);
            tx.send(a).unwrap();
            b.recv_timeout(Duration::from_secs(2)).unwrap()
        };
        assert_eq!(first_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        assert_eq!(second_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        let until = Instant::now() + Duration::from_secs(2);
        while dropped.load(Ordering::SeqCst) == 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(request(), 2);
        drop(tx);
        thread.join().unwrap();
        assert_eq!(dropped.load(Ordering::SeqCst), 2);
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires a local capture, installed model and baseline result"]
fn local_worker_preserves_cold_and_warm_results() {
    let dump = PathBuf::from(std::env::var_os("PALETTE_WORKER_DUMP").expect("capture directory"));
    let dir = PathBuf::from(std::env::var_os("PALETTE_WORKER_MODEL").expect("model directory"));
    let baseline = std::env::var_os("PALETTE_WORKER_BASELINE").expect("baseline JSON");
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dump.join("meta.json")).unwrap()).unwrap();
    let map: TimeMap = serde_json::from_value(meta["map"].clone()).unwrap();
    let raw = std::fs::read(dump.join("pcm.i16")).unwrap();
    let pcm: Vec<i16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    let lines = crate::align::parse_lrc(&std::fs::read_to_string(dump.join("lyrics.lrc")).unwrap());
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(baseline).unwrap()).unwrap();
    let expected: Vec<Word> = serde_json::from_value(expected["words"].clone()).unwrap();
    let start = Instant::now();
    let first = align(
        pcm.clone(),
        None,
        lines.clone(),
        map.clone(),
        false,
        dir.clone(),
    )
    .unwrap()
    .words;
    let cold = start.elapsed();
    let start = Instant::now();
    let second = align(pcm, None, lines, map, false, dir).unwrap().words;
    assert_eq!(first.len(), expected.len());
    for (i, (a, b)) in first.iter().zip(&expected).enumerate() {
        assert_eq!(
            (&a.text, a.t, a.end, a.line_t),
            (&b.text, b.t, b.end, b.line_t),
            "word boundary {i}"
        );
        assert_eq!(a.points.len(), b.points.len(), "point count {i}");
        for (x, y) in a.points.iter().zip(&b.points) {
            assert_eq!(x.t, y.t, "point timestamp {i}");
            assert!(
                (x.fraction - y.fraction).abs() < 1e-12,
                "point fraction {i}: {} != {}",
                x.fraction,
                y.fraction
            );
        }
    }
    assert_eq!(second, first, "warm reuse changed timings");
    println!(
        "worker cold {:.2}s; warm {:.2}s; {} unchanged words",
        cold.as_secs_f64(),
        start.elapsed().as_secs_f64(),
        second.len()
    );
}

#[cfg(test)]
#[test]
fn transient_worker_start_and_disconnect_can_retry() {
    let mut state = Connection::<u8> {
        generation: 0,
        sender: None,
    };
    assert!(state.connect(|| Err("resource pressure".into())).is_err());
    let (old, value) = state.connect(|| Ok(1)).unwrap();
    assert_eq!(value, 1);
    state.disconnect(old);
    let (new, value) = state.connect(|| Ok(2)).unwrap();
    assert_eq!(value, 2);
    state.disconnect(old);
    assert_eq!(
        state.connect(|| panic!("must retain replacement")).unwrap(),
        (new, 2)
    );
}
