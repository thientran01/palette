"""Audit offline acoustic probe output; never reads/writes the app cache.

python scripts/karaoke_audit.py probe.json --meta dump/meta.json --labels labels.txt --lead-ms 220
Support is CTC path support, NOT a calibrated probability of correct timing.
"""
import argparse
import json
import math
import statistics
from pathlib import Path


def onset_metrics(words, label_text, time_map, source_tokens, lead_ms=0):
    song_clock = any(line.strip() == '# clock: song' for line in label_text.splitlines())
    labels = [line.split('\t') for line in label_text.splitlines()
              if line.strip() and not line.strip().startswith('#')]
    if not labels or len(labels) > len(words):
        raise ValueError('Labels must be a nonempty prefix of predicted words')
    # Match source occurrences, not just repeated spelling. Missing labeled
    # words must fail rather than shift later repeated lyrics into their place.
    predictions = {}
    cursor = 0
    for word in words:
        while cursor < len(source_tokens) and (
            source_tokens[cursor]['line_t'] != word.get('line_t') or
            source_tokens[cursor]['text'].strip() != word['text'].strip()
        ):
            cursor += 1
        if cursor == len(source_tokens):
            raise ValueError('Prediction identity/order differs from source')
        predictions[cursor] = word
        cursor += 1
    deltas = []
    previous = -math.inf
    for i, fields in enumerate(labels):
        if i not in predictions or i >= len(source_tokens):
            raise ValueError('Missing labeled source occurrence')
        if len(fields) != 3 or fields[2].strip() != source_tokens[i]['text'].strip():
            raise ValueError(f'Label identity mismatch at row {i + 1}')
        seconds = float(fields[0])
        if not math.isfinite(seconds) or seconds < 0:
            raise ValueError('Invalid label time')
        truth = seconds * 1000 if song_clock else (
            time_map['intercept_ms'] + math.floor(seconds * 16000 + .5) * time_map['slope_ms'])
        truth = math.floor(truth + .5)
        if truth < previous:
            raise ValueError('Labels run backward')
        previous = truth
        deltas.append(predictions[i]['t'] - lead_ms - truth)
    absolute = sorted(abs(d) for d in deltas)
    percentile = lambda p: absolute[math.floor((len(absolute) - 1) * p + .5)]
    return {'n': len(deltas), 'median_ms': percentile(.5), 'p90_ms': percentile(.9),
            'within_100_pct': 100 * sum(d <= 100 for d in absolute) / len(deltas),
            'bias_ms': statistics.mean(deltas)}


def audit(probe):
    rows = []
    for row in probe['diagnostics']:
        spans = row['spans'][1:-1]  # omit the two wildcard targets
        support = [s['support'] for s in spans]
        rows.append({'line_t': row['line_t'],
                     'support_median': statistics.median(support) if support else None,
                     'first_ms_from_window_start': spans[0]['start'] - row['window_start'] if spans else None,
                     'last_ms_to_window_end': row['window_end'] - spans[-1]['end'] if spans else None})
    return {'margin_ms': probe['margin_ms'], 'words': len(probe['words']), 'rows': rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('probe', type=Path)
    parser.add_argument('--meta', type=Path)
    parser.add_argument('--labels', type=Path)
    parser.add_argument('--lead-ms', type=int, default=0)
    args = parser.parse_args()
    probe = json.loads(args.probe.read_text(encoding='utf-8'))
    result = audit(probe)
    if args.labels:
        if not args.meta:
            parser.error('--labels requires --meta')
        meta = json.loads(args.meta.read_text(encoding='utf-8'))
        result['onsets'] = onset_metrics(probe['words'], args.labels.read_text(encoding='utf-8'), meta['map'], probe['source_tokens'], args.lead_ms)
        result['lead_ms'] = args.lead_ms
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()