# Vocal separation listening trial

Development-only, explicitly enabled with `PALETTE_VOCAL_PREVIEW=1`. Release builds ignore this flag. Separation is native: Kim Vocal 2 (an MDX-Net model from UVR's public model repository) runs through the ONNX Runtime the acoustic aligner already loads, then the same MMS model and word/subword renderer run on the isolated vocals. No Python, torch or child process is involved.

## Setup

Download `Kim_Vocal_2.onnx` from https://github.com/TRvlvr/model_repo/releases/tag/all_public_uvr_models (SHA-256 `ce74ef3b6a6024ce44211a07be9cf8bc6d87728cc852a68ab34eb8e58cde9c8b`) and install it with the acoustic assets:

```
python scripts/install_karaoke_model.py --model <mms> --runtime <onnxruntime.dll> --vocal-model Kim_Vocal_2.onnx --destination <assets>
```

The model is checksum-verified on load and is never bundled or downloaded by the app.

## Listening behavior

- With the trial on, loopback capture also keeps the song as 48kHz stereo, frame-for-frame with the usual 16kHz mono. The downmix the aligner uses throws away stereo and high-frequency detail the separator relies on. If a capture block arrives without stereo (the cpal fallback path), stereo is dropped for that recording and separation runs on the mono PCM instead.
- First complete visible listen records the song, including songs already holding original timing.
- Existing original timing remains displayed until experimental timing is available.
- The next listen uses the trial timing. Hover the sync icon: Vocal sync saved identifies a completed experimental result; original saved words do not hide trial progress.
- Seek, capture, and complete-listen guards remain unchanged. Failed separation preserves original timing and leaves the trial retryable.
- Turning the trial off uses original timing again. Nothing in the original karaoke cache is overwritten by experimental results.

Trial output uses `karaoke-vocals-preview-v4` and recipe `mms-int8/mdx-kim2-4`. The v3/v2/v1 Demucs caches stay readable while songs relearn and are never deleted on mismatch. The ordinary karaoke cache remains separate.

## Processing

`src-tauri/src/separation.rs` follows the reference MDX-Net demixer: resample to 44.1kHz (Kaiser-windowed sinc), STFT with n_fft 7680 / hop 1024 / periodic Hann (centered, reflect padded), keep the lowest 3072 bins with the bottom three zeroed, run the model per 5.9s chunk, inverse STFT, and keep each chunk's middle 5.75s. The two vocal channels are averaged, resampled back to the capture rate, and box-decimated to 16kHz with the recorder's own phase arithmetic, so the vocals land on the recorded PCM's sample grid and the original time map still applies. One global gain (peak 0.95) preserves chunk-to-chunk levels.

The separator shares the acoustic worker thread and its idle expiry, with two ONNX threads. It runs once per song, after the listen is recorded, never during playback.

## Verification

- On a synthetic 12-second mix (formant-filtered pulse "voice" over kick, hats, saw chords and bass), the separated vocal scores 26.1dB SDR against the true voice at 44.1kHz input, 27.6dB at 48kHz stereo, and 23.7dB from 16kHz mono. The mix itself scores −4.4dB. Synthetic audio says the pipeline is wired correctly; it is not evidence about real songs.
- The Rust output matches a NumPy + onnxruntime reference implementation of the same algorithm to 134dB (numerically identical).
- Speed in a 4-vCPU Linux cloud VM with two ONNX threads: about 4.6s per 5.75s chunk, roughly 0.8× realtime, so a 4-minute song takes about 3 minutes in the background. Desktop CPUs are typically faster; measure with the example below.
- Unit tests cover resampling (tone accuracy, anti-aliasing, round-trip alignment), the STFT round trip, the recorder-grid decimation, stereo frame alignment, and cache generations.

Replay a dump through the same path with `cargo run --example karaoke_vocal_preview <dump> <model-dir> <new-output.json>`. Dumps recorded with the trial on include `stereo.i16`; older dumps use the mono PCM. No recordings or model files belong in the repository.
