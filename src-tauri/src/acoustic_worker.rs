//! One bounded acoustic worker, with an idle-expiring model. Requests already
//! run behind karaoke's ALIGNING guard; no inference touches the media thread.
use crate::{
    acoustic::AcousticAligner,
    align::{TimeMap, TimedLine, Word},
};
use std::{
    path::PathBuf,
    sync::{mpsc, Mutex, OnceLock},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, String>;
const IDLE: Duration = Duration::from_secs(5 * 60);
struct Request {
    pcm: Vec<i16>,
    lines: Vec<TimedLine>,
    map: TimeMap,
    dir: PathBuf,
    reply: mpsc::SyncSender<Result<Vec<Word>>>,
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
    lines: Vec<TimedLine>,
    map: TimeMap,
    dir: PathBuf,
) -> Result<Vec<Word>> {
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
            lines,
            map,
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
fn run(rx: mpsc::Receiver<Request>) {
    cached_loop(
        rx,
        IDLE,
        |request, cached: &mut Option<(PathBuf, AcousticAligner)>| {
            let started = Instant::now();
            let reused = cached
                .as_ref()
                .is_some_and(|(path, _)| path == &request.dir);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if !reused {
                    *cached = None;
                    let model = AcousticAligner::load(
                        &request.dir.join("mms-fa-int8.onnx"),
                        &request.dir.join("onnxruntime.dll"),
                    )?;
                    *cached = Some((request.dir.clone(), model));
                }
                let loaded = started.elapsed();
                let result = cached.as_mut().expect("model loaded").1.align(
                    &request.pcm,
                    &request.lines,
                    &request.map,
                );
                log::info!(
                    "karaoke: model reused={}, load {:.2}s, align {:.2}s",
                    reused,
                    loaded.as_secs_f64(),
                    started.elapsed().saturating_sub(loaded).as_secs_f64()
                );
                result
            }))
            .unwrap_or_else(|_| Err("acoustic worker panicked".into()));
            // Never reuse a failed or possibly poisoned runtime session.
            if result.is_err() {
                *cached = None;
            }
            let _ = request.reply.send(result);
        },
    );
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
    let first = align(pcm.clone(), lines.clone(), map.clone(), dir.clone()).unwrap();
    let cold = start.elapsed();
    let start = Instant::now();
    let second = align(pcm, lines, map, dir).unwrap();
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
