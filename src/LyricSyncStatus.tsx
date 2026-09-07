import { useCallback, useEffect, useId, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { MorphIcon } from "./icons/MorphIcon";
import { SYNC_GLYPHS } from "./icons/geometry";
import { vocalPreviewEnabled, karaokeStatus, onCursorLeft, type SyncStatus } from "./lib/backend";
import { DUR, EASE } from "./lib/tokens";
import type { NowPlaying } from "./types";
import { resolveSyncStatus, savedSyncStatus } from "./lib/lyricSyncState";

import { SavedSyncs } from "./SavedSyncs";

const titles = {waiting:"Waiting to learn",learning:"Learning timing",processing:"Finishing sync",saved:"Word sync saved",failed:"Couldn’t sync this track"};
const savedStatus = savedSyncStatus;

/** Quiet status, never a second beat visualizer. Snapshot polling is scoped
 * to this visible lyric surface and guarded against late replies on skips. */
export function LyricSyncStatus({np, saved}: {np:NowPlaying; saved:boolean}) {
  const [preview,setPreview]=useState(false);
  useEffect(()=>{let active=true;void vocalPreviewEnabled().then(value=>{if(active)setPreview(value);}).catch(()=>{});return ()=>{active=false;};},[]);
  const key = JSON.stringify([np.artist,np.title,np.album,np.duration_ms]);
  const [snapshot,setSnapshot]=useState<{key:string; status:SyncStatus} | null>(null);
  const [open,setOpen]=useState(false);
  const [library,setLibrary]=useState(false);
  const trigger=useRef<HTMLButtonElement>(null);
  const closeLibrary=useCallback((restoreFocus=true)=>{setLibrary(false);if(restoreFocus)trigger.current?.focus();},[]);
  const timer=useRef<number | undefined>(undefined);
  const id=useId();
  const reduced=useReducedMotion();
  const clear=()=>window.clearTimeout(timer.current);
  useEffect(()=>{
    let disposed=false;
    let poll: number | undefined;
    let inFlight=false;
    const refresh=async()=>{
      window.clearTimeout(poll);
      if(disposed || document.hidden || (saved && !preview) || inFlight) return;
      inFlight=true;
      try {
        const status=await karaokeStatus(np);
        if(!disposed) setSnapshot({key,status});
      } catch {
        if(!disposed) setSnapshot({key,status:{phase:"waiting",detail:"Sync status is unavailable right now."}});
      }
      inFlight=false;
      if(!disposed) poll=window.setTimeout(refresh,2000);
    };
    void refresh();
    document.addEventListener("visibilitychange",refresh);
    return ()=>{disposed=true;window.clearTimeout(poll);document.removeEventListener("visibilitychange",refresh);};
  // Identity, not the frequently updated playback position, owns a request.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  },[key,saved,preview]);
  useEffect(()=>{clear();setOpen(false);},[key]);
  useEffect(()=>{
    if(!open || library) return;
    const escape=(e:KeyboardEvent)=>{if(e.key==="Escape"){e.preventDefault();e.stopPropagation();clear();setOpen(false);}};
    document.addEventListener("keydown",escape,true);
    return ()=>document.removeEventListener("keydown",escape,true);
  },[open,library]);
  useEffect(()=>onCursorLeft(()=>{clear();setOpen(false);}),[]);
  useEffect(()=>clear,[]);
  const status=resolveSyncStatus(saved,preview,snapshot?.key===key ? snapshot.status : null);
  const title=preview ? {waiting:"Vocal sync trial",learning:"Learning vocal timing",processing:"Finishing vocal sync",saved:"Vocal sync saved",failed:"Vocal sync needs another try"}[status.phase] : titles[status.phase];
  // The heading names the state; the detail explains the result or next step.
  // Keep this presentation-only so copy updates do not interrupt capture.
  const detail = status.phase === "saved" ? preview ? "Experimental vocal timing is ready for this song." : savedStatus.detail
    : status.phase === "processing" ? preview ? "Isolating vocals and matching them to the lyrics. Your saved timing stays available." : "Your recording is being matched to the lyrics."
    : status.phase === "learning" ? np.status !== "playing"
      ? "Resume playback to continue this listen."
      : status.detail.replace(/^Learning timing…\s*/, "")
    : status.detail
      .replace(/^Couldn’t save word sync\.\s*/, "")
      .replace("Couldn’t align this track’s vocals. Word sync was not saved.",
        "This recording did not produce usable word timing. Keeping line sync.");
  return <div className="relative shrink-0">
    <button onMouseEnter={()=>{clear();if(!library)timer.current=window.setTimeout(()=>setOpen(true),DUR[4]);}} onMouseLeave={()=>{clear();timer.current=window.setTimeout(()=>setOpen(false),DUR[2]);}} ref={trigger} type="button" aria-haspopup="dialog" aria-expanded={library} aria-label={`Open lyric syncs: ${title}`} aria-describedby={open&&!library?id:undefined}
      className="grid h-7 w-7 place-items-center rounded-md text-fg [transition:color_140ms_var(--ease-out-tk),background-color_140ms_var(--ease-out-tk),scale_90ms_var(--ease-out-tk)] hover:bg-fg/10 aria-expanded:bg-fg/10 active:scale-95 motion-reduce:transition-none focus-visible:outline focus-visible:outline-2 focus-visible:outline-fg/50"
      onFocus={()=>{clear();if(!library)setOpen(true);}} onBlur={()=>{clear();setOpen(false);}}
      onClick={()=>{clear();setOpen(false);setLibrary(v=>!v);}} onKeyDown={e=>{if(e.key==="Escape"){clear();setOpen(false);if(library)closeLibrary();e.stopPropagation();}}}>
      <MorphIcon name={SYNC_GLYPHS[status.phase]} size={16} dur={DUR[5]} />
    </button>
    {library && <SavedSyncs np={np} onClose={closeLibrary} current={{title,detail,phase:status.phase}}/>}
    <AnimatePresence>{open && !library && <motion.div id={id} role="tooltip" onMouseEnter={clear} onMouseLeave={()=>{clear();timer.current=window.setTimeout(()=>setOpen(false),DUR[2]);}}
      initial={{opacity:0,y:-2}} animate={{opacity:1,y:0}} exit={{opacity:0}}
      transition={{duration:reduced?0:DUR[2]/1000,ease:[...EASE.out]}}
      className="absolute right-0 top-full z-50 mt-1.5 w-max max-w-[calc(100vw-40px)] rounded-md border border-border/10 bg-surface-2 px-2.5 py-2 text-xs leading-4 text-fg shadow-lg shadow-black/40">
      Lyric syncs
    </motion.div>}</AnimatePresence>
  </div>;
}
