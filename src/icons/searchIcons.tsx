/*
 * The search window's static icons, drawn on the pixel grid they render at.
 *
 * The old set was drawn in a 16-unit viewBox and shown at 10-14px, so every
 * 1.5 stroke shrank to 1.1-1.3px and landed between pixels. The windows are
 * transparent, so WebView2 can't use ClearType and those half-covered pixels
 * smear into gray (Thien: "kinda low quality", 2026-11-06). Here every
 * viewBox equals the rendered size (1 unit = 1 CSS px at 100% scaling), and
 * straight strokes are centered on n + 0.75: a 1.5 stroke then covers one
 * whole pixel plus a half, so at least one edge of every line is crisp.
 * Curves and diagonals can't snap and don't need to.
 *
 * Rules for a new icon: draw it at its display size, stroke 1.5, round caps
 * and joins, horizontal/vertical centerlines on .75 (or .25 when the crisp
 * edge should face the other way). Show it at that size or an integer
 * multiple of it (2x keeps the snapped edges on whole pixels). Compare it
 * at 1x in the icon lab (`npm run dev` → /?lab).
 */

const line = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

/** `scale` 2 doubles the icon for a large tile and keeps it crisp. */
const box = (grid: number, scale = 1) => ({
  viewBox: `0 0 ${grid} ${grid}`,
  width: grid * scale,
  height: grid * scale,
});

/** The search input's magnifier (16px). The only magnifier in Search. */
export function SearchIcon() {
  return (
    <svg {...line} {...box(16)}>
      <circle cx="7" cy="7" r="4.25" />
      <path d="M 10.25 10.25 L 13.25 13.25" />
    </svg>
  );
}

/** Songs tab: an eighth note (14px). */
export function SongsIcon() {
  return (
    <svg {...line} {...box(14)}>
      <path d="M 8.75 10.25 V 2.25 C 10.25 2.75 12 3.75 12 6" />
      <circle cx="6.25" cy="10.25" r="2.5" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** Library tab and the playlist tile: a stack of albums (14px). */
export function LibraryIcon({ scale = 1 }: { scale?: number }) {
  return (
    <svg {...line} {...box(14, scale)}>
      <rect x="1.75" y="5.75" width="7" height="7" rx="1.5" />
      <path d="M 4.75 2.75 H 10.25 Q 11.75 2.75 11.75 4.25 V 9.75" />
    </svg>
  );
}

/** History tab: a clock at three o'clock (14px). */
export function HistoryIcon() {
  return (
    <svg {...line} {...box(14)}>
      <circle cx="7" cy="7" r="5.25" />
      <path d="M 6.75 4 V 7.25 H 9" />
    </svg>
  );
}

/** Liked Songs tile (14px, filled). */
export function HeartIcon({ scale = 1 }: { scale?: number }) {
  return (
    <svg {...box(14, scale)} fill="currentColor" stroke="currentColor" strokeWidth={1} strokeLinejoin="round" aria-hidden>
      <path d="M 7 12 C 4.6 10.4 2 8.5 2 5.75 C 2 4 3.25 2.75 4.85 2.75 C 5.8 2.75 6.55 3.25 7 4 C 7.45 3.25 8.2 2.75 9.15 2.75 C 10.75 2.75 12 4 12 5.75 C 12 8.5 9.4 10.4 7 12 Z" />
    </svg>
  );
}

/** The playlist header's Play button (10px, filled, softly rounded). */
export function PlayIcon() {
  return (
    <svg {...box(10)} fill="currentColor" stroke="currentColor" strokeWidth={1} strokeLinejoin="round" aria-hidden>
      <path d="M 2.5 1.5 L 8.5 5 L 2.5 8.5 Z" />
    </svg>
  );
}

/** The "‹ Library" breadcrumb's back chevron (10px). */
export function BackIcon() {
  return (
    <svg {...line} {...box(10)}>
      <path d="M 6.25 1.75 L 3 5 L 6.25 8.25" />
    </svg>
  );
}

// ---- keycap glyphs (12px, inside the 18px hint keys) ----

export function EnterKeyIcon() {
  return (
    <svg {...line} {...box(12)}>
      <path d="M 9.75 2.25 V 6.75 H 2.75" />
      <path d="M 5 4.5 L 2.75 6.75 L 5 9" />
    </svg>
  );
}

export function ShiftKeyIcon() {
  return (
    <svg {...line} {...box(12)}>
      <path d="M 6 1.75 L 10.25 6.25 H 8.25 V 10.25 H 3.75 V 6.25 H 1.75 Z" />
    </svg>
  );
}

export function UpKeyIcon() {
  return (
    <svg {...line} {...box(12)}>
      <path d="M 5.75 10 V 2.25" />
      <path d="M 2.5 5.5 L 5.75 2.25 L 9 5.5" />
    </svg>
  );
}

/** Tab key: an arrow into a bar. */
export function TabKeyIcon() {
  return (
    <svg {...line} {...box(12)}>
      <path d="M 1.75 5.75 H 8" />
      <path d="M 5.75 3.5 L 8 5.75 L 5.75 8" />
      <path d="M 10.25 2.5 V 9" />
    </svg>
  );
}
