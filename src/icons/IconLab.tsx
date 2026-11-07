/*
 * Dev-only morph sequencer (the article's "build a sequencer during
 * development" advice): `npm run dev` → http://localhost:1420/?lab
 * Runs in a plain browser — imports nothing from the Tauri backend, so every
 * transition pair can be exercised without a running player.
 */
import { useEffect, useState } from "react";
import { motion, useReducedMotion } from "motion/react";
import { DUR, EASE } from "../lib/tokens";
import { ICONS, type MorphName } from "./geometry";
import { MorphIcon } from "./MorphIcon";
import { PlayerMark, type PlayerId } from "./badges";
import { useSeekTick } from "./useSeekTick";
import { useSkipFlick } from "./useSkipFlick";
import { useBracketPulse } from "./useBracketPulse";
import {
  BackIcon,
  EnterKeyIcon,
  HeartIcon,
  HistoryIcon,
  LibraryIcon,
  PlayIcon,
  SearchIcon,
  ShiftKeyIcon,
  SongsIcon,
  TabKeyIcon,
  UpKeyIcon,
} from "./searchIcons";

const NAMES = Object.keys(ICONS) as MorphName[];
const SIZES = [13, 16, 18, 22, 48] as const;
const PLAYERS: PlayerId[] = ["spotify", "apple_music", "other"];
const INOUT = [...EASE.inOut] as [number, number, number, number];

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-sm font-medium text-muted">{title}</h2>
      {children}
    </section>
  );
}

/** Any→any stage: click a glyph to morph the stage to it; cycle steps
 * through the whole set so every unplanned pair gets seen at least once. */
function Sequencer() {
  const [glyph, setGlyph] = useState<MorphName>("play");
  const [cycling, setCycling] = useState(false);
  useEffect(() => {
    if (!cycling) return;
    const t = window.setInterval(
      () => setGlyph((g) => NAMES[(NAMES.indexOf(g) + 1) % NAMES.length]),
      900,
    );
    return () => window.clearInterval(t);
  }, [cycling]);
  return (
    <div className="flex items-center gap-8">
      <div className="grid h-40 w-40 shrink-0 place-items-center rounded-xl border border-border/10 bg-surface-2 text-fg">
        <MorphIcon name={glyph} size={112} />
      </div>
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap gap-1">
          {NAMES.map((n) => (
            <button
              key={n}
              type="button"
              onClick={() => setGlyph(n)}
              className={`flex items-center gap-1.5 rounded-md px-2 py-1 text-xs transition-colors duration-2 ease-out-tk hover:bg-fg/10 ${
                n === glyph ? "bg-fg/10 text-fg" : "text-muted"
              }`}
            >
              <MorphIcon name={n} size={14} />
              {n}
            </button>
          ))}
        </div>
        <label className="flex w-fit cursor-pointer items-center gap-2 text-xs text-muted">
          <input type="checkbox" checked={cycling} onChange={(e) => setCycling(e.target.checked)} />
          auto-cycle (900ms)
        </label>
      </div>
    </div>
  );
}

/** The marquee toggle at product size and magnified. */
function PlayPauseDemo() {
  const [playing, setPlaying] = useState(false);
  const name = playing ? "pause" : "play";
  return (
    <div className="flex items-center gap-6">
      {[18, 64].map((s) => (
        <button
          key={s}
          type="button"
          aria-label={playing ? "Pause" : "Play"}
          onClick={() => setPlaying((p) => !p)}
          className="grid place-items-center rounded-md p-2 text-fg transition-colors duration-2 ease-out-tk hover:bg-fg/10 active:scale-95"
        >
          <MorphIcon name={name} size={s} dur={DUR[2]} ease={EASE.out} />
        </button>
      ))}
      <span className="text-xs text-muted">140ms EASE.out, fired on press</span>
    </div>
  );
}

