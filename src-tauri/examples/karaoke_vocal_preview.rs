//! Replay a dump through the vocal trial's native separator and the same
//! guided acoustic alignment the app runs. Uses `stereo.i16` when the dump
//! has it (recorded with the trial on), else the 16kHz mono PCM.
use pulse_lib::{
    acoustic::AcousticAligner,
    align::{parse_lrc, TimeMap},
    separation::{Stereo, VocalSeparator, MODEL_FILE},
};
use std::{path::PathBuf, time::Instant};

fn read_i16(path: &std::path::Path) -> Result<Vec<i16>, Box<dyn std::error::Error>> {
    let raw = std::fs::read(path)?;
    if raw.len() % 2 != 0 {
        return Err("truncated PCM".into());
    }
    Ok(raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect())
}

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
    let pcm = read_i16(&args[0].join("pcm.i16"))?;
    let stereo_path = args[0].join("stereo.i16");
    let stereo = if stereo_path.is_file() {
        Some(Stereo {
            interleaved: read_i16(&stereo_path)?,
            rate: meta["rate_in"].as_u64().ok_or("meta.json lacks rate_in")? as u32,
        })
    } else {
        None
    };
    // Loading the aligner first initializes the ONNX Runtime.
    let mut model = AcousticAligner::load(
        &args[1].join("mms-fa-int8.onnx"),
        &args[1].join("onnxruntime.dll"),
    )?;
    let mut separator = VocalSeparator::load(&args[1].join(MODEL_FILE))?;
    let started = Instant::now();
    let vocals = separator.vocals_16k(stereo.as_ref(), &pcm, 16_000)?;
    let separated = started.elapsed().as_secs_f64();
    let lines = parse_lrc(&std::fs::read_to_string(args[0].join("lyrics.lrc"))?);
    let words = model.align_guided_entries(&vocals, &lines, &map, None)?;
    let result = serde_json::json!({
        "recipe": "mms-int8/mdx-kim2-4",
        "input": if stereo.is_some() { "stereo" } else { "mono" },
        "separation_seconds": separated,
        "total_seconds": started.elapsed().as_secs_f64(),
        "words": words,
    });
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    use std::io::Write;
    output.write_all(&serde_json::to_vec_pretty(&result)?)?;
    println!(
        "{} words from {} input; separated in {:.2}s; total {:.2}s",
        words.len(),
        if stereo.is_some() { "stereo" } else { "mono" },
        separated,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
