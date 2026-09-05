//! Offline scorer for the word-karaoke aligner (docs/specs/2026-09-04).
//!
//! Input is one evidence dump the app writes when settings.json has
//! `"karaokeDump": true` (app-data/karaoke-dumps/<key>/): pcm.i16,
//! lyrics.lrc, words.json, meta.json.
//!
//!   cargo run --example karaoke_score -- template <dump-dir>
//!       re-runs the CURRENT aligner on the dump and writes
//!       <dump-dir>/labels.template.txt — an Audacity label track with one
//!       row per aligner token at the aligner's guess (also the token list
//!       tap.html taps through). Matching is by row order, never text.
//!
//!   cargo run --example karaoke_score -- score <dump-dir> <labels.txt>
//!       re-runs the shipped aligner, pairs token i with label row i, and
//!       prints median/p90 |Δ|, share within 100ms, signed bias, the five
//!       worst lines, the map's fit residual (clock error, kept separate
//!       from aligner error), and how far the replay sits from the dump's
//!       live words.json. A partial label file scores its prefix.
//!
//!   cargo run --example karaoke_score -- matrix <dump-dir> <labels.txt>
//!       one row per rung of the ladder (Stages::ladder()), the shipped
//!       set marked — the table that decides what ships.

use pulse_lib::align::{self, Stages, TimeMap, TimedLine, Word};
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
        [cmd, dir, labels] if cmd == "matrix" => matrix(Path::new(dir), Path::new(labels)),
        _ => {
            eprintln!(
                "usage: karaoke_score template <dump-dir>\n       karaoke_score score  <dump-dir> <labels.txt>\n       karaoke_score matrix <dump-dir> <labels.txt>"
            );
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

fn realign(d: &Dump, stages: &Stages) -> Vec<Word> {
    align::align_with(&d.pcm, RATE, &d.lines, &d.meta.map, stages)
}

fn template(dir: &Path) {
    let d = load(dir);
    let words = realign(&d, &Stages::shipped());
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
    let labels: Vec<i64> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            let start = l.split('\t').next().unwrap_or("").trim();
            start
                .parse::<f64>()
                .map(|s| (s * 1000.0).round() as i64)
                .unwrap_or_else(|_| die(&format!("labels row {}: bad start {start:?}", i + 1)))
        })
        .collect();
    if labels.is_empty() {
        die("label file has no rows");
    }
    labels
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

struct Stats {
    median: i64,
    p90: i64,
    within_100: f64,
    bias: f64,
    /// (mean |Δ| ms, line index), worst first.
    worst: Vec<(i64, usize)>,
}

/// Pairs token i with label i (a partial file scores its prefix); dies if a
/// label sits outside its token's LRC line window (rows drifted).
fn stats(d: &Dump, words: &[Word], labels: &[i64]) -> Stats {
    if labels.len() > words.len() {
        die(&format!(
            "label rows ({}) > aligner tokens ({}): a marker was added, or the dump is a different song",
            labels.len(),
            words.len(),
        ));
    }
    for (i, (w, &l)) in words.iter().zip(labels.iter()).enumerate() {
        let li = line_of(&d.lines, w.t);
        let lo = d.lines[li].t - 2_000;
        let hi = d.lines.get(li + 1).map(|n| n.t + 2_000).unwrap_or(i64::MAX);
        if l < lo || l > hi {
            die(&format!(
                "label row {} ({:.3}s, {:?}) lies outside line {} [{}]: {:?} — rows drifted out of order",
                i + 1,
                l as f64 / 1000.0,
                w.text.trim(),
                li + 1,
                fmt_ms(d.lines[li].t),
                d.lines[li].text
            ));
        }
    }
    let deltas: Vec<i64> = words.iter().zip(labels).map(|(w, &l)| w.t - l).collect();
    let mut abs: Vec<i64> = deltas.iter().map(|d| d.abs()).collect();
    abs.sort_unstable();
    let pct = |p: f64| abs[((abs.len() - 1) as f64 * p).round() as usize];
    let within = abs.iter().filter(|&&a| a <= 100).count();
    let mut per_line: Vec<(usize, Vec<i64>)> = Vec::new();
    for (w, &delta) in words.iter().zip(&deltas) {
        let li = line_of(&d.lines, w.t);
        match per_line.last_mut() {
            Some((i, v)) if *i == li => v.push(delta),
            _ => per_line.push((li, vec![delta])),
        }
    }
    let mut worst: Vec<(i64, usize)> = per_line
        .iter()
        .map(|(li, v)| (v.iter().map(|d| d.abs()).sum::<i64>() / v.len() as i64, *li))
        .collect();
    worst.sort_by_key(|(m, _)| std::cmp::Reverse(*m));
    Stats {
        median: pct(0.5),
        p90: pct(0.9),
        within_100: within as f64 * 100.0 / abs.len() as f64,
        bias: deltas.iter().sum::<i64>() as f64 / deltas.len() as f64,
        worst,
    }
}

