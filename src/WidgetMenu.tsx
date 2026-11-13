/*
 * The widget's right-click context menu — the widget-native settings entry
 * (the tray is the always-there anchor; this is the fast in-place path). App
 * suppresses the WebView2 native menu and captures the cursor point; the menu
 * is cursor-anchored but App grows it toward the window interior (away from the
 * docked screen edge) so it can't clip. Dark tray-style surface, monochrome.
 *
 * While it's open App unions the hit rect to the whole window and paints the
 * dismiss scrim, so a click-away lands (the click-through gutter would swallow
 * it otherwise) and never starts a window drag.
 */
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { commands } from "./lib/backend";
import { Keycaps } from "./Keycaps";
import type { LyricLine } from "./lib/lrc";
import { plainLine, startRetime, undoFix, type SongRef } from "./lib/lineFix";

/** Box dims (mirror the w-[204px] class + ~5 items) so App can clamp the
 * cursor-anchored position to keep the whole menu inside the fixed window. */
export const WIDGET_MENU_W = 204;
export const WIDGET_MENU_H = 200;
/** Extra height when the menu opens on a lyric line (LINE_ITEMS + Sep). */
export const LINE_ITEMS_H = 72;

/** A right-click on a lyric line of a song with word timing: the line can
 * be fixed by hand (lib/lineFix.ts). */
export type LineTarget = { song: SongRef; line: LyricLine; seekable: boolean };

/** The line's own items, in menu order. Plain words: what happens. */
export function lineItems(target: LineTarget): { label: string; run: () => void }[] {
  const { song, line, seekable } = target;
  const items = [{ label: "Tap in this line", run: () => startRetime(song, line, seekable) }];
  if (line.fixed !== "plain") items.push({ label: "Show this line plain", run: () => void plainLine(song, line) });
  if (line.fixed) items.push({ label: "Undo fix", run: () => void undoFix(song, line) });
  return items;
}

function Item({
  label,
  onClick,
  muted,
  chord,
  tabIndex,
}: {
  label: string;
  onClick: () => void;
  muted?: boolean;
  chord?: string;
  // Roving tabindex: the active item is 0 (the menu's one Tab stop), the rest
  // -1 (arrow-reachable, not Tab-reachable). See WidgetMenu.
  tabIndex: number;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      tabIndex={tabIndex}
      onClick={onClick}
      // active deepen, not scale: press feedback for a full-width text row
      // (a scale would wobble the label); rides the existing bg transition.
      className={`flex w-full items-center gap-3 rounded-md px-2.5 py-1.5 text-left text-[12.5px] [transition:background-color_var(--transition-duration-2)_var(--ease-out-tk)] hover:bg-fg/[0.08] active:bg-fg/[0.12] ${
        muted ? "text-muted" : "text-fg"
      }`}
    >
      <span className="flex-1">{label}</span>
      {chord && <Keycaps chord={chord} size="sm" />}
    </button>
  );
}

function Sep() {
  return <div className="mx-1.5 my-1 h-px bg-border/[0.08]" />;
}

