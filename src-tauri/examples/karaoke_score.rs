//! Offline scorer for the word-karaoke aligner (docs/specs/2026-09-04).
//!
//! Input is one evidence dump the app writes when settings.json has
//! `"karaokeDump": true` (app-data/karaoke-dumps/<key>/): pcm.i16,
//! lyrics.lrc, words.json, meta.json.
//!
//!   cargo run --example karaoke_score -- template <dump-dir>
//!       re-runs the CURRENT aligner on the dump and writes
//!       <dump-dir>/labels.template.txt — an Audacity label track with one
//!       row per source token in captured lines at the aligner's guess.
//!       A JSON sidecar carries song times and explicit line identity.
//!       Label rows follow source order, including unpredicted words.
//!       Unmarked labels use audio time; # clock: song declares song time.
//!
//!   cargo run --example karaoke_score -- score <dump-dir> <labels.txt>
//!       re-runs the shipped aligner, pairs token i with label row i, and
//!       prints median/p90 |Δ|, share within 100ms, signed bias, the five
//!       worst lines, the map's fit residual (anchor-fit residual, kept separate
//!       from aligner error), and how far the replay sits from the dump's
//!       live words.json. A partial label file scores its prefix.
//!
//!   cargo run --example karaoke_score -- matrix <dump-dir> <labels.txt>
//!       one row per rung of the ladder (Stages::ladder()), the shipped
//!       set marked — the table that decides what ships.

use pulse_lib::align::{self, PriorParams, Stages, TimeMap, TimedLine, Word};
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
        [cmd, dirs @ ..] if cmd == "fit" && !dirs.is_empty() => fit(dirs),
        _ => {
            eprintln!(
                "usage: karaoke_score template <dump-dir>\n       karaoke_score score  <dump-dir> <labels.txt>\n       karaoke_score matrix <dump-dir> <labels.txt>\n       karaoke_score fit    <dump-dir>... (each with labels.txt)"
            );
            exit(2);
        }
    }
}

fn load(dir: &Path) -> Dump {
    let bytes = read(dir.join("pcm.i16"));
    let (chunks, _) = bytes.as_chunks::<2>();
    let pcm: Vec<f32> = chunks
        .iter()
        .map(|c| i16::from_le_bytes(*c) as f32 / 32768.0)
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
    let mut tokens = Vec::new();
    for (line_index, line) in d.lines.iter().enumerate() {
        if line.t >= d.meta.map.pos_ms(d.pcm.len()) {
            break;
        }
        let predicted: Vec<_> = words.iter().filter(|w| w.line_t == Some(line.t)).collect();
        for (i, text) in align::tokenize(&line.text).into_iter().enumerate() {
            let t = predicted.get(i).map(|w| w.t).unwrap_or(line.t);
            // Audacity's waveform begins at capture sample zero, not song zero.
            let seconds = d.meta.map.sample_at(t) as f64 / RATE as f64;
            out.push_str(&format!("{seconds:.3}\t{seconds:.3}\t{}\n", text.trim()));
            tokens.push(serde_json::json!({
                "t": t, "text": text, "line_t": line.t, "line_index": line_index
            }));
        }
    }
    let path = dir.join("labels.template.txt");
    std::fs::write(&path, out).unwrap_or_else(|e| die(&e.to_string()));
    // The tap tool uses song timestamps and explicit line identity from this
    // sidecar; the text template remains directly importable into Audacity.
    std::fs::write(
        dir.join("labels.template.json"),
        serde_json::to_vec(&tokens).unwrap(),
    )
    .unwrap_or_else(|e| die(&e.to_string()));
    println!(
        "{} — {} tokens → {}",
        d.meta.title,
        tokens.len(),
        path.display()
    );
}

/// Legacy tap exports and Audacity labels use recording-relative seconds.
/// New tap exports declare `# clock: song`. Never compare the two clocks raw.
fn parse_labels(text: &str, map: &TimeMap) -> Result<Vec<i64>, String> {
    let song_clock = text.lines().any(|l| l.trim() == "# clock: song");
    let mut labels = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let seconds: f64 = line
            .split('\t')
            .next()
            .unwrap_or("")
            .parse()
            .map_err(|_| format!("labels row {}: invalid timestamp", i + 1))?;
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(format!(
                "labels row {}: timestamp must be finite and nonnegative",
                i + 1
            ));
        }
        let t = if song_clock {
            (seconds * 1000.0).round() as i64
        } else {
            map.pos_ms((seconds * RATE as f64).round() as usize)
        };
        if labels.last().is_some_and(|prev| *prev > t) {
            return Err(format!("labels row {}: timestamps run backward", i + 1));
        }
        labels.push(t);
    }
    if labels.is_empty() {
        return Err("label file has no rows".into());
    }
    Ok(labels)
}

