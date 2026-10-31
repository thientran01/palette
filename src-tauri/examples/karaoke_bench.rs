//! Automatic scoreboard for the word-karaoke aligner on a public sung-lyrics
//! set with human word onsets (JamendoLyrics MultiLang, CC audio). No app
//! behavior changes; this only replays both shipped pipelines offline.
//!
//!   cargo run --release --example karaoke_bench -- <jamendolyrics-dir> <model-dir> <out-dir>
//!       [--only <substring>] [--mix-only] [--lead-ms <n>] [--shard k/n] [--report]
//!
//! Per song: ffmpeg decodes the MP3 to 48kHz float stereo (the usual
//! loopback device rate); mono = channel mean box-decimated to 16kHz and
//! quantized exactly as the recorder does (Rec::push, then ×32767). The
//! TimeMap is identity. LRC lines come from the annotated word rows (line
//! breaks at `line_end`), stamped at the first word's onset rounded to the
//! centisecond, as LRCLIB stores them; `--lead-ms` moves every stamp
//! earlier to mimic an LRC that leads the vocal. Then:
//!   mix    = AcousticAligner::align on the mix (what ships)
//!   vocals = VocalSeparator::vocals_16k + align_guided_entries (the trial)
//! Each predicted word onset is paired by line and position with its
//! annotated onset. Separated vocals are cached in <out-dir>/vocals and each
//! song's pairing lands in <out-dir>/songs-lead<n>/<song>.json; the summary
//! is always aggregated from those files, so `--shard k/n` processes can run
//! in parallel and a final `--report` prints the whole set. Never point
//! <out-dir> inside the repo.
//!
//! Line stamps are human line onsets, so a line's first word starts from a
//! near-perfect stamp; "rest" scores only the words after it.

use pulse_lib::{
    acoustic::AcousticAligner,
    align::{tokenize, TimeMap, TimedLine, Word},
    separation::{to_target_grid, Stereo, VocalSeparator, MODEL_FILE},
};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{exit, Command, Stdio};
use std::time::Instant;

const CAPTURE_HZ: u32 = 48_000;
const TARGET_HZ: u32 = 16_000;
const MAX_SECONDS: usize = 8 * 60;
const PIPELINES: [&str; 2] = ["mix", "vocals"];

struct Song {
    stem: String,
    language: String,
    meta: String,
}

/// One LRC line and the annotated onsets of its words, in token order.
struct Line {
    stamp: i64,
    text: String,
    truth: Vec<i64>,
}

#[derive(Default, Clone)]
struct Errors {
    /// Signed predicted − annotated onset, ms.
    all: Vec<i64>,
    first: Vec<i64>,
    rest: Vec<i64>,
    truth_words: usize,
}

impl Errors {
    fn extend(&mut self, o: &Errors) {
        self.all.extend(&o.all);
        self.first.extend(&o.first);
        self.rest.extend(&o.rest);
        self.truth_words += o.truth_words;
    }

    fn coverage(&self) -> f64 {
        self.all.len() as f64 * 100.0 / self.truth_words.max(1) as f64
    }
}

struct Summary {
    n: usize,
    median: i64,
    p90: i64,
    within_100: f64,
    within_300: f64,
    bias_mean: f64,
    bias_median: i64,
}

fn summarize(d: &[i64]) -> Option<Summary> {
    if d.is_empty() {
        return None;
    }
    let mut abs: Vec<i64> = d.iter().map(|v| v.abs()).collect();
    abs.sort_unstable();
    let mut signed = d.to_vec();
    signed.sort_unstable();
    let rank = |v: &[i64], q: f64| v[((q * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1];
    let share =
        |ms: i64| abs.iter().filter(|&&a| a <= ms).count() as f64 * 100.0 / abs.len() as f64;
    Some(Summary {
        n: d.len(),
        median: rank(&abs, 0.5),
        p90: rank(&abs, 0.9),
        within_100: share(100),
        within_300: share(300),
        bias_mean: d.iter().sum::<i64>() as f64 / d.len() as f64,
        bias_median: rank(&signed, 0.5),
    })
}

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("karaoke_bench: {msg}");
    exit(1)
}

/// RFC-4180-ish: quoted fields may hold commas and doubled quotes.
fn csv_rows(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let (mut fields, mut cur, mut quoted) = (Vec::new(), String::new(), false);
            let mut chars = line.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '"' if quoted && chars.peek() == Some(&'"') => {
                        cur.push('"');
                        chars.next();
                    }
                    '"' => quoted = !quoted,
                    ',' if !quoted => fields.push(std::mem::take(&mut cur)),
                    _ => cur.push(c),
                }
            }
            fields.push(cur);
            fields
        })
        .collect()
}

