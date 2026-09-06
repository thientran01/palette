//! Run the same development-only separator before native acoustic alignment.
#[path = "../src/vocal_preview.rs"]
#[allow(dead_code)]
mod vocal_preview;
use pulse_lib::{
    acoustic::AcousticAligner,
    align::{parse_lrc, TimeMap},
};
use std::{path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if args.len() != 3 {
        return Err("usage: karaoke_vocal_preview <dump> <model-dir> <new-output.json>".into());
    }
    if args[2].exists() {
        return Err("output must be new".into());
    }
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(args[0].join("meta.json"))?)?;
    let map: TimeMap = serde_json::from_value(meta["map"].clone())?;
    let raw = std::fs::read(args[0].join("pcm.i16"))?;
    if raw.len() % 2 != 0 {
        return Err("truncated PCM".into());
    }
    let pcm: Vec<i16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect();
    let started = Instant::now();
    let vocals = vocal_preview::separate(&pcm)?;
    let separated = started.elapsed().as_secs_f64();
    let mut model = AcousticAligner::load(
        &args[1].join("mms-fa-int8.onnx"),
        &args[1].join("onnxruntime.dll"),
    )?;
    let lines = parse_lrc(&std::fs::read_to_string(args[0].join("lyrics.lrc"))?);
    let words = model.align(&vocals, &lines, &map)?;
    let result = serde_json::json!({"recipe":vocal_preview::RECIPE,"separation_seconds":separated,"total_seconds":started.elapsed().as_secs_f64(),"words":words});
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    use std::io::Write;
    output.write_all(&serde_json::to_vec_pretty(&result)?)?;
    println!(
        "{} words; separated in {:.2}s; total {:.2}s",
        words.len(),
        separated,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