fn validated_labels(text: &str, map: &TimeMap, lines: &[TimedLine]) -> Result<Vec<i64>, String> {
    let labels = parse_labels(text, map)?;
    let tokens: Vec<_> = lines
        .iter()
        .flat_map(|l| align::tokenize(&l.text))
        .collect();
    let rows = text
        .lines()
        .filter(|row| !row.trim().is_empty() && !row.trim().starts_with('#'));
    for (i, row) in rows.enumerate() {
        let fields: Vec<_> = row.split('\t').collect();
        let Some(expected) = tokens.get(i) else {
            return Err(format!(
                "label row {} exceeds the source token count",
                i + 1
            ));
        };
        if fields.len() != 3 || fields[2].trim() != expected.trim() {
            return Err(format!("label row {} does not match source token {:?}; labels must be a prefix in source order", i + 1, expected.trim()));
        }
    }
    Ok(labels)
}

fn read_labels(path: &Path, d: &Dump) -> Vec<i64> {
    let text = String::from_utf8_lossy(&read(path.to_path_buf())).into_owned();
    validated_labels(&text, &d.meta.map, &d.lines).unwrap_or_else(|e| die(&e))
}

struct Stats {
    median: i64,
    p90: i64,
    within_100: f64,
    bias: f64,
    /// (mean |Δ| ms, line index), worst first.
    worst: Vec<(i64, usize)>,
}

/// Each aligner token's line index, from the lyrics themselves (the
/// aligner emits tokens line by line, in `tokenize` order) — never from
/// the token's TIME, which a candidate aligner may push past the next
/// stamp.
fn token_lines(d: &Dump) -> Vec<usize> {
    let mut out = Vec::new();
    for (li, line) in d.lines.iter().enumerate() {
        for _ in align::tokenize(&line.text) {
            out.push(li);
        }
    }
    out
}

/// Pairs token i with label i (a partial file scores its prefix); dies if a
/// label sits outside its token's LRC line window (rows drifted).
fn prediction_indices(d: &Dump, words: &[Word]) -> Result<Vec<usize>, String> {
    let expected: Vec<_> = d
        .lines
        .iter()
        .flat_map(|l| align::tokenize(&l.text).into_iter().map(move |t| (l.t, t)))
        .collect();
    let mut cursor = 0;
    let mut indices = Vec::new();
    for word in words {
        let offset = expected[cursor..]
            .iter()
            .position(|(stamp, text)| word.line_t == Some(*stamp) && word.text == *text)
            .ok_or_else(|| {
                "candidate word identity/order differs from source lyrics".to_string()
            })?;
        cursor += offset;
        indices.push(cursor);
        cursor += 1;
    }
    Ok(indices)
}

