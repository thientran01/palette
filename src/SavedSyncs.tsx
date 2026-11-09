import {useCallback,useEffect,useLayoutEffect,useRef,useState} from "react";
import {commands,savedSyncs,savedSyncAction,onSyncLibraryChanged,onKaraokeReady,activeSyncs,type ActiveSync,type SavedSync,type SyncCheck} from "./lib/backend";
import {useArt} from "./lib/artAccent";
import {checkSummary,onTimePercent,savedWhen,sortSyncs,type SyncSort} from "./lib/syncCheck";
import { MorphIcon } from "./icons/MorphIcon";
import { SYNC_GLYPHS } from "./icons/geometry";
import { DUR } from "./lib/tokens";
import type { SyncStatus } from "./lib/backend";
import {ResearchRecording,actionStyle} from "./ResearchRecording";
import type {NowPlaying} from "./types";

function Glyph({kind,size=18}:{kind:"close"|"refresh"|"delete"|"music"|"working";size?:number}) {
  const paths = {
    close: <path d="m6 6 8 8M14 6l-8 8"/>,
    refresh: <><path d="M15.8 7.5a6 6 0 1 0 .15 4.5"/><path d="M16 3.5v4.25h-4.25"/></>,
    delete: <><path d="M4 6h12M7.5 6V3.75h5V6M5.5 6l.65 10.25h7.7L14.5 6M8.25 9v4.25M11.75 9v4.25"/></>,
    music: <><path d="M13.5 12.5V4l-7 1.5v8.5"/><ellipse cx="4.75" cy="14.5" rx="1.75" ry="1.25"/><ellipse cx="11.75" cy="13" rx="1.75" ry="1.25"/></>,
    working: <><path d="M10 3.5a6.5 6.5 0 1 1-6.5 6.5M10 6.5v4l2.5 1.5"/><path d="M4.25 5.75h.01M6.75 4h.01" strokeWidth="2.4"/></>,
  };
  return <svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[kind]}</svg>;
}
const PANEL_MAX = 560;
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
function Cover({thumbKey,url}:{thumbKey:string|null;url?:string|null}){const [src,setSrc]=useState<string|null>(null);useEffect(()=>{let live=true;setSrc(null);if(!url&&thumbKey)void commands.historyThumbUrl(thumbKey).then(s=>{if(live)setSrc(s);}).catch(()=>{});return ()=>{live=false;};},[thumbKey,url]);const shown=url??src;return <div className="art-edge grid h-10 w-10 shrink-0 place-items-center overflow-hidden rounded bg-fg/5 text-muted">{shown?<img src={shown} alt="" className="h-full w-full object-cover" onError={()=>setSrc(null)}/>:<Glyph kind="music"/>}</div>;}
/** The song as the review heard it: one tick per lyric line. Unsure lines
 * stand tallest, so where the sync may be off reads at a glance. Neutral
 * ink only (album colour never marks chrome); nothing moves. */
