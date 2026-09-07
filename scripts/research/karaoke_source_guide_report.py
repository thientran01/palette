"""Compare native entry probe outputs. Reports timing metadata, not lyric text.
Usage: python scripts/research/karaoke_source_guide_report.py BASELINE CANDIDATE
These are source disagreements, not measured vocal accuracy.
"""
import json
import sys
from collections import defaultdict
from pathlib import Path


def compare(before, after):
    baseline = before['words']
    candidate = after['words']
    if [(w.get('line_t'), w['text']) for w in baseline] != [(w.get('line_t'), w['text']) for w in candidate]:
        raise ValueError('Source token ownership differs')
    rows = defaultdict(list)
    for a, b in zip(baseline, candidate):
        rows[a.get('line_t')].append((a, b))
    changed = []
    for stamp, pairs in rows.items():
        if all(a == b for a, b in pairs):
            continue
        changed.append(dict(line=stamp, before_entry=pairs[0][0]['t'], after_entry=pairs[0][1]['t'],
            word_boundaries=[dict(before=[a['t'],a.get('end')],after=[b['t'],b.get('end')]) for a,b in pairs],
            changed_words=sum(a != b for a,b in pairs)))
    return dict(lines=len(rows),words=len(baseline),changed_lines=len(changed),changes=changed)


if __name__ == '__main__':
    print(json.dumps(compare(*(json.loads(Path(p).read_text(encoding='utf-8')) for p in sys.argv[1:3])),indent=2))