function SeekDemo({ dir }: { dir: -1 | 1 }) {
  const { scope, tick } = useSeekTick(dir);
  return (
    <button
      type="button"
      aria-label={dir < 0 ? "Seek tick back" : "Seek tick forward"}
      onPointerDown={() => void tick()}
      className="grid h-8 w-8 place-items-center rounded-md text-fg transition-colors duration-2 ease-out-tk hover:bg-fg/10"
    >
      <span ref={scope} className="grid place-items-center will-change-transform">
        <MorphIcon name={dir < 0 ? "seekBack" : "seekFwd"} size={17} />
      </span>
    </button>
  );
}

/** App's SkipButton strip, verbatim: masked 16px box, live glyph + ghosts at
 * ±width (the hook's wrap-on-mash frame-identity needs all three). */
function SkipDemo({ dir }: { dir: -1 | 1 }) {
  const { scope, tick } = useSkipFlick(dir, 16);
  const glyph = dir < 0 ? "prev" : "next";
  return (
    <button
      type="button"
      aria-label={dir < 0 ? "Skip flick back" : "Skip flick forward"}
      onPointerDown={() => void tick()}
      className="grid h-8 w-8 place-items-center rounded-md text-fg transition-colors duration-2 ease-out-tk hover:bg-fg/10"
    >
      <span className="grid h-4 w-4 place-items-center overflow-hidden">
        <span ref={scope} className="relative grid place-items-center will-change-transform">
          <MorphIcon name={glyph} size={16} />
          <span aria-hidden className="absolute right-full top-0 grid">
            <MorphIcon name={glyph} size={16} />
          </span>
          <span aria-hidden className="absolute left-full top-0 grid">
            <MorphIcon name={glyph} size={16} />
          </span>
        </span>
      </span>
    </button>
  );
}

function BracketPulseDemo({ verb }: { verb: "expand" | "contract" }) {
  const { scope, pulse } = useBracketPulse(verb);
  return (
    <button
      type="button"
      aria-label={verb === "expand" ? "Expand pulse" : "Contract pulse"}
      onPointerDown={pulse}
      className="grid h-7 w-7 place-items-center rounded-md text-fg transition-colors duration-2 ease-out-tk hover:bg-fg/10 active:scale-95"
    >
      <span ref={scope} className="grid place-items-center will-change-transform">
        <MorphIcon name={verb} size={13} />
      </span>
    </button>
  );
}

/** Mock of App's mode ladder: same key={mode} remount, same layoutId slots,
 * same destination-named buttons — verifies the glide + cross-remount glyph
 * morph without a Tauri window. */
function ModeDemo() {
  const [mode, setMode] = useState<"pill" | "card" | "expanded">("card");
  const reduced = useReducedMotion() ?? false;
  const layoutT = {
    layout: { duration: reduced ? 0 : DUR[3] / 1000, ease: INOUT },
    // Keep whileTap's scale on the tokens, not motion's default spring.
    scale: { duration: reduced ? 0 : DUR[1] / 1000, ease: [...EASE.out] as [number, number, number, number] },
  };
  const btn = (to: MorphName, slot: string, onClick: () => void, label: string) => (
    <motion.button
      key={slot}
      type="button"
      layoutId={slot}
      transition={layoutT}
      whileTap={{ scale: reduced ? 1 : 0.95 }}
      onClick={onClick}
      aria-label={label}
      title={label}
      className="grid h-7 w-7 place-items-center rounded-md text-muted transition-colors duration-2 ease-out-tk hover:bg-fg/10 hover:text-fg"
    >
      <MorphIcon name={to} size={13} slot={slot} dur={DUR[3]} ease={EASE.inOut} />
    </motion.button>
  );
  const size = { pill: [300, 48], card: [380, 138], expanded: [380, 240] }[mode];
  return (
    <motion.div
      key={mode}
      animate={{ width: size[0], height: size[1] }}
      initial={false}
      transition={{ duration: reduced ? 0 : DUR[3] / 1000, ease: INOUT }}
      className="flex flex-col rounded-xl border border-border/10 bg-surface-2 p-2"
    >
      <div className="flex items-center justify-end gap-1">
        <span className="mr-auto pl-1 text-xs text-muted">{mode}</span>
        {mode === "pill" && btn("expand", "mode-secondary", () => setMode("card"), "Expand to card")}
        {mode === "card" && btn("contract", "mode-secondary", () => setMode("pill"), "Collapse to pill")}
        {mode === "card" && btn("mic", "mode-primary", () => setMode("expanded"), "Show lyrics")}
        {mode === "expanded" && btn("contract", "mode-primary", () => setMode("card"), "Back to card")}
      </div>
    </motion.div>
  );
}