fn score(dir: &Path, labels_path: &Path) {
    let d = load(dir);
    let words = realign(&d, &Stages::shipped());
    let labels = read_labels(labels_path);
    let s = stats(&d, &words, &labels);

    println!("{} — {}", d.meta.artist, d.meta.title);
    println!("tokens        {}", words.len());
    if labels.len() < words.len() {
        println!(
            "partial       {} of {} tokens marked (through line {})",
            labels.len(),
            words.len(),
            line_of(&d.lines, words[labels.len() - 1].t) + 1
        );
    }
    println!("median |Δ|    {} ms", s.median);
    println!("p90 |Δ|       {} ms", s.p90);
    println!("within 100ms  {:.0}%", s.within_100);
    println!(
        "bias          {:+.0} ms ({})",
        s.bias,
        if s.bias < 0.0 { "early" } else { "late" }
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
    // The dump is i16 (the app aligned f32): a few tokens moving by a
    // frame is quantization, not infidelity — report the size, not a bool.
    let moved: Vec<i64> = words
        .iter()
        .zip(&d.live)
        .filter(|(a, b)| a.t != b.t)
        .map(|(a, b)| (a.t - b.t).abs())
        .collect();
    if words.len() != d.live.len() {
        println!(
            "replay        {} tokens vs {} live — aligner changed since the dump",
            words.len(),
            d.live.len()
        );
    } else if moved.is_empty() {
        println!("replay        reproduces live words.json exactly");
    } else {
        println!(
            "replay        {} of {} tokens differ from live, max {} ms (i16 quantization if small; an aligner change if large)",
            moved.len(),
            words.len(),
            moved.iter().max().unwrap_or(&0)
        );
    }
    println!("worst lines");
    for (mean, li) in s.worst.iter().take(5) {
        println!(
            "  {:>5} ms  [{}] {}",
            mean,
            fmt_ms(d.lines[*li].t),
            d.lines[*li].text
        );
    }
}

fn matrix(dir: &Path, labels_path: &Path) {
    let d = load(dir);
    let labels = read_labels(labels_path);
    let shipped = Stages::shipped();
    println!(
        "{} — {} ({} labels)",
        d.meta.artist,
        d.meta.title,
        labels.len()
    );
    println!(
        "{:<20} {:>7} {:>7} {:>8} {:>7}  worst line",
        "stages", "median", "p90", "<=100ms", "bias"
    );
    for (name, stages) in Stages::ladder() {
        let words = realign(&d, &stages);
        let s = stats(&d, &words, &labels);
        let (wm, wl) = s.worst.first().copied().unwrap_or((0, 0));
        println!(
            "{:<20} {:>4} ms {:>4} ms {:>7.0}% {:>+5.0}  {:>5} ms {}{}",
            name,
            s.median,
            s.p90,
            s.within_100,
            s.bias,
            wm,
            d.lines[wl].text.chars().take(24).collect::<String>(),
            if stages == shipped {
                "   <- shipped"
            } else {
                ""
            }
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
