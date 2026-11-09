"""Split Pretendard JP into three script files for the webviews.

Pretendard JP ships as one 5.2 MB variable woff2. Every window that needs a
single glyph Geist lacks (a Hangul title, a kana lyric, even the ✓ on
"Connected ✓") downloads and decodes all of it: about 18 MB of renderer
memory per window, plus a 5.2 MB read through the app's asset protocol.

The package's own dynamic subsets are no fix: they cut the font into ~120
frequency-ordered chunks, and kerning never crosses a chunk boundary, so
Korean lines set visibly looser (measured ~10% wider). This script cuts by
SCRIPT instead, so every pair that kerns stays inside one file:

  hangul    Hangul syllables + jamo                       ~1.7 MB
  japanese  kana, kanji, CJK punctuation, fullwidth forms ~3.4 MB
  other     everything else (symbols, arrows, Latin)      ~0.3 MB

A Korean song loads only the Hangul file, a Latin-only session only the
small one. Rendering is pixel-identical to the single file (checked on a
mixed Korean/Japanese/Latin corpus in Chromium).

The OFL reserves the name "Pretendard JP" for the unmodified font, so the
subsets are renamed "Palette CJK" (name table and CSS family both). The
license travels with them as OFL.txt.

Run after bumping pretendard-jp (needs `pip install fonttools brotli`):

    python scripts/split_cjk_font.py
"""

from __future__ import annotations

import shutil
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parent.parent
PKG = ROOT / "node_modules" / "pretendard-jp" / "dist"
SOURCE = PKG / "web" / "variable" / "woff2" / "PretendardJPVariable.woff2"
OUT = ROOT / "src" / "assets" / "fonts"
FAMILY = "Palette CJK"

# Inclusive codepoint ranges. Anything in neither list goes to "other".
HANGUL = [
    (0x1100, 0x11FF),  # Hangul Jamo
    (0x3130, 0x318F),  # Hangul Compatibility Jamo
    (0xA960, 0xA97F),  # Hangul Jamo Extended-A
    (0xAC00, 0xD7A3),  # Hangul Syllables
    (0xD7B0, 0xD7FF),  # Hangul Jamo Extended-B
]
JAPANESE = [
    (0x3000, 0x303F),  # CJK Symbols and Punctuation (、。「」ー々)
    (0x3040, 0x309F),  # Hiragana
    (0x30A0, 0x30FF),  # Katakana
    (0x3190, 0x319F),  # Kanbun
    (0x31F0, 0x31FF),  # Katakana Phonetic Extensions
    (0x3200, 0x33FF),  # Enclosed CJK + CJK Compatibility
    (0x3400, 0x4DBF),  # CJK Extension A
    (0x4E00, 0x9FFF),  # CJK Unified Ideographs
    (0xF900, 0xFAFF),  # CJK Compatibility Ideographs
    (0xFF00, 0xFFEF),  # Halfwidth and Fullwidth Forms
    (0x20000, 0x2FFFF),  # CJK Extensions B+
]


def in_ranges(cp: int, ranges: list[tuple[int, int]]) -> bool:
    return any(lo <= cp <= hi for lo, hi in ranges)


# Runs closer than this merge in unicode-range. A codepoint in a merged gap
# that the font lacks just costs a lookup in a file the window already loads
# for its neighbors, and the CSS stays a few KB instead of ~45.
MERGE_GAP = 128


def to_ranges(cps: list[int]) -> list[tuple[int, int]]:
    """Collapse sorted codepoints into inclusive runs, bridging small gaps."""
    runs: list[tuple[int, int]] = []
    for cp in cps:
        if runs and cp - runs[-1][1] <= MERGE_GAP:
            runs[-1] = (runs[-1][0], cp)
        else:
            runs.append((cp, cp))
    return runs


def css_range(runs: list[tuple[int, int]]) -> str:
    return ", ".join(f"U+{lo:X}" if lo == hi else f"U+{lo:X}-{hi:X}" for lo, hi in runs)


def rename(font: TTFont) -> None:
    """Swap the reserved family name for FAMILY in every name record."""
    for rec in font["name"].names:
        text = rec.toUnicode()
        renamed = text.replace("Pretendard JP", FAMILY).replace("PretendardJP", FAMILY.replace(" ", ""))
        if renamed != text:
            rec.string = renamed


def write_subset(name: str, cps: list[int]) -> str:
    font = TTFont(SOURCE)
    opts = subset.Options()
    opts.layout_features = ["*"]
    opts.name_IDs = ["*"]
    opts.name_languages = ["*"]
    opts.notdef_outline = True
    opts.flavor = "woff2"
    sub = subset.Subsetter(opts)
    sub.populate(unicodes=cps)
    sub.subset(font)
    rename(font)
    file = f"palette-cjk-{name}.woff2"
    font.flavor = "woff2"
    font.save(OUT / file)
    return file


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    cmap = sorted(TTFont(SOURCE).getBestCmap())
    groups = {
        "hangul": [c for c in cmap if in_ranges(c, HANGUL)],
        "japanese": [c for c in cmap if in_ranges(c, JAPANESE)],
        "other": [c for c in cmap if not in_ranges(c, HANGUL) and not in_ranges(c, JAPANESE)],
    }
    faces = []
    for name, cps in groups.items():
        file = write_subset(name, cps)
        size = (OUT / file).stat().st_size
        print(f"{file}: {len(cps)} codepoints, {size / 1e6:.2f} MB")
        faces.append(
            "@font-face {\n"
            f"  font-family: \"{FAMILY}\";\n"
            "  font-style: normal;\n"
            "  font-weight: 45 920;\n"
            "  font-display: swap;\n"
            f"  src: url(\"./{file}\") format(\"woff2-variations\");\n"
            f"  unicode-range: {css_range(to_ranges(cps))};\n"
            "}\n"
        )
    shutil.copyfile(PKG / "LICENSE.txt", OUT / "OFL.txt")
    header = (
        "/* Generated by scripts/split_cjk_font.py from pretendard-jp. Do not edit.\n"
        "   Pretendard JP, Copyright (c) 2021 Kil Hyung-jin, SIL Open Font License 1.1\n"
        "   (OFL.txt). Subset per script and renamed, as the OFL requires for a\n"
        "   modified version of a font with a Reserved Font Name. */\n\n"
    )
    (OUT / "palette-cjk.css").write_text(header + "\n".join(faces), encoding="utf-8")


if __name__ == "__main__":
    main()
