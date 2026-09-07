//! Offline parity probe: supplied PCM must already be separated vocals.
#[path = "../src/entry_timing.rs"]
#[allow(dead_code)]
mod entry_timing;
use pulse_lib::align::{self, TimeMap, Word};
use std::path::PathBuf;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if args.len() != 4 {
        return Err(
            "usage: karaoke_entry_probe <meta.json> <separated.i16> <words.json> <new-output.json>"
                .into(),
        );
    }
    if args[3].exists() {
        return Err("output exists".into());
    }
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let map: TimeMap = serde_json::from_value(meta["map"].clone())?;
    let raw = std::fs::read(&args[1])?;
    if raw.len() % 2 != 0 {
        return Err("truncated PCM".into());
    }
    let pcm: Vec<i16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let mut words: Vec<Word> = serde_json::from_value(stored["words"].clone())?;
    entry_timing::refine(&pcm, &map, &mut words);
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[3])?
        .write_all(&serde_json::to_vec_pretty(&words)?)?;
    Ok(())
}
