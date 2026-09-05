//! Offline scorer for the word-karaoke aligner (docs/specs/2026-09-04).
//!
//! Input is one evidence dump the app writes when settings.json has
//! `"karaokeDump": true` (app-data/karaoke-dumps/<key>/): pcm.i16,
//! lyrics.lrc, words.json, meta.json.
//!
//!   cargo run --example karaoke_score -- template <dump-dir>
//!       re-runs the CURRENT aligner on the dump and writes
//!       <dump-dir>/labels.template.txt — an Audacity label track with one
//!       row per aligner token at the aligner's guess. Import pcm.i16 into
//!       Audacity (File → Import → Raw Data: 16-bit signed, mono, 16000Hz),
//!       import the labels, drag each marker onto the sung onset, export.
//!
//!   cargo run --example karaoke_score -- score <dump-dir> <labels.txt>
//!       re-runs the aligner, pairs token i with label row i (order, never
//!       text), and prints median/p90 |Δ|, share within 100ms, signed bias,
//!       the five worst lines, the map's fit residual (clock error, kept
//!       separate from aligner error), and whether the replay reproduces
//!       the dump's live words.json exactly.

use pulse_lib::align::{self, TimeMap, TimedLine, Word};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::exit;

const RATE: u32 = 16_000;

#[derive(Deserialize)]
struct Meta {
    #[serde(default)]
    artist: String,
    #[serde(default)]
    title: String,
    map: TimeMap,
}

#[derive(Deserialize)]
struct StoreFile {
    words: Vec<Word>,
}

struct Dump {
    pcm: Vec<f32>,
    lines: Vec<TimedLine>,
    meta: Meta,
    live: Vec<Word>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, dir] if cmd == "template" => template(Path::new(dir)),
        [cmd, dir, labels] if cmd == "score" => score(Path::new(dir), Path::new(labels)),
        _ => {
            eprintln!("usage: karaoke_score template <dump-dir>\n       karaoke_score score <dump-dir> <labels.txt>");
            exit(2);
        }
    }
}

fn load(dir: &Path) -> Dump {
    let bytes = read(dir.join("pcm.i16"));
    let pcm: Vec<f32> = bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
        .collect();
    let lrc = String::from_utf8_lossy(&read(dir.join("lyrics.lrc"))).into_owned();
    let lines = align::parse_lrc(&lrc);
    let meta: Meta = serde_json::from_slice(&read(dir.join("meta.json")))
        .unwrap_or_else(|e| die(&format!("meta.json: {e}")));
    let live: Vec<Word> = serde_json::from_slice::<StoreFile>(&read(dir.join("words.json")))
        .map(|f| f.words)
        .unwrap_or_default();
    if pcm.is_empty() || lines.is_empty() {
        die("dump has no pcm or no lyric lines");
    }
    Dump {
        pcm,
        lines,
        meta,
        live,
    }
}

fn read(path: PathBuf) -> Vec<u8> {
    std::fs::read(&path).unwrap_or_else(|e| die(&format!("{}: {e}", path.display())))
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(1)
}

fn realign(d: &Dump) -> Vec<Word> {
    align::align(&d.pcm, RATE, &d.lines, &d.meta.map)
}

fn template(dir: &Path) {
    let d = load(dir);
    let words = realign(&d);
    let mut out = String::new();
    for w in &words {
        let s = w.t as f64 / 1000.0;
        out.push_str(&format!("{s:.3}\t{s:.3}\t{}\n", w.text.trim()));
    }
    let path = dir.join("labels.template.txt");
    std::fs::write(&path, out).unwrap_or_else(|e| die(&format!("{}: {e}", path.display())));
    println!(
        "{} — {} tokens over {} lines → {}",
        d.meta.title,
        words.len(),
        d.lines.len(),
        path.display()
    );
}

/// Audacity label rows: `start\tend\ttext`, seconds. Blank lines skipped.
fn read_labels(path: &Path) -> Vec<i64> {
    let text = String::from_utf8_lossy(&read(path.to_path_buf())).into_owned();
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            let start = l.split('\t').next().unwrap_or("").trim();
            start
                .parse::<f64>()
                .map(|s| (s * 1000.0).round() as i64)
                .unwrap_or_else(|_| die(&format!("labels row {}: bad start {start:?}", i + 1)))
        })
        .collect()
}