function LineMap({check}:{check:SyncCheck}){
 const lines=[...check.lines];
 const sure=lines.filter(c=>c==="s").length,timed=lines.filter(c=>c!=="n").length;
 const tick={s:"h-[5px] bg-fg/20",l:"h-[11px] bg-fg/45",u:"h-5 bg-fg",n:"h-[5px] shadow-[inset_0_0_0_1px_rgb(var(--fg)/0.22)]"} as const;
 return <div role="img" aria-label={`${sure} of ${timed} lines sure, ${check.unsure} unsure`} className={`mt-3 flex h-5 items-end ${lines.length>60?"gap-px":"gap-0.5"}`}>
  {lines.map((c,i)=><i key={i} className={`min-w-px flex-1 rounded-[1px] ${tick[c as keyof typeof tick]??tick.l}`}/>)}
 </div>;
}
function CurrentSong({np,mine,current,busy,onRelearn}:{np:NowPlaying;mine:SavedSync|null;current:{title:string;detail:string;phase:SyncStatus["phase"]};busy:boolean;onRelearn:()=>void}){
 const art=useArt(np.art_id??null);
 const check=current.phase==="saved"?mine?.check??null:null;
 const sub=mine&&current.phase==="saved"?`${np.artist} · learned ${savedWhen(mine.saved_at)}`:np.artist;
 const note=check?checkSummary(check)
  :current.phase!=="saved"?null
  :mine?.refresh?"Relearns on your next full listen. This timing stays until then."
  :mine?.relearns?"Learned before the check. Your next full listen checks it."
  :null;
 return <section aria-label="Current song sync" className="mx-2 mb-3 border-b border-border/10 pb-3 pt-1">
  <div className="flex items-center gap-2.5">
   <Cover thumbKey={mine?.thumb_key??null} url={art}/>
   <div className="min-w-0 flex-1"><div className="truncate text-sm font-medium">{np.title}</div><div className="truncate text-xs text-muted">{sub}</div></div>
   {check&&<div className="shrink-0 text-right" title="Estimated share of words that land within 0.3s of the singing"><div className="text-[22px] font-semibold leading-none tracking-[-0.01em] tabular-nums">{onTimePercent(check)}%</div><div className="mt-1 text-[10.5px] text-muted">words on time</div></div>}
  </div>
  {current.phase!=="saved"&&<div className="mt-3"><div className="flex items-center gap-2 text-xs font-medium"><MorphIcon name={SYNC_GLYPHS[current.phase]} size={16} dur={DUR[5]}/><span>{current.title}</span></div><p className="mt-1 text-xs leading-5 text-muted">{current.detail}</p></div>}
  {check&&<LineMap check={check}/>}
  {note&&<p className="mt-2.5 text-[11.5px] leading-[17px] text-fg/80">{note}</p>}
  <ResearchRecording np={np} lead={current.phase==="saved"&&mine&&!mine.refresh&&<button className={actionStyle} disabled={busy} title="Learn this song again on your next full listen" onClick={onRelearn}><Glyph kind="refresh" size={14}/>Relearn</button>}/>
 </section>;
}
export function SavedSyncs({np,onClose,current}:{np:NowPlaying;onClose:(restoreFocus?:boolean)=>void;current:{title:string;detail:string;phase:SyncStatus["phase"]}}){
 const {jobs, unavailable}=useActiveSyncs();
 const [rows,setRows]=useState<SavedSync[]|null>(null),[error,setError]=useState(""),[busy,setBusy]=useState<string|null>(null),[notice,setNotice]=useState("");
 const [undo,setUndo]=useState<{key:string;revision:number}|null>(null),[sort,setSort]=useState<SyncSort>("recent"),[maxH,setMaxH]=useState<number|null>(null);const alive=useRef(true),serial=useRef(0),panel=useRef<HTMLDivElement>(null);
 const refresh=useCallback(async()=>{const n=++serial.current;try{const data=await savedSyncs();if(alive.current&&n===serial.current){setRows(data);setError("");}}catch{if(alive.current&&n===serial.current)setError("Couldn’t load saved syncs.");}},[]);
 useEffect(()=>{alive.current=true;void refresh();const a=onSyncLibraryChanged(()=>void refresh()),b=onKaraokeReady(()=>void refresh());return ()=>{alive.current=false;serial.current++;a();b();};},[refresh]);
 useEffect(()=>{panel.current?.focus();const outside=(e:PointerEvent)=>{if(!panel.current?.parentElement?.contains(e.target as Node))onClose(false);};const escape=(e:KeyboardEvent)=>{if(e.key==="Escape"){e.preventDefault();e.stopPropagation();onClose();}};document.addEventListener("pointerdown",outside);document.addEventListener("keydown",escape,true);return ()=>{document.removeEventListener("pointerdown",outside);document.removeEventListener("keydown",escape,true);};},[onClose]);
 async function act(key:string,action:"refresh"|"delete"|"undo",revision?:number){if(busy)return;setBusy(key);setError("");try{const rev=await savedSyncAction(key,action,revision);if(!alive.current)return;if(action==="delete"){setUndo({key,revision:rev});setNotice("Timing deleted.");}else{setUndo(null);setNotice(action==="refresh"?"Will relearn on your next full listen. Current timing stays available.":"Timing restored.");}await refresh();}catch(e){if(alive.current)setError(typeof e==="string"?e:"Couldn’t update this sync.");}finally{if(alive.current)setBusy(null);}}
 // Reach down to the window's bottom edge, over the transport while open
 // (the window never resizes; the old 65vh cap left the expanded widget's
 // lower third unused). Focus mode's tall room stops at PANEL_MAX.
 useLayoutEffect(()=>{const fit=()=>{const top=panel.current?.getBoundingClientRect().top;if(top!==undefined)setMaxH(Math.max(0,Math.min(PANEL_MAX,Math.floor(window.innerHeight-top-8))));};fit();window.addEventListener("resize",fit);return ()=>window.removeEventListener("resize",fit);},[]);
 const mine=rows?.find(r=>r.title===np.title&&r.artist===np.artist)??null;
 const list=rows?sortSyncs(rows,sort):[];
 const btn="grid h-7 w-7 shrink-0 place-items-center rounded-md text-fg/65 transition-[color,background-color,transform] duration-2 ease-out-tk hover:bg-fg/10 hover:text-fg active:scale-95 motion-reduce:transition-none focus-visible:outline focus-visible:outline-2 focus-visible:outline-fg/50 disabled:opacity-40";
 const seg=(on:boolean)=>`rounded-[5px] px-[7px] py-[3px] text-[11px] font-medium normal-case tracking-normal transition-colors duration-2 ease-out-tk focus-visible:outline focus-visible:outline-2 focus-visible:outline-fg/50 ${on?"bg-fg/10 text-fg":"text-muted hover:text-fg"}`;
 return <div ref={panel} role="dialog" aria-label="Lyric syncs" data-sync-library tabIndex={-1} onMouseDown={e=>e.stopPropagation()} onKeyDown={e=>{if(e.key==="Escape"){e.stopPropagation();onClose();}}} style={maxH===null?undefined:{maxHeight:maxH}} className="absolute right-0 top-full z-50 mt-2 flex max-h-[min(320px,65vh)] w-[312px] max-w-[calc(100vw-32px)] flex-col overflow-hidden rounded-xl border border-border/15 bg-surface text-fg shadow-xl shadow-black/40 outline-none">
  <div className="flex shrink-0 items-center justify-between px-3 pb-2 pt-3"><div className="text-sm font-medium">Lyric syncs</div><button className={btn} aria-label="Close lyric syncs" onClick={()=>onClose()}><Glyph kind="close"/></button></div>
  <div className="sync-scroll min-h-0 overflow-y-auto overscroll-contain px-1 pb-2">
    <CurrentSong np={np} mine={mine} current={current} busy={!!busy} onRelearn={()=>{if(mine)void act(mine.key,"refresh");}}/>
    {jobs.length>0 && <section aria-label="Currently syncing" className="mx-2 mb-2 border-b border-border/10 pb-2"><h3 className="pt-1 text-[10px] font-medium uppercase tracking-wider text-muted">Syncing · {jobs.length}</h3>{jobs.map(job=><SyncingRow key={job.key} job={job}/>)}</section>}
    {unavailable && <p className="px-2 pb-2 text-xs text-muted">Sync activity is temporarily unavailable.</p>}
    <div className="flex items-center justify-between pb-1 pl-2 pr-1 pt-1 text-[10px] font-medium uppercase tracking-wider text-muted"><span>Saved <span className="ml-1 tabular-nums">{rows?.length ?? ""}</span></span>
      {!!rows?.length&&<div className="flex gap-0.5" role="group" aria-label="Sort saved syncs"><button className={seg(sort==="recent")} aria-pressed={sort==="recent"} onClick={()=>setSort("recent")}>Recent</button><button className={seg(sort==="least")} aria-pressed={sort==="least"} onClick={()=>setSort("least")}>Least sure</button></div>}
    </div>
    {rows===null&&!error?<p className="px-3 py-7 text-xs text-muted">Loading saved timing…</p>:rows?.length===0?<div className="px-3 py-7"><p className="text-sm">Your collection starts here.</p><p className="mt-1 text-xs leading-5 text-muted">Listen to a song with lyrics from start to finish. Saved word timing will appear here.</p></div>:list.map(song=><SavedRow key={song.key} song={song} busy={!!busy} btn={btn} act={act}/>)}</div>
  {error&&<div role="alert" className="px-3 pb-3 text-xs text-muted">{error} <button className="underline" onClick={()=>void refresh()}>Retry</button></div>}
  {notice&&<div role="status" className="border-t border-border/10 px-3 py-2 text-xs leading-5 text-muted">{notice}{undo&&<button className="ml-2 text-fg underline" disabled={!!busy} onClick={()=>void act(undo.key,"undo",undo.revision)}>Undo</button>}</div>}
 </div>;
}
/** The estimate rests at the row's end; hover or focus swaps it for the
 * row's actions in the same seat, so neither shifts the title. */
