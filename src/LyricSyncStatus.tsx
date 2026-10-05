import { useCallback, useEffect, useRef, useState } from "react";
import { vocalPreviewEnabled, karaokeStatus, type SyncStatus } from "./lib/backend";
import { MorphIcon } from "./icons/MorphIcon";
import { DUR } from "./lib/tokens";
import type { NowPlaying } from "./types";
import { resolveSyncStatus, savedSyncStatus } from "./lib/lyricSyncState";
import { SavedSyncs } from "./SavedSyncs";

const titles = {waiting:"Waiting to learn",learning:"Learning timing",processing:"Finishing sync",saved:"Word sync saved",failed:"Couldn’t sync this track"};

/** Direct sync-library access in the shared corner controls, including queue.
 * The existing ribbon is a stable entry icon; live status lives in the library,
 * so closing it cannot leave a stale processing glyph in the corner. */
export function LyricSyncStatus({np, saved}: {np:NowPlaying; saved:boolean}) {
  const [preview, setPreview] = useState(false);
  const [library, setLibrary] = useState(false);
  const [snapshot, setSnapshot] = useState<{key:string; status:SyncStatus} | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const key = JSON.stringify([np.artist,np.title,np.album,np.duration_ms]);
  const close = useCallback((restoreFocus = true) => {
    setLibrary(false);
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

  const status = resolveSyncStatus(saved,preview,snapshot?.key === key ? snapshot.status : null);
  const title = preview ? {waiting:"Vocal sync trial",learning:"Learning vocal timing",processing:"Finishing vocal sync",saved:"Vocal sync saved",failed:"Vocal sync needs another try"}[status.phase] : titles[status.phase];
  const detail = status.phase === "saved" ? preview ? "Experimental vocal timing is ready for this song." : savedSyncStatus.detail
    : status.phase === "processing" ? preview ? "Isolating vocals and matching them to the lyrics. Your saved timing stays available." : "Your recording is being matched to the lyrics."
    : status.phase === "learning" ? np.status !== "playing" ? "Resume playback to continue this listen." : status.detail.replace(/^Learning timing…\s*/, "")
    : status.detail.replace(/^Couldn’t save word sync\.\s*/, "").replace("Couldn’t align this track’s vocals. Word sync was not saved.", "This recording did not produce usable word timing. Keeping line sync.");
  return <div className="relative shrink-0" onMouseDown={e => e.stopPropagation()}>
    <button ref={trigger} type="button" aria-label="Open lyric syncs" title="Lyric syncs" aria-haspopup="dialog" aria-expanded={library}
      className="grid h-7 w-7 place-items-center rounded-md text-fg transition-[background-color,scale] duration-2 ease-out-tk hover:bg-fg/10 aria-expanded:bg-fg/10 active:scale-95 focus-visible:outline focus-visible:outline-2 focus-visible:outline-fg/50"
      onClick={() => setLibrary(current => !current)}>
      <MorphIcon name="syncSaved" size={16} dur={DUR[5]} />
    </button>
    {library && <SavedSyncs np={np} onClose={close} current={{title,detail,phase:status.phase}}/>}
  </div>;
}
