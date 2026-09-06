"""Lightweight safety checks shared by offline research tools."""
import json


def write_new_json(path, value):
    # Exclusive creation also rejects existing symlinks and protects evidence.
    with path.open('x', encoding='utf-8') as output:
        json.dump(value, output, indent=2)


def validate_disjoint_windows(windows):
    ordered = sorted(windows)
    if any(start >= end for start, end in ordered):
        raise ValueError('Empty audio window')
    if any(current[0] < previous[1] for previous, current in zip(ordered, ordered[1:])):
        raise ValueError('Selected alignment windows overlap; use separate output fixtures')