/** The search window's icons before the pixel-grid redraw (2026-11-06):
 * 16-unit drawings shown at 10-14px. Kept only so the lab can show old and
 * new side by side at 1x; delete once the new set has settled. */
const SEARCH_LEGACY: Record<string, string> = {
  search: `<svg viewBox='0 0 16 16' width='16' height='16' fill='none' stroke='currentColor' stroke-width='1.5' stroke-linecap='round'><circle cx='7' cy='7' r='4.4'/><path d='M 10.4,10.4 L 13.6,13.6'/></svg>`,
  songs: `<svg viewBox='0 0 16 16' width='14' height='14' fill='none' stroke='currentColor' stroke-width='1.5' stroke-linecap='round'><circle cx='7' cy='7' r='4.2'/><path d='M 10.3,10.3 L 13.2,13.2'/></svg>`,
  library: `<svg viewBox='0 0 16 16' width='14' height='14' fill='none' stroke='currentColor' stroke-width='1.5' stroke-linecap='round'><path d='M 3.2,3.2 L 3.2,12.8'/><path d='M 6.4,3.2 L 6.4,12.8'/><path d='M 9.2,3.8 L 12.8,12.2'/></svg>`,
  history: `<svg viewBox='0 0 16 16' width='14' height='14' fill='none' stroke='currentColor' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'><circle cx='8' cy='8' r='5.6'/><path d='M 8,5 L 8,8 L 10.2,9.4'/></svg>`,
  enter: `<svg viewBox='0 0 16 16' width='12' height='12' fill='none' stroke='currentColor' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'><path d='M 12.5,4 L 12.5,9 L 4.5,9'/><path d='M 7.5,6 L 4.5,9 L 7.5,12'/></svg>`,
  shift: `<svg viewBox='0 0 16 16' width='12' height='12' fill='none' stroke='currentColor' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'><path d='M 8,3 L 12.8,8.4 L 9.9,8.4 L 9.9,12.5 L 6.1,12.5 L 6.1,8.4 L 3.2,8.4 Z'/></svg>`,
  up: `<svg viewBox='0 0 16 16' width='12' height='12' fill='none' stroke='currentColor' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'><path d='M 8,12.5 L 8,3.5'/><path d='M 4.5,7 L 8,3.5 L 11.5,7'/></svg>`,
  tab: `<svg viewBox='0 0 16 16' width='12' height='12' fill='none' stroke='currentColor' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'><path d='M 2.5,8 L 11.5,8'/><path d='M 8.5,5 L 11.5,8 L 8.5,11'/><path d='M 13.5,4.5 L 13.5,11.5'/></svg>`,
  play: `<svg viewBox='0 0 16 16' width='11' height='11' fill='currentColor'><path d='M 5,3.2 L 12.6,8 L 5,12.8 Z'/></svg>`,
  back: `<svg viewBox='0 0 16 16' width='10' height='10' fill='none' stroke='currentColor' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'><path d='M10,3.5 L5.5,8 L10,12.5'/></svg>`,
  heart: `<svg viewBox='0 0 16 16' width='13' height='13' fill='currentColor' stroke='currentColor' stroke-width='1.5' stroke-linejoin='round'><path d='M8,13 C5.3,11.1 2.7,9 2.7,6.3 C2.7,4.5 4,3.2 5.6,3.2 C6.7,3.2 7.5,3.8 8,4.7 C8.5,3.8 9.3,3.2 10.4,3.2 C12,3.2 13.3,4.5 13.3,6.3 C13.3,9 10.7,11.1 8,13 Z'/></svg>`,
};
const SEARCH_NEW: Record<string, React.ReactNode> = {
  search: <SearchIcon />,
  songs: <SongsIcon />,
  library: <LibraryIcon />,
  history: <HistoryIcon />,
  enter: <EnterKeyIcon />,
  shift: <ShiftKeyIcon />,
  up: <UpKeyIcon />,
  tab: <TabKeyIcon />,
  play: <PlayIcon />,
  back: <BackIcon />,
  heart: <HeartIcon />,
};

