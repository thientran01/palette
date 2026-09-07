# Vocal separation listening trial

Development-only, explicitly enabled with `PALETTE_VOCAL_PREVIEW=1`. Release builds ignore this flag. It uses the same MMS model and word/subword renderer after a local HDemucs vocal-separation pass.

The worker requires absolute `PALETTE_VOCAL_PYTHON`, `PALETTE_VOCAL_SCRIPT`, and `PALETTE_VOCAL_TORCH_HOME` paths. Provision torch/torchaudio 2.8 CPU and the MUSDB-HQ HDemucs weights before launching. The runtime loads local weights with weights_only=True and never downloads assets. The script is scripts/karaoke_vocals_preview.py. Acoustic assets must also be installed/enabled.

## Listening behavior

- First complete visible listen records the song, including songs already holding original timing.
- Existing original timing remains displayed until experimental timing is available.
- The next listen uses the trial timing. Hover the sync icon: Vocal sync saved identifies a completed experimental result; original saved words do not hide trial progress.
- Seek, capture, and complete-listen guards remain unchanged. Failed separation preserves original timing and leaves the trial retryable.
- Turning the trial off uses original timing again. Nothing in the original karaoke cache is overwritten by experimental results.

Trial output uses karaoke-vocals-preview-v1 and recipe mms-int8/demucs-preview-1. The ordinary karaoke cache remains separate. There is no claim of universal accuracy: this is the user-requested broad listening test.

## Processing bounds

The separator uses two CPU threads, ten-second segments with two-second overlap and two seconds of extra context. Weighted overlap-add preserves sample count and smooths chunk joins; normalization uses one final gain. The original capture-to-song time map is retained. Recording input remains mono 16kHz; resampling for the separation model does not restore stereo detail.

One existing alignment worker owns the job. Python exits after each separation pass. A five-minute timeout kills and reaps it; a kill-on-close Windows job binds it to the app, and normal app exit terminates it and removes scratch audio. A parent-handle watchdog covers the startup interval before job assignment. Failed or missing local assets do not produce a preview cache.

## Verification

Overlap reconstruction tests cover short, exact-boundary, and partial-tail recordings. Rust tests verify original/preview cache isolation, fallback after invalid preview data, unchanged PCM size, timeout reaping, kill-on-close behavior, and scratch directory cleanup. Frontend tests verify original saved timing cannot mask trial processing.

A full 187.72-second local recording completed separation with exactly the original sample count. Use the karaoke_vocal_preview example for an end-to-end smoke run through the same native separator and MMS path. No recordings or model files belong in the repository.