fn stats(d: &Dump, words: &[Word], labels: &[i64]) -> Stats {
    let tl = token_lines(d);
    if labels.len() > tl.len() {
        die("more labels than source lyric tokens");
    }
    let indices = prediction_indices(d, words).unwrap_or_else(|e| die(&e));
    let mut predicted = vec![None; tl.len()];
    for (word, i) in words.iter().zip(indices) {
        predicted[i] = Some(word);
    }
    let mut deltas = Vec::new();
    for (i, &label) in labels.iter().enumerate() {
        // Missing UNLABELLED tail words are okay for partial evidence. A
        // candidate cannot improve its score by dropping labelled words.
        let word = predicted[i].unwrap_or_else(|| {
            die(&format!(
                "label row {} has no predicted word (outside captured audio or line-only fallback)",
                i + 1
            ))
        });
        deltas.push(word.t - label);
    }
    let mut abs: Vec<i64> = deltas.iter().map(|d| d.abs()).collect();
    abs.sort_unstable();
    let pct = |p: f64| abs[((abs.len() - 1) as f64 * p).round() as usize];
    let within = abs.iter().filter(|&&a| a <= 100).count();
    let mut per_line: Vec<(usize, Vec<i64>)> = Vec::new();
    for (i, &delta) in deltas.iter().enumerate() {
        let li = tl[i];
        match per_line.last_mut() {
            Some((cur, v)) if *cur == li => v.push(delta),
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
    let labels = read_labels(labels_path, &d);
    let s = stats(&d, &words, &labels);

    println!("{} — {}", d.meta.artist, d.meta.title);
    println!("tokens        {}", words.len());
    if labels.len() < words.len() {
        println!(
            "partial       {} of {} tokens marked (through line {})",
            labels.len(),
            words.len(),
            token_lines(&d)[labels.len() - 1] + 1
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
        .filter(|(a, b)| a != b)
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
            "replay        {} of {} tokens differ from live, max onset change {} ms (end/text/line identity differences also count)",
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
    let labels = read_labels(labels_path, &d);
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

/// Grid-search the prior's constants across every truth song. Reports;
/// never writes — updating LEAD_MS / RATE_SYL_S / FILL stays a human
/// commit with this table in its message.
fn fit(dirs: &[String]) {
    let songs: Vec<(Dump, Vec<i64>)> = dirs
        .iter()
        .map(|d| {
            let dir = Path::new(d);
            let dump = load(dir);
            let labels = read_labels(&dir.join("labels.txt"), &dump);
            (dump, labels)
        })
        .collect();
    let names: Vec<String> = songs.iter().map(|(d, _)| d.meta.title.clone()).collect();
    let medians = |stages: &Stages, params: &PriorParams| -> Vec<i64> {
        songs
            .iter()
            .map(|(d, labels)| {
                let words =
                    align::align_with_params(&d.pcm, RATE, &d.lines, &d.meta.map, stages, params);
                stats(d, &words, labels).median
            })
            .collect()
    };
    let mean = |v: &[i64]| v.iter().sum::<i64>() as f64 / v.len() as f64;
    let defaults = PriorParams::default();
    println!("songs: {}", names.join(", "));

    // Shipped stages (song lead calibrated per song): rate × fill.
    let shipped = Stages::shipped();
    let base = medians(&shipped, &defaults);
    let mut best = (mean(&base), defaults, base.clone());
    for rate10 in 15..=60 {
        for fill20 in 10..=20 {
            let params = PriorParams {
                rate_syl_s: rate10 as f32 / 10.0,
                fill: fill20 as f32 / 20.0,
                ..defaults
            };
            let m = medians(&shipped, &params);
            let score = mean(&m);
            if score < best.0 {
                best = (score, params, m);
            }
        }
    }
    println!(
        "shipped stages, defaults  rate {:.1} fill {:.2}: {:?} ms (mean {:.0})",
        defaults.rate_syl_s,
        defaults.fill,
        base,
        mean(&base)
    );
    println!(
        "shipped stages, best      rate {:.1} fill {:.2}: {:?} ms (mean {:.0})",
        best.1.rate_syl_s, best.1.fill, best.2, best.0
    );

    // Fallback lead (PRIOR_ONLY, no calibration): the default when a song
    // has too few lines to vote.
    let prior = Stages::PRIOR_ONLY;
    let base = medians(&prior, &defaults);
    let mut best_lead = (mean(&base), defaults.lead_ms, base.clone());
    for lead in (-100..=600).step_by(10) {
        let params = PriorParams {
            lead_ms: lead,
            ..defaults
        };
        let m = medians(&prior, &params);
        let score = mean(&m);
        if score < best_lead.0 {
            best_lead = (score, lead, m);
        }
    }
    println!(
        "prior only, default lead {}ms: {:?} ms (mean {:.0})",
        defaults.lead_ms,
        base,
        mean(&base)
    );
    println!(
        "prior only, best lead    {}ms: {:?} ms (mean {:.0})",
        best_lead.1, best_lead.2, best_lead.0
    );
}

fn fmt_ms(ms: i64) -> String {
    format!(
        "{:02}:{:02}.{:02}",
        ms / 60_000,
        (ms / 1000) % 60,
        (ms % 1000) / 10
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recording_labels_convert_to_song_time_but_declared_song_labels_do_not() {
        let map = TimeMap::from_origin(750, RATE);
        assert_eq!(
            parse_labels("1.000\t1.000\tone\n", &map).unwrap(),
            vec![1750]
        );
        assert_eq!(
            parse_labels("# clock: song\n1.750\t1.750\tone\n", &map).unwrap(),
            vec![1750]
        );
    }
    #[test]
    fn shifted_label_rows_are_rejected_but_a_source_prefix_scores() {
        let map = TimeMap::from_origin(0, RATE);
        let lines = align::parse_lrc("[00:01.00]first\n[00:10.00]second\n[00:20.00]third");
        assert!(validated_labels("20.0\t20.0\tthird", &map, &lines).is_err());
        assert!(validated_labels("1.0\t1.0\tfirst\n20.0\t20.0\tthird", &map, &lines).is_err());
        assert_eq!(
            validated_labels("1.0\t1.0\tfirst\n10.0\t10.0\tsecond", &map, &lines).unwrap(),
            vec![1000, 10000]
        );
        // Large timing error is valid evidence when source identity is right.
        assert_eq!(
            validated_labels("20.0\t20.0\tfirst", &map, &lines).unwrap(),
            vec![20000]
        );
    }

    #[test]
    fn malformed_or_backward_truth_is_rejected() {
        let map = TimeMap::from_origin(0, RATE);
        for raw in ["NaN\tNaN\tx", "-1\t-1\tx", "2\t2\tx\n1\t1\ty"] {
            assert!(parse_labels(raw, &map).is_err());
        }
    }
    #[test]
    fn partial_predictions_score_a_labelled_prefix_without_the_unheard_tail() {
        let map = TimeMap::from_origin(0, RATE);
        let d = Dump {
            pcm: vec![0.0; RATE as usize * 3],
            lines: align::parse_lrc("[00:01.00]one two\n[00:05.00]last"),
            meta: Meta {
                artist: String::new(),
                title: String::new(),
                map,
            },
            live: vec![],
        };
        let words = realign(&d, &Stages::PRIOR_ONLY);
        assert_eq!(words.len(), 2);
        let labels: Vec<_> = words.iter().map(|w| w.t).collect();
        assert_eq!(stats(&d, &words, &labels).median, 0);
        assert_eq!(prediction_indices(&d, &words).unwrap(), vec![0, 1]);
    }
}
