import { useCallback, useEffect, useRef, useState } from "react";
import { commands, vocalPreviewEnabled, karaokeStatus, onCursorLeft, type SyncStatus } from "./lib/backend";
import type { NowPlaying } from "./types";
import { resolveSyncStatus, savedSyncStatus } from "./lib/lyricSyncState";
import { SavedSyncs } from "./SavedSyncs";

const titles = {waiting:"Waiting to learn",learning:"Learning timing",processing:"Finishing sync",saved:"Word sync saved",failed:"Couldn’t sync this track"};

/** Shared corner menu. Sync status lives inside the library and does not
 * claim a lyric rail or disappear when the queue opens. */
export function PlayerMenu({np, saved}: {np:NowPlaying; saved:boolean}) {
  const [preview, setPreview] = useState(false);
  const [surface, setSurface] = useState<"menu" | "library" | null>(null);
  const [snapshot, setSnapshot] = useState<{key:string; status:SyncStatus} | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const library = surface === "library";
  const key = JSON.stringify([np.artist,np.title,np.album,np.duration_ms]);
  const close = useCallback((restoreFocus = true) => {
    setSurface(null);
    if (restoreFocus) trigger.current?.focus();
  }, []);

  useEffect(() => {
    if (!library) return;
    let active = true;
    void vocalPreviewEnabled().then(value => { if (active) setPreview(value); }).catch(() => {});
    return () => { active = false; };
  }, [library]);

  useEffect(() => {
    if (!library) return;
    let disposed = false;
    let poll: number | undefined;
    let inFlight = false;
    const refresh = async () => {
      window.clearTimeout(poll);
      if (disposed || document.hidden || (saved && !preview) || inFlight) return;
      inFlight = true;
      try {
        const status = await karaokeStatus(np);
        if (!disposed) setSnapshot({key,status});
      } catch {
        if (!disposed) setSnapshot({key,status:{phase:"waiting",detail:"Sync status is unavailable right now."}});
      } finally {
        inFlight = false;
        if (!disposed) poll = window.setTimeout(refresh,2000);
      }
    };
    void refresh();
    document.addEventListener("visibilitychange",refresh);
    return () => { disposed = true; window.clearTimeout(poll); document.removeEventListener("visibilitychange",refresh); };
  // Identity, not the frequently updated playback position, owns a request.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key,saved,preview,library]);

  useEffect(() => {
    if (surface !== "menu") return;
    menu.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const outside = (e: PointerEvent) => { if (!root.current?.contains(e.target as Node)) close(false); };
    const escape = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); }
    };
    document.addEventListener("pointerdown",outside);
    document.addEventListener("keydown",escape,true);
    return () => { document.removeEventListener("pointerdown",outside); document.removeEventListener("keydown",escape,true); };
  }, [surface,close]);
  useEffect(() => onCursorLeft(() => setSurface(current => current === "menu" ? null : current)), []);

  const status = resolveSyncStatus(saved,preview,snapshot?.key === key ? snapshot.status : null);
  const title = preview ? {waiting:"Vocal sync trial",learning:"Learning vocal timing",processing:"Finishing vocal sync",saved:"Vocal sync saved",failed:"Vocal sync needs another try"}[status.phase] : titles[status.phase];
  const detail = status.phase === "saved" ? preview ? "Experimental vocal timing is ready for this song." : savedSyncStatus.detail
    : status.phase === "processing" ? preview ? "Isolating vocals and matching them to the lyrics. Your saved timing stays available." : "Your recording is being matched to the lyrics."
    : status.phase === "learning" ? np.status !== "playing" ? "Resume playback to continue this listen." : status.detail.replace(/^Learning timing…\s*/, "")
    : status.detail.replace(/^Couldn’t save word sync\.\s*/, "").replace("Couldn’t align this track’s vocals. Word sync was not saved.", "This recording did not produce usable word timing. Keeping line sync.");
  const item = "w-full rounded-md px-3 py-2 text-left text-xs text-fg transition-colors duration-2 ease-out-tk hover:bg-fg/10 focus-visible:bg-fg/10 focus-visible:outline-none active:bg-fg/15";

  return <div ref={root} className="relative shrink-0" onMouseDown={e => e.stopPropagation()}>
    <button ref={trigger} type="button" aria-label="More options" title="More options" aria-haspopup={library ? "dialog" : "menu"} aria-expanded={surface !== null}
      className="grid h-7 w-7 place-items-center rounded-md text-fg transition-[background-color,scale] duration-2 ease-out-tk hover:bg-fg/10 aria-expanded:bg-fg/10 active:scale-95 focus-visible:outline focus-visible:outline-2 focus-visible:outline-fg/50"
      onClick={() => setSurface(current => current ? null : "menu")}
      onKeyDown={e => { if (e.key === "ArrowDown") { e.preventDefault(); setSurface("menu"); } }}>
      <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><circle cx="3" cy="8" r="1"/><circle cx="8" cy="8" r="1"/><circle cx="13" cy="8" r="1"/></svg>
    </button>
    {surface === "menu" && <div ref={menu} role="menu" aria-label="Player options"
      className="absolute right-0 top-full z-50 mt-1.5 w-44 rounded-xl border border-border/10 bg-surface-2 p-1.5 shadow-xl shadow-black/40 animate-[caption-in_140ms_var(--ease-out-tk)_both]"
      onKeyDown={e => {
        const items = Array.from(menu.current?.querySelectorAll<HTMLButtonElement>("button") ?? []);
        const index = items.indexOf(document.activeElement as HTMLButtonElement);
        if (["ArrowDown","ArrowUp","Home","End"].includes(e.key)) {
          e.preventDefault();
          items[e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : (index + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus();
        } else if (e.key === "Tab") close(false);
      }}>
      <button type="button" role="menuitem" className={item} onClick={() => setSurface("library")}>Lyric syncs</button>
      <button type="button" role="menuitem" className={item} onClick={() => { close(); commands.openPrefs("playback"); }}>Lyric settings</button>
    </div>}
    {library && <SavedSyncs np={np} onClose={close} current={{title,detail,phase:status.phase}}/>}
  </div>;
}