export function WidgetMenu({
  style,
  spotifyConnected,
  showhideChord,
  onClose,
  line,
}: {
  style: CSSProperties;
  spotifyConnected: boolean;
  showhideChord: string;
  onClose: () => void;
  line?: LineTarget | null;
}) {
  const fixes = line ? lineItems(line) : [];
  // Roving index of the first settings item, after the line's own.
  const o = fixes.length;
  const act = (fn: () => void) => () => {
    fn();
    onClose();
  };
  const menuRef = useRef<HTMLDivElement>(null);
  // role=menu contract: the full ARIA menu pattern — a SINGLE Tab stop
  // (roving tabindex: only `active` is tabbable, the rest tabIndex=-1) with
  // the arrows / Home / End moving WITHIN it and Esc handled at the window
  // level (App). Without the roving tabindex every native <button> was its
  // own Tab stop, so Tab past the last item escaped the open menu behind the
  // dismiss scrim (audit A7-3). `active` seeds at 0, so the effect focuses
  // the first item on open; focus then follows the roving index.
  const [active, setActive] = useState(0);
  const itemCount = () =>
    menuRef.current?.querySelectorAll('[role="menuitem"]').length ?? 0;
  useEffect(() => {
    const items = menuRef.current
      ? Array.from(menuRef.current.querySelectorAll<HTMLElement>('[role="menuitem"]'))
      : [];
    items[active]?.focus();
  }, [active]);
  const onKeyDown = (e: React.KeyboardEvent) => {
    const n = itemCount();
    if (n === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => (i + 1) % n); // wrap
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => (i <= 0 ? n - 1 : i - 1)); // wrap
    } else if (e.key === "Home") {
      e.preventDefault();
      setActive(0);
    } else if (e.key === "End") {
      e.preventDefault();
      setActive(n - 1);
    }
  };
  return (
    <div
      role="menu"
      ref={menuRef}
      onKeyDown={onKeyDown}
      onMouseDown={(e) => e.stopPropagation()}
      // The popover recipe verbatim (rounded-xl border-border/10 shadow-xl
      // shadow-black/40, App.tsx) — this menu had drifted into a third
      // floating-surface recipe (10px/0.11/black-50). .pop-in on mount = the
      // family's 140ms reveal, grown from the cursor (App sets
      // transform-origin there); close stays instant — dismissing menus
      // don't linger.
      className="pop-in absolute z-40 w-[204px] rounded-xl border border-border/10 bg-surface-2 p-1.5 shadow-xl shadow-black/40"
      style={style}
    >
      {/* tabIndex indices = DOM order of [role="menuitem"] (Sep isn't one), so
          they must stay in sync with the roving `active` index above. */}
      {fixes.map((item, i) => (
        <Item key={item.label} label={item.label} tabIndex={active === i ? 0 : -1} onClick={act(item.run)} />
      ))}
      {o > 0 && <Sep />}
      <Item label="Preferences…" tabIndex={active === o ? 0 : -1} onClick={act(() => commands.openPrefs())} />
      {/* Ellipsis: opens the same prefs window "Preferences…" does. */}
      <Item label="Shortcuts…" tabIndex={active === o + 1 ? 0 : -1} onClick={act(() => commands.openPrefs("hotkeys"))} />
      <Item
        label={spotifyConnected ? "Disconnect Spotify" : "Connect Spotify"}
        tabIndex={active === o + 2 ? 0 : -1}
        onClick={act(() => (spotifyConnected ? commands.spotifyDisconnect() : commands.spotifyConnect()))}
      />
      <Sep />
      <Item label="Hide Palette" chord={showhideChord} tabIndex={active === o + 3 ? 0 : -1} onClick={act(() => commands.hideWidget())} />
      <Sep />
      <Item label="Quit Palette" muted tabIndex={active === o + 4 ? 0 : -1} onClick={act(() => commands.quitApp())} />
    </div>
  );
}

/** The focus room's right-click on a lyric line: only the line's own items
 * (the room has no settings menu). Same surface as the widget menu, at the
 * cursor, over a full-window scrim that dismisses it. Esc closes the menu,
 * not the room. */
export function LineMenu({
  at,
  target,
  onClose,
}: {
  at: { x: number; y: number };
  target: LineTarget;
  onClose: () => void;
}) {
  const items = lineItems(target);
  const [active, setActive] = useState(0);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]')[active]?.focus();
  }, [active]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const n = items.length;
        setActive((i) => (e.key === "ArrowDown" ? (i + 1) % n : (i - 1 + n) % n));
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [items.length, onClose]);
  const w = WIDGET_MENU_W;
  const h = items.length * 32 + 12;
  const left = Math.max(4, Math.min(at.x, window.innerWidth - w - 4));
  const top = Math.max(4, Math.min(at.y, window.innerHeight - h - 4));
  return (
    <>
      <div
        aria-hidden
        className="fixed inset-0 z-30"
        onMouseDown={(e) => {
          e.stopPropagation();
          onClose();
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        role="menu"
        ref={menuRef}
        onMouseDown={(e) => e.stopPropagation()}
        className="pop-in fixed z-40 w-[204px] rounded-xl border border-border/10 bg-surface-2 p-1.5 shadow-xl shadow-black/40"
        style={{ left, top, transformOrigin: `${at.x - left}px ${at.y - top}px` }}
      >
        {items.map((item, i) => (
          <Item
            key={item.label}
            label={item.label}
            tabIndex={active === i ? 0 : -1}
            onClick={() => {
              item.run();
              onClose();
            }}
          />
        ))}
      </div>
    </>
  );
}