/** Old vs new at their real size, in both text tones, on the window's own
 * surface. Judge on the real PC: headless screenshots can't show it. */
function SearchIconCompare() {
  return (
    <div className="flex flex-col gap-1 rounded-xl border border-border/10 bg-surface p-4">
      <div className="flex items-center gap-6 pb-1 text-[11px] text-muted">
        <span className="w-16" />
        <span className="w-24">old</span>
        <span className="w-24">new</span>
      </div>
      {Object.keys(SEARCH_NEW).map((k) => (
        <div key={k} className="flex h-7 items-center gap-6">
          <span className="w-16 text-xs text-muted">{k}</span>
          {[SEARCH_LEGACY[k], null].map((old, i) => (
            <span key={i} className="flex w-24 items-center gap-4">
              {(["text-fg", "text-muted"] as const).map((tone) => (
                <span key={tone} className={`flex ${tone}`}>
                  {old !== null ? <span className="flex" dangerouslySetInnerHTML={{ __html: old }} /> : SEARCH_NEW[k]}
                </span>
              ))}
            </span>
          ))}
        </div>
      ))}
    </div>
  );
}

export function IconLab() {
  return (
    <main className="min-h-screen bg-surface p-8 text-fg">
      <div className="flex max-w-3xl flex-col gap-10">
        <header>
          <h1 className="text-lg font-medium">Palette icon lab</h1>
          <p className="text-xs text-muted">
            3 strokes × 2 cubics per glyph — any icon morphs into any other.
          </p>
        </header>
        <Section title="Search icons — old vs pixel-grid redraw, at 1x">
          <SearchIconCompare />
        </Section>
        <Section title="Sequencer — any → any (200ms EASE.inOut)">
          <Sequencer />
        </Section>
        <Section title="Play ↔ pause">
          <PlayPauseDemo />
        </Section>
        <Section title="Seek spin — one revolution per press, accumulates on mash">
          <div className="flex items-center gap-1">
            <SeekDemo dir={-1} />
            <SeekDemo dir={1} />
          </div>
        </Section>
        <Section title="Skip flick — pass-through travel, mash re-launches mid-stream">
          <div className="flex items-center gap-1">
            <SkipDemo dir={-1} />
            <SkipDemo dir={1} />
          </div>
        </Section>
        <Section title="Bracket pulse — expand bursts apart, contract pulls together">
          <div className="flex items-center gap-1">
            <BracketPulseDemo verb="contract" />
            <BracketPulseDemo verb="expand" />
          </div>
        </Section>
        <Section title="Mode ladder — destination-named buttons, layoutId glide + slot morph">
          <ModeDemo />
        </Section>
        <Section title="At size">
          <div className="flex flex-col gap-2">
            {NAMES.map((n) => (
              <div key={n} className="flex items-center gap-4">
                <span className="w-20 text-xs text-muted">{n}</span>
                {SIZES.map((s) => (
                  <span key={s} className="grid w-14 place-items-center text-fg">
                    <MorphIcon name={n} size={s} />
                  </span>
                ))}
              </div>
            ))}
          </div>
        </Section>
        <Section title="Badges (static, muted)">
          <div className="flex items-center gap-6 text-muted">
            {PLAYERS.map((p) => (
              <span key={p} className="flex items-center gap-3">
                <PlayerMark player={p} size={14} />
                <PlayerMark player={p} size={28} />
                <span className="text-xs">{p}</span>
              </span>
            ))}
          </div>
        </Section>
      </div>
    </main>
  );
}