function SavedRow({song,busy,btn,act}:{song:SavedSync;busy:boolean;btn:string;act:(key:string,action:"refresh"|"delete")=>Promise<void>}){
 const when=song.relearns?song.check?"relearns next listen":"not checked yet":savedWhen(song.saved_at);
 const swap="transition-opacity duration-2 ease-out-tk motion-reduce:transition-none";
 return <div className="group/sync flex min-h-14 items-center gap-2.5 rounded-lg px-2 py-1.5 hover:bg-fg/5">
  <Cover thumbKey={song.thumb_key}/>
  <div className="min-w-0 flex-1"><div className="truncate text-sm font-medium">{song.title}</div><div className="truncate text-xs text-muted">{song.refresh?"Relearns on your next full listen":`${song.artist} · ${when}`}</div></div>
  <div className="relative flex shrink-0 items-center justify-end">
   <span aria-hidden={!song.check} title={song.check?"Estimated words on time":"Not checked yet"} className={`pointer-events-none absolute right-1 text-xs tabular-nums ${song.check?"text-fg/85":"text-muted"} ${swap} group-hover/sync:opacity-0 group-focus-within/sync:opacity-0`}>{song.check?`${onTimePercent(song.check)}%`:"—"}</span>
   <div className={`flex opacity-0 ${swap} group-hover/sync:opacity-100 focus-within:opacity-100`}><button className={btn} disabled={busy} title="Relearn on next full listen" aria-label={`Relearn timing for ${song.title}`} onClick={()=>void act(song.key,"refresh")}><Glyph kind="refresh"/></button><button className={btn} disabled={busy} title="Delete timing" aria-label={`Delete timing for ${song.title}`} onClick={()=>void act(song.key,"delete")}><Glyph kind="delete"/></button></div>
  </div>
 </div>;
}
