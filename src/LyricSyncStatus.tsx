import { useEffect, useId, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { MorphIcon } from "./icons/MorphIcon";
import type { MorphName } from "./icons/geometry";
import { karaokeStatus, onCursorLeft, type SyncStatus } from "./lib/backend";
import { DUR, EASE } from "./lib/tokens";
import type { NowPlaying } from "./types";

const glyphs: Record<SyncStatus["phase"], MorphName> = {
  waiting:"syncWaiting", learning:"syncLearning", processing:"syncProcessing", saved:"syncSaved", failed:"syncFailed",
};
const titles = {waiting:"Waiting to learn",learning:"Learning timing",processing:"Finishing sync",saved:"Word sync saved",failed:"Couldn’t sync this track"};
const savedStatus: SyncStatus = {phase:"saved",detail:"Available on this device for future listens."};

/** Quiet status, never a second beat visualizer. Snapshot polling is scoped
 * to this visible lyric surface and guarded against late replies on skips. */
export function LyricSyncStatus({np, saved}: {np:NowPlaying; saved:boolean}) {
  const key = JSON.stringify([np.artist,np.title,np.album,np.duration_ms]);
  const [snapshot,setSnapshot]=useState<{key:string; status:SyncStatus} | null>(null);
  const [open,setOpen]=useState(false);
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
      if(disposed || document.hidden || saved || inFlight) return;
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
  },[key,saved]);
  useEffect(()=>{clear();setOpen(false);},[key]);
  useEffect(()=>{
    if(!open) return;
    const escape=(e:KeyboardEvent)=>{if(e.key==="Escape"){e.preventDefault();e.stopPropagation();clear();setOpen(false);}};
    document.addEventListener("keydown",escape,true);
    return ()=>document.removeEventListener("keydown",escape,true);
  },[open]);
  useEffect(()=>onCursorLeft(()=>{clear();setOpen(false);}),[]);
  useEffect(()=>clear,[]);
  const status=saved ? savedStatus : snapshot?.key===key ? snapshot.status : {phase:"waiting" as const,detail:"Checking word sync…"};
  // The heading names the state; the detail explains the result or next step.
  // Keep this presentation-only so copy updates do not interrupt capture.
  const detail = status.phase === "saved" ? savedStatus.detail
    : status.phase === "processing" ? "Your recording is being matched to the lyrics."
    : status.phase === "learning" ? np.status !== "playing"
      ? "Resume playback to continue this listen."
      : status.detail.replace(/^Learning timing…\s*/, "")
    : status.detail
      .replace(/^Couldn’t save word sync\.\s*/, "")
      .replace("Couldn’t align this track’s vocals. Word sync was not saved.",
        "This recording did not produce usable word timing. Keeping line sync.");
  return <div className="relative shrink-0"
    onMouseEnter={()=>{clear();timer.current=window.setTimeout(()=>setOpen(true),DUR[4]);}}
    onMouseLeave={()=>{clear();timer.current=window.setTimeout(()=>setOpen(false),DUR[2]);}}>
    <button type="button" aria-label={`Lyric sync: ${titles[status.phase]}`} aria-describedby={open?id:undefined}
      className="grid h-7 w-7 place-items-center rounded-md text-fg [transition:color_140ms_var(--ease-out-tk),background-color_140ms_var(--ease-out-tk),scale_90ms_var(--ease-out-tk)] hover:bg-fg/10 active:scale-95 focus-visible:outline focus-visible:outline-2 focus-visible:outline-fg/50"
      onFocus={()=>{clear();setOpen(true);}} onBlur={()=>{clear();setOpen(false);}}
      onClick={()=>{clear();setOpen(true);}} onKeyDown={e=>{if(e.key==="Escape"){clear();setOpen(false);e.stopPropagation();}}}>
      <MorphIcon name={glyphs[status.phase]} size={13} dur={DUR[5]} />
    </button>
    <AnimatePresence>{open && <motion.div id={id} role="tooltip"
      initial={{opacity:0,y:-2}} animate={{opacity:1,y:0}} exit={{opacity:0}}
      transition={{duration:reduced?0:DUR[2]/1000,ease:[...EASE.out]}}
      className="absolute right-0 top-full z-50 mt-1.5 w-56 max-w-[calc(100vw-40px)] rounded-md border border-border/10 bg-surface-2 px-2.5 py-2 text-xs leading-4 text-fg shadow-lg shadow-black/40">
      <div className="font-medium">{titles[status.phase]}</div>
      <div className="mt-0.5 text-muted">{detail}</div>
    </motion.div>}</AnimatePresence>
  </div>;
}