/// Index of the lyric line whose window holds `t`.
fn line_of(lines: &[TimedLine], t: i64) -> usize {
    let mut idx = 0;
    for (i, l) in lines.iter().enumerate() {
        if l.t <= t {
            idx = i;
        } else {
            break;
        }
    }
    idx
}

fn score(dir: &Path, labels_path: &Path) {
    let d = load(dir);
    let words = realign(&d);
    let labels = read_labels(labels_path);
    if labels.is_empty() {
        die("label file has no rows");
    }
    if labels.len() > words.len() {
        die(&format!(
            "label rows ({}) > aligner tokens ({}): a marker was added, or the dump is a different song",
            labels.len(),
            words.len(),
        ));
    }
    // A partial label file scores the prefix it covers — marking stops
    // wherever the person marking ran out of patience.
    if labels.len() < words.len() {
        let last_line = line_of(&d.lines, words[labels.len() - 1].t);
        println!(
            "partial       {} of {} tokens marked (through line {})",
            labels.len(),
            words.len(),
            last_line + 1
        );
    }
    if let Some(i) = words
        .iter()
        .zip(labels.iter())
        .position(|(w, &l)| (w.t - l).abs() > 5_000)
    {
        die(&format!(
            "label row {} is {}s from the aligner's token — rows drifted out of order around line {}: {:?}",
            i + 1,
            (words[i].t - labels[i]).abs() / 1000,
            line_of(&d.lines, words[i].t) + 1,
            d.lines[line_of(&d.lines, words[i].t)].text
        ));
    }
    let deltas: Vec<i64> = words.iter().zip(&labels).map(|(w, &l)| w.t - l).collect();
    let mut abs: Vec<i64> = deltas.iter().map(|d| d.abs()).collect();
    abs.sort_unstable();
    let pct = |p: f64| abs[((abs.len() - 1) as f64 * p).round() as usize];
    let within = abs.iter().filter(|&&a| a <= 100).count();
    let bias = deltas.iter().sum::<i64>() as f64 / deltas.len() as f64;

    println!("{} — {}", d.meta.artist, d.meta.title);
    println!("tokens        {}", words.len());
    println!("median |Δ|    {} ms", pct(0.5));
    println!("p90 |Δ|       {} ms", pct(0.9));
    println!(
        "within 100ms  {:.0}%",
        within as f64 * 100.0 / abs.len() as f64
    );
    println!(
        "bias          {bias:+.0} ms ({})",
        if bias < 0.0 { "early" } else { "late" }
    );
    let m = &d.meta.map;
    println!(
        "time map      {} anchors, rms {:.0} ms{}{}",
        m.n_anchors,
        m.residual_rms_ms,
        if m.clamped { ", slope clamped" } else { "" },
        if m.from_origin {
            ", ORIGIN FALLBACK"
        } else {
            ""
        },
    );
    println!(
        "replay        {}",
        if words == d.live {
            "reproduces live words.json"
        } else {
            "DIFFERS from live words.json (aligner changed since the dump, or dump infidelity)"
        }
    );

    // Worst lines by mean |Δ|.
    let mut per_line: Vec<(usize, Vec<i64>)> = Vec::new();
    for (w, &delta) in words.iter().zip(&deltas) {
        let li = line_of(&d.lines, w.t);
        match per_line.last_mut() {
            Some((i, v)) if *i == li => v.push(delta),
            _ => per_line.push((li, vec![delta])),
        }
    }
    per_line.sort_by_key(|(_, v)| {
        std::cmp::Reverse(v.iter().map(|d| d.abs()).sum::<i64>() / v.len() as i64)
    });
    println!("worst lines");
    for (li, v) in per_line.iter().take(5) {
        let mean = v.iter().map(|d| d.abs()).sum::<i64>() / v.len() as i64;
        println!(
            "  {:>5} ms  [{}] {}",
            mean,
            fmt_ms(d.lines[*li].t),
            d.lines[*li].text
        );
    }
}

fn fmt_ms(ms: i64) -> String {
    format!(
        "{:02}:{:02}.{:02}",
        ms / 60_000,
        (ms / 1000) % 60,
        (ms % 1000) / 10
    )
}