fn songs(dataset: &Path) -> Vec<Song> {
    let text = std::fs::read_to_string(dataset.join("JamendoLyrics.csv"))
        .unwrap_or_else(|e| die(format!("JamendoLyrics.csv: {e}")));
    let rows = csv_rows(&text);
    let head = &rows[0];
    let col = |name: &str| {
        head.iter()
            .position(|h| h == name)
            .unwrap_or_else(|| die(format!("metadata lacks {name}")))
    };
    let (file, lang, genre) = (col("Filepath"), col("Language"), col("Genre"));
    let flags = ["LyricOverlap", "Polyphonic", "NonLexical"].map(|f| (f, col(f)));
    rows[1..]
        .iter()
        .map(|r| {
            let flags: Vec<&str> = flags
                .iter()
                .filter(|(_, i)| r[*i] == "true")
                .map(|(f, _)| *f)
                .collect();
            Song {
                stem: r[file].trim_end_matches(".mp3").to_string(),
                language: r[lang].clone(),
                meta: [r[genre].as_str()]
                    .into_iter()
                    .chain(flags)
                    .collect::<Vec<_>>()
                    .join(" · "),
            }
        })
        .collect()
}

/// Lines from the word annotations themselves, so tokens pair 1:1 with rows.
fn lines(dataset: &Path, stem: &str, lead_ms: i64) -> Result<Vec<Line>, String> {
    let words = std::fs::read_to_string(dataset.join("lyrics").join(format!("{stem}.words.txt")))
        .map_err(|e| format!("words.txt: {e}"))?;
    let words: Vec<&str> = words
        .lines()
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .collect();
    let csv = std::fs::read_to_string(
        dataset
            .join("annotations/words")
            .join(format!("{stem}.csv")),
    )
    .map_err(|e| format!("word annotations: {e}"))?;
    let rows = csv_rows(&csv);
    let rows = &rows[1..];
    if rows.len() != words.len() {
        return Err(format!("{} word rows vs {} words", rows.len(), words.len()));
    }
    let mut out: Vec<Line> = Vec::new();
    let mut cur: Option<Line> = None;
    for (row, word) in rows.iter().zip(&words) {
        let start: f64 = row[0]
            .parse()
            .map_err(|_| format!("bad onset {:?}", row[0]))?;
        let line = cur.get_or_insert_with(|| Line {
            // LRCLIB stamps are centisecond precision.
            stamp: ((start * 100.0).round() as i64 * 10 - lead_ms).max(0),
            text: String::new(),
            truth: Vec::new(),
        });
        if !line.text.is_empty() {
            line.text.push(' ');
        }
        line.text.push_str(word);
        line.truth.push((start * 1000.0).round() as i64);
        if row
            .get(2)
            .is_some_and(|v| v.parse::<f64>().is_ok_and(f64::is_finite))
        {
            out.extend(cur.take());
        }
    }
    out.extend(cur.take());
    for (i, l) in out.iter().enumerate() {
        let n = tokenize(&l.text).len();
        if n != l.truth.len() {
            return Err(format!("line {i}: {n} tokens vs {} words", l.truth.len()));
        }
    }
    // Equal stamps would make attachment by line_t ambiguous.
    for i in 1..out.len() {
        if out[i].stamp <= out[i - 1].stamp {
            out[i].stamp = out[i - 1].stamp + 10;
        }
    }
    Ok(out)
}

