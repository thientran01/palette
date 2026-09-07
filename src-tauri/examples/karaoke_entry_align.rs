//! Native entry-trial probe. Input PCM must already be separated vocals.
//! karaoke_acoustic <dump-dir> <model.onnx> <onnxruntime.dll> <output.json>
use pulse_lib::{
    acoustic::AcousticAligner,
    align::{parse_lrc, tokenize, TimeMap},
};
use std::{path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if args.len() != 4 {
        return Err("usage: karaoke_acoustic <dump> <model> <runtime> <output>".into());
    }
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(args[0].join("meta.json"))?)?;
    let map: TimeMap = serde_json::from_value(meta["map"].clone())?;
    let raw = std::fs::read(args[0].join("pcm.i16"))?;
    if !raw.len().is_multiple_of(2) {
        return Err("truncated PCM".into());
    }
    let pcm: Vec<i16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    let mut lines = parse_lrc(&std::fs::read_to_string(args[0].join("lyrics.lrc"))?);
    if let Ok(value) = std::env::var("PALETTE_ENTRY_LINE_MS") {
        let stamp: i64 = value.parse()?;
        let i = lines
            .iter()
            .position(|l| l.t == stamp)
            .ok_or("line absent")?;
        let next = lines.get(i + 1).ok_or("following line absent")?.t;
        lines = vec![
            lines[i].clone(),
            pulse_lib::align::TimedLine {
                t: next,
                text: String::new(),
            },
        ];
    }
    let start = Instant::now();
    let threads = std::env::var("PALETTE_BENCH_THREADS")
        .ok()
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(2);
    let spinning = std::env::var("PALETTE_BENCH_SPIN").map_or(true, |s| s != "0");
    let mut aligner = AcousticAligner::load_tuned(&args[1], &args[2], threads, spinning)?;
    let loaded = start.elapsed().as_secs_f64();
    let margin = std::env::var("PALETTE_PROBE_MARGIN_MS")
        .ok()
        .map(|s| s.parse::<f64>())
        .transpose()?
        .unwrap_or(500.0);
    let mut diagnostics = Vec::new();
    let guide = std::env::var_os("PALETTE_SOURCE_GUIDE").is_some();
    let words = if guide {
        aligner.align_guided_entries(&pcm, &lines, &map, Some(&mut diagnostics))?
    } else {
        aligner.align_entries(&pcm, &lines, &map)?
    };
    let seconds = start.elapsed().as_secs_f64() - loaded;
    if std::env::var_os("PALETTE_BENCH_REPEAT").is_some() {
        let again = Instant::now();
        let repeated = if guide {
            aligner.align_guided_entries(&pcm, &lines, &map, None)?
        } else {
            aligner.align_entries(&pcm, &lines, &map)?
        };
        assert_eq!(words, repeated, "model reuse changed timings");
        eprintln!(
            "warm align {:.3}s; identical words",
            again.elapsed().as_secs_f64()
        );
    }
    let source_tokens: Vec<_> = lines
        .iter()
        .flat_map(|line| {
            tokenize(&line.text)
                .into_iter()
                .map(move |text| serde_json::json!({"line_t": line.t, "text": text}))
        })
        .collect();
    let result = serde_json::json!({"model":"native-mms-int8", "load_seconds":loaded, "seconds":seconds, "words":words, "margin_ms":margin, "diagnostics":diagnostics, "source_tokens":source_tokens});
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[3])?
        .write_all(&serde_json::to_vec_pretty(&result)?)?;
    println!(
        "{} words; load {:.2}s; align {:.2}s; {}",
        words.len(),
        loaded,
        seconds,
        args[3].display()
    );
    Ok(())
}
