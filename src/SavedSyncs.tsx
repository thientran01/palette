import {memo,startTransition,useCallback,useEffect,useLayoutEffect,useRef,useState} from "react";
import {commands,savedSyncs,savedSyncAction,onSyncLibraryChanged,onKaraokeReady,activeSyncs,type ActiveSync,type SavedSync,type SyncCheck} from "./lib/backend";
import {useArt} from "./lib/artAccent";
import {checkSummary,onTimePercent,savedWhen,sortSyncs,type SyncSort} from "./lib/syncCheck";
import { MorphIcon } from "./icons/MorphIcon";
import { SYNC_GLYPHS } from "./icons/geometry";
import { DUR } from "./lib/tokens";
import type { SyncStatus } from "./lib/backend";
import {ResearchRecording,actionStyle} from "./ResearchRecording";
import {isWindowShown,onWindowShown} from "./lib/windowShown";
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
/** Saved rows drawn on the opening frame: more than the tallest panel shows. */
const FIRST_ROWS = 8;
function useActiveSyncs() {
  const [jobs, setJobs] = useState<ActiveSync[]>([]);
  const [unavailable, setUnavailable] = useState(false);
  useEffect(() => {
    let live = true, pending = false;
    let timer: number | undefined;
    const poll = async () => {
      window.clearTimeout(timer);
      if (!live || !isWindowShown() || pending) return;
      pending = true;
      try {
        const next = await activeSyncs();
        if (live) { setJobs(next); setUnavailable(false); }
      } catch { if (live) { setJobs([]); setUnavailable(true); } }
      finally {
        pending = false;
        if (live && isWindowShown()) timer = window.setTimeout(poll, 1000);
      }
    };
    void poll();
    // Rust's shown signal, not visibilitychange: a hidden Tauri window never
    // fires it (lib/windowShown.ts).
    const offShown = onWindowShown(() => void poll());
    return () => { live = false; window.clearTimeout(timer); offShown(); };
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
/* The list and its covers outlive the panel. Reopening shows the last list
 * at once while a fresh read runs behind it, instead of "Loading" and one
 * cover request per row on every open. Covers arrive in batches. */
let lastRows:SavedSync[]|null=null;
let rowsInFlight:Promise<SavedSync[]>|null=null;
function readRows(){const p=savedSyncs().then(r=>{lastRows=r;requestThumbs(r.map(x=>x.thumb_key));return r;});rowsInFlight=p;void p.catch(()=>{}).finally(()=>{if(rowsInFlight===p)rowsInFlight=null;});return p;}
/** Starts the first read before the panel opens (hover or focus on its
 * button), so even the first open rarely waits. */
export function prefetchSavedSyncs(){if(!lastRows&&!rowsInFlight)void readRows().catch(()=>{});}
const THUMB_BATCH=200;
const thumbs=new Map<string,string|null>(),thumbQueue=new Set<string>(),thumbListeners=new Set<()=>void>();
let thumbPump=false;
/** Queues covers not held yet; a miss (no thumb captured) is asked again
 * next time, since the song may have played visibly since. */
function requestThumbs(keys:(string|null)[]){for(const k of keys)if(k&&!thumbs.get(k))thumbQueue.add(k);void pumpThumbs();}
async function pumpThumbs(){
 if(thumbPump)return;thumbPump=true;
 try{while(thumbQueue.size){const part=[...thumbQueue].slice(0,THUMB_BATCH);part.forEach(k=>thumbQueue.delete(k));const urls=await commands.historyThumbUrls(part).catch(()=>[] as (string|null)[]);part.forEach((k,i)=>thumbs.set(k,urls[i]??null));thumbListeners.forEach(f=>f());}}
 finally{thumbPump=false;}
}
function useThumbs(){const [,bump]=useState(0);useEffect(()=>{const f=()=>bump(n=>n+1);thumbListeners.add(f);return ()=>{thumbListeners.delete(f);};},[]);}
function Cover({thumbKey,url}:{thumbKey:string|null;url?:string|null}){const [broken,setBroken]=useState<string|null>(null);const shown=url??(thumbKey?thumbs.get(thumbKey)??null:null);return <div className="art-edge grid h-10 w-10 shrink-0 place-items-center overflow-hidden rounded bg-fg/5 text-muted">{shown&&shown!==broken?<img src={shown} alt="" decoding="async" className="h-full w-full object-cover" onError={()=>setBroken(shown)}/>:<Glyph kind="music"/>}</div>;}
/** One tick per lyric line, the same strip in every state so the card never
 * changes shape: while a listen is captured the heard share fills in, and
 * once the check lands each line rises by how sure the review is (unsure
 * lines stand tallest). Neutral ink only (album colour never marks
 * chrome). The rise is a one-shot state change, never ambient motion. */
type MapMode={kind:"check";check:SyncCheck}|{kind:"heard";count:number;share:number};
function SongMap({mode}:{mode:MapMode}){
 const ticks=mode.kind==="check"?[...mode.check.lines]:Array.from({length:mode.count},(_,i)=>(i+0.5)/mode.count<=mode.share?"h":"-");
 const tick:{[k:string]:string}={f:"h-[5px] bg-fg/50",s:"h-[5px] bg-fg/25",l:"h-[11px] bg-fg/50",u:"h-5 bg-fg",n:"h-[5px] bg-fg/[0.06] shadow-[inset_0_0_0_1px_rgb(var(--fg)/0.22)]",h:"h-[5px] bg-fg/50","-":"h-[5px] bg-fg/[0.12]"};
 const label=mode.kind==="check"
  ?`${[...mode.check.lines].filter(c=>c==="s").length} of ${[...mode.check.lines].filter(c=>c!=="n").length} lines sure, ${mode.check.unsure} unsure${mode.check.hand?`, ${mode.check.hand} fixed by hand`:""}`
  :`${Math.round(mode.share*100)}% of the song heard`;
 return <div role="img" aria-label={label} className={`flex h-5 items-end ${ticks.length>60?"gap-px":"gap-0.5"}`}>
  {ticks.map((c,i)=><i key={i} style={{transitionDelay:`${Math.min(i*6,240)}ms`}} className={`min-w-px flex-1 rounded-[1px] transition-[height,background-color] duration-4 ease-out-tk motion-reduce:transition-none ${tick[c]??tick.l}`}/>)}
 </div>;
}
/** Lines to draw before the lyrics are known. */
const MAP_FALLBACK=36;
function CurrentSong({np,mine,job,lines,current,busy,onRelearn}:{np:NowPlaying;mine:SavedSync|null;job:ActiveSync|null;lines:number;current:{title:string;detail:string;phase:SyncStatus["phase"]};busy:boolean;onRelearn:()=>void}){
 const art=useArt(np.art_id??null);
 // A saved song being relearned shows the new listen, not the old check.
 const saved=current.phase==="saved"&&!job;
 const check=saved?mine?.check??null:null;
 const count=check?check.lines.length:lines||MAP_FALLBACK;
 const heard=job?.phase==="processing"||current.phase==="processing"?1:job?.phase==="learning"?(job.progress??0)/100:saved?1:0;
 const figure=check?{value:`${onTimePercent(check)}%`,label:"words on time",title:"Estimated share of words that land within 0.3s of the singing"}
  :job?.phase==="learning"?{value:`${job.progress??0}%`,label:"heard",title:"How much of this listen has been captured"}
  :null;
 const note=check?checkSummary(check)
  :job&&current.phase==="saved"?"Relearning on this listen. Your saved timing stays until it finishes."
  :!saved?current.detail
  :mine?.refresh?"Relearns on your next full listen. This timing stays until then."
  :mine?.relearns?"Saved before syncs were checked. Your next full listen checks it."
  :"Word timing saved on this device.";
 return <section aria-label="Current song sync" className="mx-2 mb-3 rounded-lg bg-fg/[0.04] p-3">
  <div className="flex items-center gap-2.5">
   <Cover thumbKey={mine?.thumb_key??null} url={art}/>
   <div className="min-w-0 flex-1"><div className="truncate text-sm font-medium">{np.title}</div><div className="truncate text-xs text-muted">{np.artist}</div></div>
   {figure&&<div className="shrink-0 text-right" title={figure.title}><div className="text-[22px] font-semibold leading-none tracking-[-0.01em] tabular-nums">{figure.value}</div><div className="mt-1 text-[10.5px] text-muted">{figure.label}</div></div>}
  </div>
  <div className="mt-3"><SongMap mode={check?{kind:"check",check}:{kind:"heard",count,share:heard}}/></div>
  {!saved&&<div className="mt-2.5 flex items-center gap-1.5 text-xs font-medium"><MorphIcon name={SYNC_GLYPHS[job?.phase??current.phase]} size={15} dur={DUR[5]}/><span>{job&&current.phase==="saved"?"Relearning timing":current.title}</span></div>}
  <p className={`${saved?"mt-2.5":"mt-1"} text-[11.5px] leading-[17px] ${saved?"text-fg/80":"text-muted"}`}>{note}</p>
  {saved&&mine&&!mine.refresh&&<button className={`${actionStyle} -mb-1 -ml-2 mt-1.5`} disabled={busy} title="Learn this song again on your next full listen" onClick={onRelearn}><Glyph kind="refresh" size={14}/>Relearn</button>}
 </section>;
}
/** `top` is where the panel opens, read by the trigger before the click's
 * render, so sizing the panel never forces a layout on the opening frame. */
export function SavedSyncs({np,lines,onClose,current,top}:{np:NowPlaying;lines:number;onClose:(restoreFocus?:boolean)=>void;current:{title:string;detail:string;phase:SyncStatus["phase"]};top?:number}){
 const {jobs, unavailable}=useActiveSyncs();
 const [rows,setRows]=useState<SavedSync[]|null>(lastRows),[error,setError]=useState(""),[busy,setBusy]=useState<string|null>(null),[notice,setNotice]=useState("");
 const [undo,setUndo]=useState<{key:string;revision:number}|null>(null),[sort,setSort]=useState<SyncSort>("recent"),[allRows,setAllRows]=useState(false);const alive=useRef(true),serial=useRef(0),panel=useRef<HTMLDivElement>(null);
 useThumbs();
 // Opening may join the read a hover already started; later refreshes
 // (after an action or a new save) always read again.
 const refresh=useCallback(async(join=false)=>{const n=++serial.current;try{const data=await (join&&rowsInFlight?rowsInFlight:readRows());if(alive.current&&n===serial.current){setRows(data);setError("");}}catch{if(alive.current&&n===serial.current)setError("Couldn’t load saved syncs.");}},[]);
 useEffect(()=>{alive.current=true;void refresh(true);const a=onSyncLibraryChanged(()=>void refresh()),b=onKaraokeReady(()=>void refresh());return ()=>{alive.current=false;serial.current++;a();b();};},[refresh]);
 useEffect(()=>{const outside=(e:PointerEvent)=>{if(!panel.current?.parentElement?.contains(e.target as Node))onClose(false);};const escape=(e:KeyboardEvent)=>{if(e.key==="Escape"){e.preventDefault();e.stopPropagation();onClose();}};document.addEventListener("pointerdown",outside);document.addEventListener("keydown",escape,true);return ()=>{document.removeEventListener("pointerdown",outside);document.removeEventListener("keydown",escape,true);};},[onClose]);
 async function act(key:string,action:"refresh"|"delete"|"undo",revision?:number){if(busy)return;setBusy(key);setError("");try{const rev=await savedSyncAction(key,action,revision);if(!alive.current)return;if(action==="delete"){setUndo({key,revision:rev});setNotice("Timing deleted.");}else{setUndo(null);setNotice(action==="refresh"?"Will relearn on your next full listen. Current timing stays available.":"Timing restored.");}await refresh();}catch(e){if(alive.current)setError(typeof e==="string"?e:"Couldn’t update this sync.");}finally{if(alive.current)setBusy(null);}}
 // Rows are memoized, so the once-a-second sync poll doesn't redraw a long
 // library: they get one stable act that calls the latest.
 const actNow=useRef(act);actNow.current=act;
 const rowAct=useCallback((key:string,action:"refresh"|"delete")=>actNow.current(key,action),[]);
 // Reach down to the window's bottom edge, over the transport while open
 // (the window never resizes; the old 65vh cap left the expanded widget's
 // lower third unused). Focus mode's tall room stops at PANEL_MAX.
 // Written straight to the element: as state it re-rendered every row
 // once more before the first paint. Measuring the panel here would force a
 // style and layout pass inside the click, so the first fit uses `top`.
 useLayoutEffect(()=>{const el=panel.current;if(!el)return;const fit=(at:number)=>{el.style.maxHeight=`${Math.max(0,Math.min(PANEL_MAX,Math.floor(window.innerHeight-at-8)))}px`;};fit(top??el.getBoundingClientRect().top);const resize=()=>fit(el.getBoundingClientRect().top);window.addEventListener("resize",resize);return ()=>window.removeEventListener("resize",resize);
 // eslint-disable-next-line react-hooks/exhaustive-deps
 },[]);
 // The click paints the panel with the rows that fit on screen. Focus moves
 // in (two style passes over the whole window, for :focus-visible) and the
 // rest of a long library mounts after that frame, the rows as
 // interruptible work, so the panel opens as fast as any other button.
 useEffect(()=>{let t=0;const f=requestAnimationFrame(()=>{t=window.setTimeout(()=>{panel.current?.focus({preventScroll:true});startTransition(()=>setAllRows(true));});});return ()=>{cancelAnimationFrame(f);window.clearTimeout(t);};},[]);
 const mine=rows?.find(r=>r.title===np.title&&r.artist===np.artist)??null;
 const isMine=(j:{title:string;artist:string})=>j.title===np.title&&j.artist===np.artist;
 const myJob=jobs.find(isMine)??null,others=jobs.filter(j=>!isMine(j));
 const sorted=rows?sortSyncs(rows,sort):[],list=allRows?sorted:sorted.slice(0,FIRST_ROWS);
 const checked=rows?.filter(r=>r.check).length??0;
 const btn="grid h-7 w-7 shrink-0 place-items-center rounded-md text-fg/65 transition-[color,background-color,transform] duration-2 ease-out-tk hover:bg-fg/10 hover:text-fg active:scale-95 motion-reduce:transition-none focus-visible:outline focus-visible:outline-2 focus-visible:outline-fg/50 disabled:opacity-40";
 const seg=(on:boolean)=>`rounded-[5px] px-[7px] py-[3px] text-[11px] font-medium normal-case tracking-normal transition-colors duration-2 ease-out-tk focus-visible:outline focus-visible:outline-2 focus-visible:outline-fg/50 ${on?"bg-fg/10 text-fg":"text-muted hover:text-fg"}`;
 // .pop-in like the right-click menu: 0.97 to 1 with the fade, grown from
 // the button above its right edge so it reads as coming from there.
 return <div ref={panel} role="dialog" aria-label="Lyric syncs" data-sync-library tabIndex={-1} onMouseDown={e=>e.stopPropagation()} onKeyDown={e=>{if(e.key==="Escape"){e.stopPropagation();onClose();}}} className="pop-in absolute right-0 top-full z-50 mt-2 flex origin-top-right max-h-[min(320px,65vh)] w-[312px] max-w-[calc(100vw-32px)] flex-col overflow-hidden rounded-xl border border-border/15 bg-surface text-fg shadow-xl shadow-black/40 outline-none">
  <div className="flex shrink-0 items-center justify-between px-3 pb-2 pt-3"><div className="text-sm font-medium">Lyric syncs</div><button className={btn} aria-label="Close lyric syncs" onClick={()=>onClose()}><Glyph kind="close"/></button></div>
  <div className="sync-scroll min-h-0 overflow-y-auto overscroll-contain px-1 pb-2">
    <CurrentSong np={np} mine={mine} job={myJob} lines={lines} current={current} busy={!!busy} onRelearn={()=>{if(mine)void act(mine.key,"refresh");}}/>
    {others.length>0 && <section aria-label="Also syncing" className="mx-2 mb-3"><h3 className="px-0.5 pb-0.5 text-[10px] font-medium uppercase tracking-wider text-muted">Also syncing</h3>{others.map(job=><SyncingRow key={job.key} job={job}/>)}</section>}
    {unavailable && <p className="px-2 pb-2 text-xs text-muted">Sync activity is temporarily unavailable.</p>}
    <div className="flex items-center justify-between pb-1 pl-2 pr-1 pt-1 text-[10px] font-medium uppercase tracking-wider text-muted"><span>Saved <span className="ml-1 tabular-nums">{rows?.length ?? ""}</span>{!!rows?.length&&<span className="ml-1 normal-case tracking-normal">· {checked} checked</span>}</span>
      {!!rows?.length&&<div className="flex gap-0.5" role="group" aria-label="Sort saved syncs"><button className={seg(sort==="recent")} aria-pressed={sort==="recent"} onClick={()=>setSort("recent")}>Recent</button><button className={seg(sort==="least")} aria-pressed={sort==="least"} onClick={()=>setSort("least")}>Least sure</button></div>}
    </div>
    {rows===null&&!error?<p className="px-3 py-7 text-xs text-muted">Loading saved timing…</p>:rows?.length===0?<div className="px-3 py-7"><p className="text-sm">Your collection starts here.</p><p className="mt-1 text-xs leading-5 text-muted">Listen to a song with lyrics from start to finish. Saved word timing will appear here.</p></div>:list.map(song=><SavedRow key={song.key} song={song} thumb={song.thumb_key?thumbs.get(song.thumb_key)??null:null} busy={!!busy} btn={btn} act={rowAct}/>)}</div>
  {error&&<div role="alert" className="px-3 pb-3 text-xs text-muted">{error} <button className="underline" onClick={()=>void refresh()}>Retry</button></div>}
  {notice&&<div role="status" className="border-t border-border/10 px-3 py-2 text-xs leading-5 text-muted">{notice}{undo&&<button className="ml-2 text-fg underline" disabled={!!busy} onClick={()=>void act(undo.key,"undo",undo.revision)}>Undo</button>}</div>}
  {/* Research capture is a tool, not part of the song's story: it rides the footer. */}
  <div className="shrink-0 border-t border-border/10 px-2 pb-1.5 pt-0.5"><ResearchRecording np={np}/></div>
 </div>;
}
/** A row's song at a glance: the card's ticks, small and quiet. Long songs
 * fold into ROW_TICKS stretches, each drawn as its least sure line. */
const ROW_TICKS=14;
function RowMap({lines}:{lines:string}){
 const rank="nfslu",n=Math.min(lines.length,ROW_TICKS);
 const ticks=Array.from({length:n},(_,i)=>[...lines.slice(Math.floor(i*lines.length/n),Math.floor((i+1)*lines.length/n))].reduce((a,c)=>rank.indexOf(c)>rank.indexOf(a)?c:a,"n"));
 const tick:{[k:string]:string}={f:"h-[3px] bg-fg/40",s:"h-[3px] bg-fg/20",l:"h-[7px] bg-fg/40",u:"h-3 bg-fg/85",n:"h-[3px] bg-fg/[0.08]"};
 return <div aria-hidden className="flex h-3 w-10 shrink-0 items-end gap-px">{ticks.map((c,i)=><i key={i} className={`min-w-0 flex-1 rounded-[0.5px] ${tick[c]??tick.l}`}/>)}</div>;
}
/** The estimate rests at the row's end; hover or focus swaps it for the
 * row's actions in the same seat, so neither shifts the title. */
const SavedRow=memo(function SavedRow({song,thumb,busy,btn,act}:{song:SavedSync;thumb:string|null;busy:boolean;btn:string;act:(key:string,action:"refresh"|"delete")=>Promise<void>}){
 const when=song.relearns?song.check?"relearns next listen":"not checked yet":savedWhen(song.saved_at);
 const swap="transition-opacity duration-2 ease-out-tk motion-reduce:transition-none";
 // Rows off screen skip layout and paint until scrolled to (a long library
 // is hundreds of rows, all mounted on open).
 return <div className="group/sync flex min-h-14 items-center [contain-intrinsic-size:auto_56px] [content-visibility:auto] gap-2.5 rounded-lg px-2 py-1.5 hover:bg-fg/5">
  <Cover thumbKey={null} url={thumb}/>
  <div className="min-w-0 flex-1"><div className="truncate text-sm font-medium">{song.title}</div><div className="truncate text-xs text-muted">{song.refresh?"Relearns on your next full listen":`${song.artist} · ${when}`}</div></div>
  {song.check&&<RowMap lines={song.check.lines}/>}
  <div className="relative flex shrink-0 items-center justify-end">
   <span aria-hidden={!song.check} title={song.check?"Estimated words on time":"Not checked yet"} className={`pointer-events-none absolute right-1 text-xs tabular-nums ${song.check?"text-fg/85":"text-muted"} ${swap} group-hover/sync:opacity-0 group-focus-within/sync:opacity-0`}>{song.check?`${onTimePercent(song.check)}%`:"—"}</span>
   <div className={`flex opacity-0 ${swap} group-hover/sync:opacity-100 focus-within:opacity-100`}><button className={btn} disabled={busy} title="Relearn on next full listen" aria-label={`Relearn timing for ${song.title}`} onClick={()=>void act(song.key,"refresh")}><Glyph kind="refresh"/></button><button className={btn} disabled={busy} title="Delete timing" aria-label={`Delete timing for ${song.title}`} onClick={()=>void act(song.key,"delete")}><Glyph kind="delete"/></button></div>
  </div>
 </div>;
});