/// ffmpeg → interleaved f32 stereo at the capture rate.
fn decode(mp3: &Path) -> Result<Vec<f32>, String> {
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(mp3)
        .args([
            "-f",
            "f32le",
            "-ac",
            "2",
            "-ar",
            &CAPTURE_HZ.to_string(),
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let mut raw = Vec::new();
    child
        .stdout
        .take()
        .expect("piped")
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "ffmpeg failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

fn quantize(x: &[f32]) -> Vec<i16> {
    x.iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

fn read_i16(path: &Path) -> Option<Vec<i16>> {
    let raw = std::fs::read(path).ok()?;
    Some(
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect(),
    )
}

fn write_i16(path: &Path, x: &[i16]) {
    let bytes: Vec<u8> = x.iter().flat_map(|v| v.to_le_bytes()).collect();
    if let Err(e) = std::fs::write(path, bytes) {
        eprintln!("cache write {}: {e}", path.display());
    }
}

/// Pair predictions with truth by line stamp and token position.
fn score(lines: &[Line], words: &[Word]) -> serde_json::Value {
    let mut by_line: BTreeMap<i64, Vec<&Word>> = BTreeMap::new();
    for w in words {
        if let Some(t) = w.line_t {
            by_line.entry(t).or_default().push(w);
        }
    }
    let mut rows = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let predicted = by_line.get(&line.stamp).map_or(&[][..], Vec::as_slice);
        if predicted.len() != line.truth.len() {
            rows.push(serde_json::json!({
                "line": li, "stamp": line.stamp, "text": line.text,
                "missing": line.truth.len(), "predicted": predicted.len(),
            }));
            continue;
        }
        let deltas: Vec<i64> = predicted
            .iter()
            .zip(&line.truth)
            .map(|(w, &t)| w.t - t)
            .collect();
        rows.push(serde_json::json!({
            "line": li, "stamp": line.stamp, "text": line.text,
            "truth": line.truth,
            "predicted": predicted.iter().map(|w| w.t).collect::<Vec<_>>(),
            "delta": deltas,
        }));
    }
    rows.into()
}

/// Rebuild one pipeline's errors from the rows `score` wrote.
fn from_rows(rows: &serde_json::Value) -> Errors {
    let mut e = Errors::default();
    for row in rows.as_array().into_iter().flatten() {
        if let Some(n) = row["missing"].as_u64() {
            e.truth_words += n as usize;
            continue;
        }
        let d: Vec<i64> = row["delta"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_i64)
            .collect();
        e.truth_words += d.len();
        if let Some((first, rest)) = d.split_first() {
            e.first.push(*first);
            e.rest.extend(rest);
        }
        e.all.extend(d);
    }
    e
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut flag = |name: &str, value: bool| -> Option<String> {
        let i = args.iter().position(|a| a == name)?;
        args.remove(i);
        if !value {
            return Some(String::new());
        }
        Some(if i < args.len() {
            args.remove(i)
        } else {
            die(format!("{name} needs a value"))
        })
    };
    let only = flag("--only", true);
    let mix_only = flag("--mix-only", false).is_some();
    let report_only = flag("--report", false).is_some();
    let shard: Option<(usize, usize)> = flag("--shard", true).map(|v| {
        v.split_once('/')
            .and_then(|(k, n)| Some((k.parse().ok()?, n.parse().ok()?)))
            .filter(|&(k, n): &(usize, usize)| k < n)
            .unwrap_or_else(|| die("--shard k/n with 0 <= k < n"))
    });
    let lead_ms: i64 = flag("--lead-ms", true).map_or(0, |v| {
        v.parse()
            .unwrap_or_else(|_| die("--lead-ms needs an integer"))
    });
    let [dataset, models, out] = args.as_slice() else {
        die("usage: karaoke_bench <jamendolyrics-dir> <model-dir> <out-dir> [--only s] [--mix-only] [--lead-ms n] [--shard k/n] [--report]");
    };
    let (dataset, models, out) = (
        PathBuf::from(dataset),
        PathBuf::from(models),
        PathBuf::from(out),
    );
    let started = Instant::now();
    let list: Vec<Song> = songs(&dataset)
        .into_iter()
        .filter(|s| only.as_ref().is_none_or(|o| s.stem.contains(o.as_str())))
        .collect();
    let songs_dir = out.join(format!("songs-lead{lead_ms}"));
    if !report_only {
        let run = Run {
            dataset: &dataset,
            models: &models,
            vocals_dir: out.join("vocals"),
            songs_dir: &songs_dir,
            lead_ms,
        };
        for d in [&run.vocals_dir, &songs_dir] {
            std::fs::create_dir_all(d).unwrap_or_else(|e| die(e));
        }
        run.process(&list, shard, mix_only);
    }
    // A shard only writes its songs; `--report` prints the whole set.
    if shard.is_none() {
        report(&songs_dir, &list, lead_ms, started.elapsed().as_secs_f64());
    }
}

struct Run<'a> {
    dataset: &'a Path,
    models: &'a Path,
    vocals_dir: PathBuf,
    songs_dir: &'a Path,
    lead_ms: i64,
}

impl Run<'_> {
    fn process(&self, list: &[Song], shard: Option<(usize, usize)>, mix_only: bool) {
        // Loading the aligner first initializes the ONNX Runtime.
        let mut aligner = AcousticAligner::load(
            &self.models.join("mms-fa-int8.onnx"),
            &self.models.join("onnxruntime.dll"),
        )
        .unwrap_or_else(|e| die(e));
        let mut separator = (!mix_only).then(|| {
            VocalSeparator::load(&self.models.join(MODEL_FILE)).unwrap_or_else(|e| die(e))
        });
        for (k, song) in list.iter().enumerate() {
            if shard.is_some_and(|(i, n)| k % n != i) {
                continue;
            }
            let progress = format!("[{}/{}] {}", k + 1, list.len(), song.stem);
            let lines = match lines(self.dataset, &song.stem, self.lead_ms) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("{progress} skipped: {e}");
                    continue;
                }
            };
            let t = Instant::now();
            let mp3 = self.dataset.join("mp3").join(format!("{}.mp3", song.stem));
            let stereo = match decode(&mp3) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{progress} skipped: {e}");
                    continue;
                }
            };
            let mono: Vec<f32> = stereo
                .as_chunks::<2>()
                .0
                .iter()
                .map(|f| (f[0] + f[1]) / 2.0)
                .collect();
            let pcm = quantize(&to_target_grid(&mono, CAPTURE_HZ, TARGET_HZ));
            let decode_secs = t.elapsed().as_secs_f64();
            let timed: Vec<TimedLine> = lines
                .iter()
                .map(|l| TimedLine {
                    t: l.stamp,
                    text: l.text.clone(),
                })
                .collect();
            let map = TimeMap::from_origin(0, TARGET_HZ);
            let mut detail = serde_json::Map::new();

            let t = Instant::now();
            match aligner.align(&pcm, &timed, &map) {
                Ok(words) => {
                    detail.insert("mix".into(), score(&lines, &words));
                }
                Err(e) => eprintln!("{progress} mix align failed: {e}"),
            }
            let mix_secs = t.elapsed().as_secs_f64();

            let (mut sep_secs, mut voc_secs) = (0.0, 0.0);
            if let Some(sep) = separator.as_mut() {
                let cache = self.vocals_dir.join(format!("{}.i16", song.stem));
                let vocals = match read_i16(&cache).filter(|v| v.len() == pcm.len()) {
                    Some(v) => Some(v),
                    None if pcm.len() > TARGET_HZ as usize * MAX_SECONDS => {
                        eprintln!("{progress}: longer than the separator's 8-minute bound");
                        None
                    }
                    None => {
                        let t = Instant::now();
                        let input = Stereo {
                            interleaved: quantize(&stereo),
                            rate: CAPTURE_HZ,
                        };
                        let v = sep.vocals_16k(Some(&input), &pcm, TARGET_HZ);
                        sep_secs = t.elapsed().as_secs_f64();
                        match v {
                            Ok(v) => {
                                write_i16(&cache, &v);
                                Some(v)
                            }
                            Err(e) => {
                                eprintln!("{progress} separation failed: {e}");
                                None
                            }
                        }
                    }
                };
                if let Some(vocals) = vocals {
                    let t = Instant::now();
                    match aligner.align_guided_entries(&vocals, &timed, &map, None) {
                        Ok(words) => {
                            detail.insert("vocals".into(), score(&lines, &words));
                        }
                        Err(e) => eprintln!("{progress} vocal align failed: {e}"),
                    }
                    voc_secs = t.elapsed().as_secs_f64();
                }
            }
            let audio = pcm.len() as f64 / TARGET_HZ as f64;
            detail.insert(
                "seconds".into(),
                serde_json::json!({
                    "audio": audio, "decode": decode_secs, "separate": sep_secs,
                    "align_mix": mix_secs, "align_vocals": voc_secs,
                }),
            );
            let brief = |p: &str| {
                detail
                    .get(p)
                    .and_then(|rows| summarize(&from_rows(rows).all))
                    .map_or("–".to_string(), |s| format!("{}ms", s.median))
            };
            eprintln!(
                "{progress} ({}) mix {} vocals {} · {audio:.0}s audio, sep {sep_secs:.1}s, align {mix_secs:.1}+{voc_secs:.1}s",
                song.language,
                brief("mix"),
                brief("vocals"),
            );
            let json = serde_json::to_vec_pretty(&serde_json::Value::Object(detail))
                .expect("serializable");
            let path = self.songs_dir.join(format!("{}.json", song.stem));
            if let Err(e) = std::fs::write(&path, json) {
                eprintln!("{}: {e}", path.display());
            }
        }
    }
}

