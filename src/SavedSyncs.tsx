import {useCallback,useEffect,useRef,useState} from "react";
import {commands,savedSyncs,savedSyncAction,onSyncLibraryChanged,onKaraokeReady,activeSyncs,type ActiveSync,type SavedSync} from "./lib/backend";
import { MorphIcon } from "./icons/MorphIcon";
import { SYNC_GLYPHS } from "./icons/geometry";
import { DUR } from "./lib/tokens";
import type { SyncStatus } from "./lib/backend";
import {ResearchRecording} from "./ResearchRecording";
import type {NowPlaying} from "./types";

function Glyph({kind}:{kind:"close"|"refresh"|"delete"|"music"|"working"}) {
  const paths = {
    close: <path d="m6 6 8 8M14 6l-8 8"/>,
    refresh: <><path d="M15.8 7.5a6 6 0 1 0 .15 4.5"/><path d="M16 3.5v4.25h-4.25"/></>,
    delete: <><path d="M4 6h12M7.5 6V3.75h5V6M5.5 6l.65 10.25h7.7L14.5 6M8.25 9v4.25M11.75 9v4.25"/></>,
    music: <><path d="M13.5 12.5V4l-7 1.5v8.5"/><ellipse cx="4.75" cy="14.5" rx="1.75" ry="1.25"/><ellipse cx="11.75" cy="13" rx="1.75" ry="1.25"/></>,
    working: <><path d="M10 3.5a6.5 6.5 0 1 1-6.5 6.5M10 6.5v4l2.5 1.5"/><path d="M4.25 5.75h.01M6.75 4h.01" strokeWidth="2.4"/></>,
  };
  return <svg width="18" height="18" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[kind]}</svg>;
}
function useActiveSyncs() {
  const [jobs, setJobs] = useState<ActiveSync[]>([]);
  const [unavailable, setUnavailable] = useState(false);
  useEffect(() => {
    let live = true, pending = false;
    let timer: number | undefined;
    const poll = async () => {
      window.clearTimeout(timer);
      if (!live || document.hidden || pending) return;
      pending = true;
      try {
        const next = await activeSyncs();
        if (live) { setJobs(next); setUnavailable(false); }
      } catch { if (live) { setJobs([]); setUnavailable(true); } }
      finally {
        pending = false;
        if (live && !document.hidden) timer = window.setTimeout(poll, 1000);
      }
    };
    void poll();
    document.addEventListener("visibilitychange", poll);
    return () => { live = false; window.clearTimeout(timer); document.removeEventListener("visibilitychange", poll); };
  }, []);
  return {jobs, unavailable};
}
function SyncingRow({job}:{job:ActiveSync}) {
  const finishing = job.phase === "processing";
  return <div className="flex items-center gap-2.5 py-2">
    <div className="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-fg/5 text-muted"><MorphIcon name={SYNC_GLYPHS[job.phase]} size={18} dur={DUR[5]}/></div>
    <div className="min-w-0 flex-1">
      <div className="flex items-baseline gap-2"><span className="truncate text-sm font-medium">{job.title}</span>{!finishing && <span className="ml-auto text-[11px] tabular-nums text-muted">{job.progress}%</span>}</div>
      <div className="mt-0.5 truncate text-[11px] text-muted">{finishing ? "Finishing sync" : "Listening"} · {job.artist}</div>
      {finishing ? <div className="mt-1.5 flex gap-1" role="progressbar" aria-label={`Finishing sync for ${job.title}`} aria-valuetext="Matching vocals to lyrics"><span className="h-0.5 flex-1 rounded-full bg-accent/70"/><span className="h-0.5 flex-1 rounded-full bg-fg/15"/></div>
        : <div className="mt-1.5 h-0.5 overflow-hidden rounded-full bg-fg/10" role="progressbar" aria-label={`Audio captured for ${job.title}`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={job.progress ?? 0}><div className="h-full origin-left rounded-full bg-accent transition-transform duration-3 ease-out-tk motion-reduce:transition-none" style={{transform:`scaleX(${(job.progress ?? 0)/100})`}}/></div>}
    </div>
  </div>;
}
function Cover({song}:{song:SavedSync}){const [src,setSrc]=useState<string|null>(null);useEffect(()=>{let live=true;setSrc(null);if(song.thumb_key)void commands.historyThumbUrl(song.thumb_key).then(s=>{if(live)setSrc(s);}).catch(()=>{});return ()=>{live=false;};},[song.thumb_key]);return <div className="grid h-10 w-10 shrink-0 place-items-center overflow-hidden rounded bg-fg/5 text-muted">{src?<img src={src} alt="" className="h-full w-full object-cover" onError={()=>setSrc(null)}/>:<Glyph kind="music"/>}</div>;}
export function SavedSyncs({np,onClose,current}:{np:NowPlaying;onClose:(restoreFocus?:boolean)=>void;current:{title:string;detail:string;phase:SyncStatus["phase"]}}){
 const {jobs, unavailable}=useActiveSyncs();
 const [rows,setRows]=useState<SavedSync[]|null>(null),[error,setError]=useState(""),[busy,setBusy]=useState<string|null>(null),[notice,setNotice]=useState("");
 const [undo,setUndo]=useState<{key:string;revision:number}|null>(null);const alive=useRef(true),serial=useRef(0),panel=useRef<HTMLDivElement>(null);
 const refresh=useCallback(async()=>{const n=++serial.current;try{const data=await savedSyncs();if(alive.current&&n===serial.current){setRows(data);setError("");}}catch{if(alive.current&&n===serial.current)setError("Couldn’t load saved syncs.");}},[]);
 useEffect(()=>{alive.current=true;void refresh();const a=onSyncLibraryChanged(()=>void refresh()),b=onKaraokeReady(()=>void refresh());return ()=>{alive.current=false;serial.current++;a();b();};},[refresh]);
 useEffect(()=>{panel.current?.focus();const outside=(e:PointerEvent)=>{if(!panel.current?.parentElement?.contains(e.target as Node))onClose(false);};const escape=(e:KeyboardEvent)=>{if(e.key==="Escape"){e.preventDefault();e.stopPropagation();onClose();}};document.addEventListener("pointerdown",outside);document.addEventListener("keydown",escape,true);return ()=>{document.removeEventListener("pointerdown",outside);document.removeEventListener("keydown",escape,true);};},[onClose]);
 async function act(key:string,action:"refresh"|"delete"|"undo",revision?:number){if(busy)return;setBusy(key);setError("");try{const rev=await savedSyncAction(key,action,revision);if(!alive.current)return;if(action==="delete"){setUndo({key,revision:rev});setNotice("Timing deleted.");}else{setUndo(null);setNotice(action==="refresh"?"Will relearn on your next full listen. Current timing stays available.":"Timing restored.");}await refresh();}catch(e){if(alive.current)setError(typeof e==="string"?e:"Couldn’t update this sync.");}finally{if(alive.current)setBusy(null);}}
 const sorted=rows?[...rows].sort((a,b)=>Number(b.title===np.title&&b.artist===np.artist)-Number(a.title===np.title&&a.artist===np.artist)):[];
 const btn="grid h-7 w-7 shrink-0 place-items-center rounded-md text-fg/65 transition-[color,background-color,transform] duration-2 ease-out-tk hover:bg-fg/10 hover:text-fg active:scale-95 motion-reduce:transition-none focus-visible:outline focus-visible:outline-2 focus-visible:outline-fg/50 disabled:opacity-40";
 return <div ref={panel} role="dialog" aria-label="Saved syncs" tabIndex={-1} onMouseDown={e=>e.stopPropagation()} onKeyDown={e=>{if(e.key==="Escape"){e.stopPropagation();onClose();}}} className="absolute right-0 top-full z-50 mt-2 flex max-h-[min(320px,65vh)] w-[312px] max-w-[calc(100vw-32px)] flex-col overflow-hidden rounded-xl border border-border/15 bg-surface text-fg shadow-xl shadow-black/40 outline-none">
  <div className="flex shrink-0 items-center justify-between px-3 pb-2 pt-3"><div className="text-sm font-medium">Lyric syncs</div><button className={btn} aria-label="Close saved syncs" onClick={()=>onClose()}><Glyph kind="close"/></button></div>
  <div className="sync-scroll min-h-0 overflow-y-auto overscroll-contain px-1 pb-2">
    <section aria-label="Current song sync status" className="mx-2 mb-3 border-b border-border/10 pb-3 pt-1">
      <div className="flex items-center gap-2 text-sm font-medium"><MorphIcon name={SYNC_GLYPHS[current.phase]} size={17} dur={DUR[5]}/><span>{current.title}</span></div>
      <div className="mt-1 truncate text-xs text-fg/80">{np.title} · {np.artist}</div>
      <p className="mt-1 text-xs leading-5 text-muted">{current.detail}</p>
      <ResearchRecording np={np}/>
    </section>
    {jobs.length>0 && <section aria-label="Currently syncing" className="mx-2 mb-2 border-b border-border/10 pb-2"><h3 className="pt-1 text-[10px] font-medium uppercase tracking-wider text-muted">Syncing · {jobs.length}</h3>{jobs.map(job=><SyncingRow key={job.key} job={job}/>)}</section>}
    {unavailable && <p className="px-2 pb-2 text-xs text-muted">Sync activity is temporarily unavailable.</p>}
    <div className="px-2 pb-1 pt-1 text-[10px] font-medium uppercase tracking-wider text-muted">Saved <span className="ml-1 tabular-nums">{rows?.length ?? ""}</span></div>{rows===null&&!error?<p className="px-3 py-7 text-xs text-muted">Loading saved timing…</p>:rows?.length===0?<div className="px-3 py-7"><p className="text-sm">Your collection starts here.</p><p className="mt-1 text-xs leading-5 text-muted">Listen to a song with lyrics from start to finish. Saved word timing will appear here.</p></div>:sorted.map(song=><div key={song.key} className="group/sync flex min-h-14 items-center gap-2.5 rounded-lg px-2 py-1.5 hover:bg-fg/5"><Cover song={song}/><div className="min-w-0 flex-1"><div className="truncate text-sm font-medium">{song.title}</div><div className="truncate text-xs text-muted">{song.refresh?"Refresh on next listen":song.artist}</div></div><div className="flex shrink-0 opacity-0 group-hover/sync:opacity-100 focus-within:opacity-100"><button className={btn} disabled={!!busy} title="Relearn on next full listen" aria-label={`Refresh timing for ${song.title}`} onClick={()=>void act(song.key,"refresh")}><Glyph kind="refresh"/></button><button className={btn} disabled={!!busy} title="Delete timing" aria-label={`Delete timing for ${song.title}`} onClick={()=>void act(song.key,"delete")}><Glyph kind="delete"/></button></div></div>)}</div>
  {error&&<div role="alert" className="px-3 pb-3 text-xs text-muted">{error} <button className="underline" onClick={()=>void refresh()}>Retry</button></div>}
  {notice&&<div role="status" className="border-t border-border/10 px-3 py-2 text-xs leading-5 text-muted">{notice}{undo&&<button className="ml-2 text-fg underline" disabled={!!busy} onClick={()=>void act(undo.key,"undo",undo.revision)}>Undo</button>}</div>}
 </div>;
}
