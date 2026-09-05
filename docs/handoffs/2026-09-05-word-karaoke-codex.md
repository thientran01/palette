# Handoff: word-karaoke accuracy and smoothness (for Codex)

**Date:** 2026-09-05 · **Repo:** `thientran01/palette` (local checkout of this branch: `C:\Users\Thien\.cursor\worktrees\pulse\fu2s`; the main checkout `C:\Users\Thien\Projects\pulse` sits on another branch) · **Branch / PR:** `feature/word-karaoke`, [PR #162](https://github.com/thientran01/palette/pull/162) · **Owner:** Thien

Read the repo root `CLAUDE.md` first. It carries house rules that this
work must respect (listed again in §7). Then the three specs in
`docs/specs/2026-09-04-*.md`, which record what was decided and measured.

## 1. The product problem in one paragraph

Palette is a Windows mini music player (Tauri v2: Rust backend, React
frontend). Its expanded view shows synced lyrics from LRCLIB, which only
has LINE timestamps. "Word karaoke" makes each word of the current line
wipe from muted to foreground as it is sung. LRCLIB has no word timing,
so Palette records the song's audio locally (WASAPI loopback of the
player process), aligns the known words onto that audio, and stores word
times per track. Thien's bar (2026-07-06, vault): **perfect or nothing** —
an "only okay" karaoke sweep was removed once already. Today the wipe is
"honestly very synced" on his words, but the measured error is still a
~170ms median against his own tap-labelled truth, and he wants it as
accurate and smooth as it can be.

## 2. Architecture (what exists on this branch)

Backend, `src-tauri/src/`:

- `karaoke.rs` — the recorder + store. `observe(np)` runs on the media
  loop every beat: arms a recording when a local Spotify/Apple Music
  track starts (position < 8s, no word file yet, not in the session miss
  set); every fresh `(position_ms, position_at_ms)` pair becomes an
  **anchor** `(sample_index, position_ms)`; a pair >2.5s off the running
  fit is a seek "strike", two consecutive strikes drop the recording;
  pairs inside the last 1.5s of the track are ignored (GSMTC clamps the
  position at the duration while audio keeps flowing). `push_frames`
  runs on the realtime audio thread (both `audio.rs` cpal fallback and
  `loopback.rs` process capture call it, one lock per packet): box-
  decimates to 16kHz mono, pads wall-clock delivery gaps >400ms with
  silence. On track change / capture stop, `try_commit` → `commit_sync`
  (spawned thread): fits a `TimeMap`, optionally writes an evidence dump,
  runs the aligner, stores `app-data/karaoke/<key>.json` (v4, carries the
  aligner `recipe`; mismatched files are deleted on read and re-record),
  emits `karaoke-ready`. Also owns the **word lead** (settings
  `wordLeadMs`, default 160, nudged by the Ctrl+Alt+[ / ] hotkeys, event
  `word-lead`).
- `align.rs` — `TimeMap` (least-squares sample-index → position, slope
  clamped to nominal ±0.5%, 1.2s outliers dropped once) and the aligner.
  `align_with_params(pcm, 16000, lines, &map, &Stages, &PriorParams)`.
  The shipped aligner (`Stages::shipped()` = `song_lead`) is:
  1. **Song-lead calibration** — run the `EnergyRise` start detector on
     every line (first HP-RMS envelope rise above 12% of the window
     peak, searched in [stamp−300, stamp+900]ms); take the MEDIAN of
     (detected − stamp) over lines where it fired (≥6 needed, clamped
     −100..800ms). Per-line detections are noisy; their median is
     unbiased on both truth songs.
  2. **Prior** per line — `start = stamp + song_lead`;
     `span = min(S / 3.1 syl/s, 0.9 × window − lead)` where `S` = syllable
     weight of all tokens except the last (`token_weight`: 1 per
     hangul/CJK glyph, `1 + 0.12·(n−1)` per Latin word) and `window` =
     next stamp − this stamp (last line: 8s); tokens spread linearly by
     cumulative weight; last word holds 250ms.
  Every other rung (per-line `Flux` / `EnergyRise` start refinement,
  end refinement, song-rate calibration) exists behind `Stages` flags
  and LOST on both songs. `Stages::RECIPE` names the shipped recipe —
  bump it on ANY change that moves word times.
- `examples/karaoke_score.rs` — the offline scorer (see §4).

Frontend, `src/`:

- `lib/lrc.ts` — `LyricWord {t, text, end?, line_t?}`; `attachWords`
  pins words to lines by `line_t` (the LRC stamp) when present; `wordWipe`
  = which word is live and its fill fraction: an ease-out over
  `WORD_ATTACK_MS` (90ms) after `t`, then holds at 100% (crawling through
  a held note read as a progress bar, not a beat — Thien's call).
- `LyricsPanel.tsx` — `useWordWipe`: every word span in the current row
  wears the same fg→muted gradient; ONE rAF loop moves each span's
  `--wipe` stop (5px feather). No React state per word (the previous
  state-driven version flickered at every boundary). The lead applied is
  `VOCAL_LEAD_MS[player] + wordLeadMs`.
- `lib/wordLead.ts` — mirror of the persisted word lead + `useWordLead`.
- `lib/posClock.ts` — THE display clock (pre-existing). Rust never
  projects a UI-visible position; all word/line timing is judged against
  `posClock.now()`.

Line timing (pre-existing, untouched): `VOCAL_LEAD_MS` per player
(Spotify 50ms), `useLyricIndex` scheduling. Thien called the line
highlight "perfect" in July — do not regress it.

## 3. What was measured (the numbers to beat)

Truth = Thien tapping each word/syllable on a slowed-down playback with
reaction-time calibration (`scripts/karaoke_tap.py`); tap noise is about
±100ms, so nothing under ~80ms median is measurable with this truth.

| song | tokens | original aligner | fixed prior (330ms lead) | **shipped: song lead** |
|---|---:|---:|---:|---:|
| TAEYEON — Blur (K-pop, mostly hangul) | 343 | 498ms median, p90 1874, 10% ≤100ms | 183 | **177 / p90 528 / 28% ≤100 / bias +27** |
| Mac DeMarco — Heart To Heart (English) | 118 | — | 374 (bias +359: its stamps lead the vocal by 114ms, Blur's by 330) | **160 / p90 432 / 31% ≤100 / bias −71** |

Decomposition on Blur (oracles from the truth set):

- Given each line's TRUE first/last sung time, a flat syllable-weighted
  spread scores **44ms** median; the audio envelope placing words inside
  that same span scored 127ms. Inside a line, audio hurts.
- So the remaining error is almost entirely **where each line starts and
  ends**. Start error: song-lead median gets the mean right but per-line
  variance stays (truth lead p10 −71 / p90 +536ms on Blur). End/span
  error: the syllable rate varies 1.7–4.4/s within a song; the window cap
  (0.9×) is the main defence.
- Heart To Heart with true lead AND true rate scores **76ms** — i.e. a
  per-song (or per-line) rate estimate is the biggest remaining lever, if
  it can be measured. The end-detector rate estimate LOST (bias −338:
  the HP-RMS envelope on a full mix never drops to the floor at a phrase
  end, so spans measure short).
- Clock is not the problem: TimeMap residual 13–20ms on real songs.
- `karaoke_score fit` on both songs: best prior constants (rate 3.2, fill
  0.95) buy ~6ms; best fallback lead 60ms (only used when <6 lines vote).

Perception: even with unbiased data Thien read words as late; the wipe
now leads the aligned onset by 160ms (his setting). The 90ms attack ramp
is the other half of "smoothness".

## 4. How to iterate (the loop that must gate any change)

Never judge an aligner change by ear first. The loop:

```powershell
cd C:\Users\Thien\.cursor\worktrees\pulse\fu2s\src-tauri
$env:CARGO_TARGET_DIR = "C:\Users\Thien\Projects\pulse\src-tauri\target"   # warm cache; optional
cargo run --example karaoke_score -- matrix "C:\Users\Thien\OneDrive\Desktop\karaoke-blur" "C:\Users\Thien\OneDrive\Desktop\karaoke-blur\labels.txt"
cargo run --example karaoke_score -- matrix "$env:APPDATA\com.thien.pulse\karaoke-dumps\ff357c69da505299" "$env:APPDATA\com.thien.pulse\karaoke-dumps\ff357c69da505299\labels.txt"
cargo run --example karaoke_score -- fit    <both dump dirs>          # grid-search prior constants, report only
cargo run --example karaoke_score -- score  <dump-dir> <labels.txt>   # shipped stages, worst lines, replay-vs-live check
```

- A dump dir holds `pcm.i16` (16kHz mono LE), `lyrics.lrc`, `words.json`
  (what the app produced live), `meta.json` (TimeMap + anchors + fit
  stats), `labels.txt` (truth, Audacity label format `start\tend\ttext`,
  one row per aligner token IN ORDER — matching is by row order, never by
  text). Blur's dump also lives beside its labels on the Desktop.
- **Acceptance for shipping a change** (spec): median ≤ 200ms and no line
  with mean |Δ| > 1500ms on BOTH songs, and no song's median gets worse.
  Add a rung to `Stages::ladder()` rather than replacing the shipped
  path, so the matrix prints what it buys; ship the highest rung that
  wins on both songs; bump `Stages::RECIPE`.
- More truth: enable `"karaokeDump": true` in
  `%APPDATA%\com.thien.pulse\settings.json` (already on), play a song
  through without pausing/seeking past 55%, let the next track start;
  then `karaoke_score template <dump>` and `python scripts/karaoke_tap.py
  <dump> <out-dir>` build a self-contained tap page (`tap.html`); Thien
  taps ~4 minutes per song. Two more songs (one with fast rap-like
  delivery, one slow ballad) would make the constants trustworthy — the
  spec says a learned model is worth trying at ≥10 tapped songs.

Unit tests: `cargo test --lib` (84), `npx vitest run` (26), `npx tsc
--noEmit`. CI runs `cargo fmt --check` and `cargo clippy --all-targets
-- -D warnings` on a 2026-07 stable toolchain (it denies lints the local
toolchain may not — e.g. `chunks_exact` with a constant size).

## 5. What was tried and rejected (don't re-propose without new evidence)

- **Full-mix onset aligner** (SuperFlux spectral flux + monotonic DP over
  all onsets, 2026-09-04): shipped for hours, regressed live — latched
  onto drum onsets, and a pre-stamp search window put the next line's
  first words on the previous row. Reverted. Its flux detector survives
  only as a windowed start-refinement rung, where it still loses.
- **Per-line start/end refinement** from audio: loses to the flat prior
  on both songs (see matrix in the ladder spec). Aggregate (median)
  signals help; per-line ones don't, on a full mix.
- **Song-rate from the end detector**: biased short. The idea is right
  (per-song rate is the biggest lever); the measurement is wrong.
- **Even-pace sweep without audio** (2026-07-06, PR 8): removed on
  Thien's "only okay" verdict — a no-audio prior alone must not ship as
  the visible fallback (his call again 2026-09-04: prior stays internal).

## 6. Open leads, ranked by expected payoff

1. **Vocal separation before alignment.** Everything above is limited by
   drums/instruments in the envelope. A Spleeter-class 2-stem ONNX model
   (~40MB, seconds per song on CPU) run once at commit would give a
   vocal envelope where the end detector and per-line starts likely stop
   lying. Cost was estimated at ~5 PRs with `ort`; the scorer decides.
   Even cheaper first probe: try the current detectors on a
   vocal-band-filtered, dynamics-compressed signal and re-run the matrix.
2. **A real forced aligner on the separated vocal** (CTC, e.g. MMS —
   note CC-BY-NC — or Apache wav2vec2 per language; Korean needs
   romanization). 50–100ms medians are typical on clean vocals. Heavy
   (~300MB model, 1–2GB transient RAM); gate on idle.
3. **Per-song rate from a better signal**: line duration statistics from
   the LRC itself (window lengths vs syllable counts across the song
   give a rate estimate with no audio at all — check it against the
   truth rates: Blur 3.1/s, HtH 4.0/s).
4. **Fetch word-level lyrics** (QQ Music QRC / Musixmatch richsync) as a
   source, with local alignment as fallback. Unofficial endpoints;
   Thien has not decided on this.
5. **Smoothness**: the 90ms attack and 5px feather are feel knobs with
   no truth set; Thien's ear is the instrument. Consider per-word
   duration-aware attack for very short syllables (hangul lines run
   ~200ms/syllable) and whether the last word should hold to the next
   line's start rather than 250ms.
6. **Learned per-word offset model** once ≥10 songs are tapped
   (features: syllable count, window length, position in line, script).

## 7. House rules that bind this work (from CLAUDE.md and the specs)

- Never commit to `main`. Work on `feature/*`, PR with self-review, then
  `/quick-review`; squash-merge. PR #162 is open and reviewed — either
  build on it or branch from it after it merges.
- Rust never projects a UI-visible position; the frontend `posClock`
  owns display time. The TimeMap is alignment-internal (same carve-out
  as history's `ms_listened`).
- No work while the widget is hidden except the documented carve-outs;
  capture runs only while visible + playing. `push_frames` is on the
  realtime audio thread: no allocation, one short lock.
- Design: accent never colors text; lyric text stays `fg`; motion uses
  the EASE/DUR tokens; "the art never moves" (ambiently). The word wipe
  is fg→muted only.
- Any aligner change that moves word times bumps `Stages::RECIPE` (files
  re-record). Any hotkey/settings change updates `hotkey_defs`, the
  frontend `MOCK_HOTKEYS`, and the specs.
- The repo is LF. On this Windows machine, text-mode Python writes
  produce CRLF and turn a 30-line change into a whole-file diff — patch
  in binary or use an editor; check `git diff --stat` before committing.
- Live launch: `npm run tauri dev` from the worktree with
  `CARGO_TARGET_DIR` pointing at the main checkout's target dir; kill any
  listener on port 1420 first; quit the running dev app (`pulse.exe`)
  before relaunching. The tauri file watcher does not always rebuild —
  relaunch after Rust changes.

## 8. Where the truth and evidence live

- `C:\Users\Thien\OneDrive\Desktop\karaoke-blur\` — Blur dump + `labels.txt` + `tap.html`
- `C:\Users\Thien\OneDrive\Desktop\karaoke-heart-to-heart\` — HtH tap page; its dump + `labels.txt` at `%APPDATA%\com.thien.pulse\karaoke-dumps\ff357c69da505299\`
- `%APPDATA%\com.thien.pulse\karaoke-dumps\` — last 5 dumps (also DEAN — NASA, unlabelled)
- `%APPDATA%\com.thien.pulse\karaoke\` — stored word files (v4, recipe `song-lead/2`)
- Vault (Obsidian): `Projects/Pulse.md` § Key decisions, entries dated 2026-09-04, carry the decision history.