fn fmt_row(label: &str, s: &Option<Summary>, coverage: Option<f64>) -> String {
    match s {
        None => format!("| {label} | – | – | – | – | – | – | – |"),
        Some(s) => format!(
            "| {label} | {} | {} | {} | {:.0}% | {:.0}% | {:+.0} / {:+} | {} |",
            s.n,
            s.median,
            s.p90,
            s.within_100,
            s.within_300,
            s.bias_mean,
            s.bias_median,
            coverage.map_or(String::from("–"), |c| format!("{c:.0}%")),
        ),
    }
}

const HEADER: &str = "| | words | median ms | p90 ms | ≤100ms | ≤300ms | bias mean / median | coverage |\n|---|---|---|---|---|---|---|---|";

fn report(songs_dir: &Path, list: &[Song], lead_ms: i64, wall: f64) {
    let mut overall: BTreeMap<&str, Errors> = BTreeMap::new();
    let mut by_lang: BTreeMap<(&str, &str), Errors> = BTreeMap::new();
    let mut per_song: Vec<(&Song, BTreeMap<&str, Errors>)> = Vec::new();
    // audio, decode, separate, align mix, align vocals
    let mut times = [0.0f64; 5];
    for song in list {
        let Ok(raw) = std::fs::read(songs_dir.join(format!("{}.json", song.stem))) else {
            continue;
        };
        let detail: serde_json::Value = serde_json::from_slice(&raw).unwrap_or_else(|e| die(e));
        for (slot, key) in
            times
                .iter_mut()
                .zip(["audio", "decode", "separate", "align_mix", "align_vocals"])
        {
            *slot += detail["seconds"][key].as_f64().unwrap_or(0.0);
        }
        let mut results = BTreeMap::new();
        for p in PIPELINES {
            if let Some(rows) = detail.get(p) {
                let e = from_rows(rows);
                overall.entry(p).or_default().extend(&e);
                by_lang
                    .entry((song.language.as_str(), p))
                    .or_default()
                    .extend(&e);
                results.insert(p, e);
            }
        }
        per_song.push((song, results));
    }

    let mut md = String::new();
    let mut w = |s: String| {
        md.push_str(&s);
        md.push('\n');
    };
    w(format!(
        "# Karaoke bench — JamendoLyrics, stamps lead {lead_ms} ms, {} songs\n",
        per_song.len()
    ));
    w("## Overall\n".into());
    w(HEADER.into());
    for p in PIPELINES {
        if let Some(e) = overall.get(p) {
            w(fmt_row(
                &format!("{p} · all"),
                &summarize(&e.all),
                Some(e.coverage()),
            ));
            w(fmt_row(
                &format!("{p} · first word"),
                &summarize(&e.first),
                None,
            ));
            w(fmt_row(&format!("{p} · rest"), &summarize(&e.rest), None));
        }
    }
    w("\n## By language\n".into());
    w(HEADER.into());
    for ((lang, p), e) in &by_lang {
        w(fmt_row(
            &format!("{lang} · {p}"),
            &summarize(&e.all),
            Some(e.coverage()),
        ));
    }
    for ((lang, p), e) in &by_lang {
        w(fmt_row(
            &format!("{lang} · {p} · rest"),
            &summarize(&e.rest),
            None,
        ));
    }
    w("\n## Per song (worst vocals median first)\n".into());
    w("| song | lang | genre · flags | mix median | mix p90 | mix ≤300 | mix bias | vocals median | vocals p90 | vocals ≤300 | vocals bias |\n|---|---|---|---|---|---|---|---|---|---|---|".into());
    let rank_key = |r: &BTreeMap<&str, Errors>| {
        r.get("vocals")
            .or(r.get("mix"))
            .and_then(|e| summarize(&e.all))
            .map_or(i64::MAX, |s| s.median)
    };
    per_song.sort_by_key(|(_, r)| std::cmp::Reverse(rank_key(r)));
    for (song, r) in &per_song {
        let cell = |p: &str| match r.get(p).and_then(|e| summarize(&e.all)) {
            Some(s) => format!(
                "{} | {} | {:.0}% | {:+.0}",
                s.median, s.p90, s.within_300, s.bias_mean
            ),
            None => "– | – | – | –".into(),
        };
        w(format!(
            "| {} | {} | {} | {} | {} |",
            song.stem,
            song.language,
            song.meta,
            cell("mix"),
            cell("vocals")
        ));
    }
    w(format!(
        "\n{:.0} min of audio. Summed per-song time {:.0}s: decode {:.0}s, separation {:.0}s (uncached songs only), align mix {:.0}s, align vocals {:.0}s. This process: {:.0}s wall.",
        times[0] / 60.0,
        times[1..].iter().sum::<f64>(),
        times[1],
        times[2],
        times[3],
        times[4],
        wall
    ));
    print!("{md}");
    let _ = std::fs::write(songs_dir.with_extension("md"), md);
}
