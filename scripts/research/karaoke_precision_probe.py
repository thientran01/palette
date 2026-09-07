"""Compare local float/int8 MMS exports on selected English rows, offline only.
Requires the isolated MMS lab environment. No downloads or cache writes.
"""
import argparse
import gc
import json
import re
import sys
from pathlib import Path
import numpy as np
import onnxruntime as ort
import torch
import torchaudio

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from karaoke_tap import parse_lrc, legacy_tokens
from research.karaoke_probe_safety import write_new_json

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('dump', type=Path)
p.add_argument('models', type=Path)
p.add_argument('output', type=Path)
p.add_argument('--lines', type=int, nargs='+', required=True)
a = p.parse_args()
if a.output.exists():
    raise FileExistsError('Choose a new output file; existing evidence is protected')
torch.set_num_threads(2)
meta = json.loads((a.dump / 'meta.json').read_text(encoding='utf-8'))
mapping = meta['map']
pcm = np.fromfile(a.dump / 'pcm.i16', dtype='<i2').astype(np.float32) / 32768
lines = parse_lrc((a.dump / 'lyrics.lrc').read_text(encoding='utf-8'))
to_time = lambda sample: round(mapping['intercept_ms'] + sample * mapping['slope_ms'])
to_sample = lambda ms: max(0, min(len(pcm), round((ms - mapping['intercept_ms']) / mapping['slope_ms'])))
bundle = torchaudio.pipelines.MMS_FA
tokenizer, aligner = bundle.get_tokenizer(), bundle.get_aligner()
results = {}
for name in ['mms-fa-int8.onnx', 'mms-fa.onnx']:
    opts = ort.SessionOptions()
    opts.intra_op_num_threads = 2
    opts.inter_op_num_threads = 1
    session = ort.InferenceSession(str(a.models / name), sess_options=opts, providers=['CPUExecutionProvider'])
    rows = []
    for i, (seconds, text) in enumerate(lines):
        stamp = round(seconds * 1000)
        if stamp not in a.lines:
            continue
        tokens = legacy_tokens(text)
        if not text.isascii() or any(c.isdigit() for c in text):
            raise ValueError('Precision probe currently requires ASCII rows without numbers')
        normalized = [re.sub("[^a-z']", '', t.lower()) for t in tokens]
        if any(not t for t in normalized):
            raise ValueError('Unsupported punctuation-only token')
        start = to_sample(stamp - 500)
        next_ms = lines[i + 1][0] * 1000 if i + 1 < len(lines) else to_time(len(pcm))
        end = to_sample(min(next_ms + 500, stamp + 20000))
        if not 400 <= end - start <= 336000:
            raise ValueError('Audio window outside bounds')
        emissions = session.run(['log_probs'], {'waveform': pcm[None, start:end]})[0]
        spans = aligner(torch.from_numpy(emissions[0]), tokenizer(['*'] + normalized + ['*']))[1:-1]
        ratio = (end - start) / emissions.shape[1]
        words = [{'text': token, 't': to_time(start + chars[0].start * ratio),
                  'end': to_time(start + chars[-1].end * ratio)} for token, chars in zip(tokens, spans)]
        rows.append({'line_t': stamp, 'words': words})
        print(name, stamp, words, flush=True)
    if len(rows) != len(set(a.lines)):
        raise ValueError('Requested source rows were not all found')
    results[name] = rows
    del session
    gc.collect()
write_new_json(a.output, results)