//! Opt-in development-only vocal separation. Never enabled in release builds.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[path = "preview_lifetime.rs"]
mod lifetime;
pub fn shutdown() {
    lifetime::shutdown();
}

pub const CACHE_DIR: &str = "karaoke-vocals-preview-v2";
pub const RECIPE: &str = "mms-int8/demucs-entry-2";
pub const PREVIOUS_CACHE_DIR: &str = "karaoke-vocals-preview-v1";

pub fn enabled() -> bool {
    cfg!(debug_assertions) && std::env::var("PALETTE_VOCAL_PREVIEW").is_ok_and(|v| v == "1")
}

fn cache_is_preview(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == CACHE_DIR)
}

pub fn cache_recipe<'a>(path: &Path, baseline: &'a str) -> &'a str {
    if path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == PREVIOUS_CACHE_DIR)
    {
        "mms-int8/demucs-preview-1"
    } else if cache_is_preview(path) {
        RECIPE
    } else {
        baseline
    }
}

struct Job(PathBuf);
impl Job {
    fn new() -> Result<Self, String> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "palette-vocal-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("could not allocate vocal preview workspace".into())
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait_bounded(child: &mut Child, timeout: Duration) -> Result<(), String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("vocal separation exited {status}"))
                }
            }
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(100))
            }
            outcome => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match outcome {
                    Err(e) => format!("vocal worker wait failed: {e}"),
                    _ => "vocal separation timed out after five minutes".into(),
                });
            }
        }
    }
}

fn decode_output(bytes: &[u8], samples: usize) -> Result<Vec<i16>, String> {
    if bytes.len() != samples.checked_mul(2).ok_or("PCM size overflow")? {
        return Err("vocal separation changed recording length".into());
    }
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect())
}

pub fn separate(pcm: &[i16]) -> Result<Vec<i16>, String> {
    if !enabled() {
        return Err("vocal preview is not enabled".into());
    }
    if !(400..=16_000 * 60 * 8).contains(&pcm.len()) {
        return Err("invalid vocal preview PCM size".into());
    }
    let python = std::env::var_os("PALETTE_VOCAL_PYTHON")
        .map(PathBuf::from)
        .ok_or("missing preview Python")?;
    let script = std::env::var_os("PALETTE_VOCAL_SCRIPT")
        .map(PathBuf::from)
        .ok_or("missing preview script")?;
    let models = std::env::var_os("PALETTE_VOCAL_TORCH_HOME")
        .map(PathBuf::from)
        .ok_or("missing preview model cache")?;
    if !python.is_absolute()
        || !python.is_file()
        || !script.is_absolute()
        || !script.is_file()
        || !models.is_absolute()
        || !models.is_dir()
    {
        return Err("invalid local vocal preview assets".into());
    }
    let job = Job::new()?;
    let input = job.0.join("input.i16");
    let output = job.0.join("vocals.i16");
    let raw: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    fs::write(&input, raw).map_err(|e| e.to_string())?;
    let stderr = fs::File::create(job.0.join("worker.log")).map_err(|e| e.to_string())?;
    let mut command = Command::new(python);
    command
        .arg(script)
        .arg(&input)
        .arg(&output)
        .env("TORCH_HOME", models)
        .env("PYTHONUTF8", "1")
        .env("PALETTE_PARENT_PID", std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let started = Instant::now();
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let _lifetime = match lifetime::Lifetime::attach(&child, &job.0) {
        Ok(binding) => binding,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    wait_bounded(&mut child, Duration::from_secs(300))?;
    let size = fs::metadata(&output).map_err(|e| e.to_string())?.len();
    if size != (pcm.len() as u64) * 2 {
        return Err("vocal output has unexpected size".into());
    }
    let bytes = fs::read(output).map_err(|e| e.to_string())?;
    let vocals = decode_output(&bytes, pcm.len())?;
    log::info!(
        "karaoke: vocal preview separated {:.1}s audio in {:.2}s",
        pcm.len() as f64 / 16000.0,
        started.elapsed().as_secs_f64()
    );
    Ok(vocals)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(windows)]
    fn timed_out_worker_is_reaped() {
        use std::os::windows::process::CommandExt;
        let mut child = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .creation_flags(0x08000000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert!(wait_bounded(&mut child, Duration::from_millis(20)).is_err());
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    #[cfg(windows)]
    fn dropping_job_binding_terminates_worker() {
        use std::os::windows::process::CommandExt;
        let directory = Job::new().unwrap();
        let mut child = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .creation_flags(0x08000000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let binding = lifetime::Lifetime::attach(&child, &directory.0).unwrap();
        assert!(child.try_wait().unwrap().is_none());
        let started = Instant::now();
        drop(binding);
        // Windows may report exit code zero for kill-on-job-close.
        let _ = wait_bounded(&mut child, Duration::from_secs(3));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn cache_identity_is_separate_from_original() {
        assert_eq!(
            cache_recipe(Path::new("root/karaoke/song.json"), "original"),
            "original"
        );
        assert_eq!(
            cache_recipe(
                &Path::new("root").join(CACHE_DIR).join("song.json"),
                "original"
            ),
            RECIPE
        );
    }
    #[test]
    fn output_preserves_signed_pcm_and_rejects_truncation() {
        assert_eq!(
            decode_output(&[0, 128, 255, 127], 2).unwrap(),
            [-32768, 32767]
        );
        assert!(decode_output(&[0, 128, 1], 2).is_err());
        assert!(decode_output(&[0, 128, 1, 0, 0, 0], 2).is_err());
    }
    #[test]
    fn job_cleanup_removes_only_its_owned_directory() {
        let job = Job::new().unwrap();
        let path = job.0.clone();
        fs::write(path.join("fixture"), b"test").unwrap();
        drop(job);
        assert!(!path.exists());
    }
}
