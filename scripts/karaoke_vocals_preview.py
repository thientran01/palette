"""Local-only development worker: bounded overlapping vocal separation.
No automatic model downloads. Run with the pre-provisioned lab TORCH_HOME.
"""
import argparse
import os
from pathlib import Path

RATE = 16000


def windows(count, segment=RATE * 10, stride=RATE * 8, context=RATE * 2):
    if count <= 0 or not 0 < stride < segment or context < 0:
        raise ValueError('Invalid chunk configuration')
    for start in range(0, count, stride):
        end = min(count, start + segment)
        yield start, end, max(0, start - context), min(count, end + context)
        if end == count:
            break


def weights(length, overlap, fade_in, fade_out):
    result = [1.0] * length
    width = min(length, overlap)
    if width <= 0:
        raise ValueError('Invalid overlap')
    for i in range(width):
        if fade_in:
            result[i] *= i / width
        if fade_out:
            result[length - width + i] *= (width - i) / width
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('input', type=Path)
    p.add_argument('output', type=Path)
    a = p.parse_args()
    if a.output.exists():
        raise FileExistsError('Output must be new')
    size = a.input.stat().st_size
    if size % 2 or not 800 <= size <= RATE * 60 * 8 * 2:
        raise ValueError('Invalid PCM size')
    if os.name == 'nt' and os.environ.get('PALETTE_PARENT_PID'):
        # Covers the small process-startup interval before Rust assigns its job.
        import ctypes
        import threading
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        kernel.OpenProcess.argtypes = [ctypes.c_uint, ctypes.c_int, ctypes.c_uint]
        kernel.OpenProcess.restype = ctypes.c_void_p
        kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint]
        kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        parent = kernel.OpenProcess(0x00100000, False, int(os.environ['PALETTE_PARENT_PID']))
        if not parent:
            raise RuntimeError('Preview host has exited')
        def watch_parent():
            kernel.WaitForSingleObject(parent, 0xFFFFFFFF)
            kernel.CloseHandle(parent)
            os._exit(1)
        threading.Thread(target=watch_parent, daemon=True).start()
    import numpy as np
    import torch
    import torchaudio
    torch.set_num_threads(2)
    torch.set_num_interop_threads(1)
    # Construct the documented bundle architecture from provisioned weights.
    # Do not call get_model(): it can download assets when the cache is absent.
    asset = Path(os.environ['TORCH_HOME']) / 'hub/torchaudio/models/hdemucs_high_musdbhq_only.pt'
    if not asset.is_file():
        raise FileNotFoundError('Vocal model missing from local TORCH_HOME')
    model = torchaudio.models.hdemucs_high(sources=['drums', 'bass', 'other', 'vocals'])
    model.load_state_dict(torch.load(asset, map_location='cpu', weights_only=True))
    model.eval()
    vocal_index = model.sources.index('vocals')
    pcm = np.fromfile(a.input, dtype='<i2').astype(np.float32) / 32768
    mixed = np.zeros_like(pcm)
    total = np.zeros_like(pcm)
    for start, end, lo, hi in windows(len(pcm)):
        wave = torch.from_numpy(pcm[lo:hi]).unsqueeze(0)
        wave = torchaudio.functional.resample(wave, RATE, 44100).repeat(2, 1).unsqueeze(0)
        with torch.inference_mode():
            vocal = model(wave)[0, vocal_index].mean(dim=0, keepdim=True)
            vocal = torchaudio.functional.resample(vocal, 44100, RATE)[0].numpy()
        if len(vocal) < hi - lo or not np.isfinite(vocal).all():
            raise ValueError('Invalid separated audio')
        core = vocal[start - lo:end - lo]
        weight = np.asarray(weights(end - start, RATE * 2, start > 0, end < len(pcm)), dtype=np.float32)
        mixed[start:end] += core * weight
        total[start:end] += weight
        print(f'separated {end / RATE:.1f}/{len(pcm) / RATE:.1f}s', flush=True)
    if not np.all(total > 0):
        raise ValueError('Uncovered audio samples')
    mixed /= total
    if not np.isfinite(mixed).all():
        raise ValueError('Nonfinite vocals')
    # One global gain preserves chunk-to-chunk amplitude relationships.
    mixed *= min(1.0, .95 / max(float(np.max(np.abs(mixed))), 1e-8))
    data = (mixed * 32768).clip(-32768, 32767).astype('<i2').tobytes()
    if len(data) != size:
        raise ValueError('Sample count changed')
    with a.output.open('xb') as out:
        out.write(data)


if __name__ == '__main__':
    main()