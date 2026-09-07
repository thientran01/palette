import {useEffect, useRef, useState} from "react";
import {researchAction, researchStatus, type ResearchStatus} from "./lib/backend";
import type {NowPlaying} from "./types";

const LABELS = {armed:"Ready to record",recording:"Recording for research",saving:"Saving recording…",saved:"Research recording saved",error:"Recording interrupted"};
const actionStyle = "inline-flex items-center gap-1.5 rounded-md px-2 py-1.5 text-[11px] font-medium text-fg/80 transition-colors duration-2 hover:bg-fg/10 hover:text-fg focus-visible:outline focus-visible:outline-2 focus-visible:outline-fg/50 disabled:opacity-40";
function RecordGlyph({active=false}:{active?:boolean}) {
  return <svg width="14" height="14" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" aria-hidden="true"><path d="M6 3.5H4a.5.5 0 0 0-.5.5v2M14 3.5h2a.5.5 0 0 1 .5.5v2M16.5 14v2a.5.5 0 0 1-.5.5h-2M6 16.5H4a.5.5 0 0 1-.5-.5v-2"/>{active?<rect x="7" y="7" width="6" height="6" rx="1" fill="currentColor" stroke="none"/>:<circle cx="10" cy="10" r="3"/>}</svg>;
}
export function ResearchRecording({np}:{np:NowPlaying}) {
  const [status,setStatus]=useState<ResearchStatus|null>(null);
  const [busy,setBusy]=useState(false),[error,setError]=useState("");
  const alive=useRef(false),pending=useRef(false),generation=useRef(0);
  useEffect(()=>{
    alive.current=true; let live=true,inFlight=false; let timer:number|undefined;
    const poll=async()=>{
      window.clearTimeout(timer);
      if(!live || document.hidden) return;
      if(inFlight||pending.current){timer=window.setTimeout(poll,1000);return;}
      inFlight=true;const n=generation.current;
      try { const s=await researchStatus(); if(live&&n===generation.current){setStatus(s);setError(e=>e==="Recording status is unavailable."?"":e);} }
      catch { if(live&&n===generation.current)setError("Recording status is unavailable."); }
      finally { inFlight=false;if(live&&!document.hidden)timer=window.setTimeout(poll,1000); }
    };
    void poll(); document.addEventListener("visibilitychange",poll);
    return ()=>{live=false;alive.current=false;generation.current++;window.clearTimeout(timer);document.removeEventListener("visibilitychange",poll);};
  },[]);
  async function act(action:"start"|"stop"|"cancel"|"open") {
    if(pending.current)return; pending.current=true;generation.current++;setBusy(true);setError("");
    try { await researchAction(action,np); const s=await researchStatus();if(alive.current)setStatus(s); }
    catch(e){if(alive.current)setError(e instanceof Error?e.message:typeof e==="string"?e:"Couldn’t update recording.");}
    finally{pending.current=false;if(alive.current)setBusy(false);}
  }
  const phase=status?.phase??"";
  const working=phase==="armed"||phase==="recording"||phase==="saving";
  const eligible=(np.player==="spotify"||np.player==="apple_music")&&np.duration_ms>0;
  const seconds=Math.floor((status?.elapsed_ms??0)/1000);
  return <div className="mt-2 rounded-lg bg-fg/[0.025] px-1.5 py-1" aria-label="Research recording">
    {phase && <div className="px-1 pb-1 pt-1" role="status">
      <div className="flex items-center gap-1.5 text-[11px] font-medium"><span className={phase==="recording"?"text-accent":"text-muted"}><RecordGlyph active={phase==="recording"}/></span>{LABELS[phase]}{phase==="recording"&&<span className="ml-auto tabular-nums text-muted">{Math.floor(seconds/60)}:{String(seconds%60).padStart(2,"0")}</span>}</div>
      {status?.title!==np.title && <p className="mt-1 truncate text-[11px] text-fg/80">{status?.title}</p>}
      <p className="mt-1 text-[11px] leading-4 text-muted">{status?.detail}</p>
      {phase==="recording"&&<div className="mt-2 h-0.5 overflow-hidden rounded bg-fg/10" role="progressbar" aria-label="Research audio captured" aria-valuemin={0} aria-valuemax={100} aria-valuenow={status?.progress??0}><div className="h-full origin-left bg-accent transition-transform duration-3 motion-reduce:transition-none" style={{transform:`scaleX(${(status?.progress??0)/100})`}}/></div>}
    </div>}
    <div className="flex flex-wrap items-center gap-0.5">
      {!working&&<button className={actionStyle} disabled={busy||!eligible||!status} onClick={()=>void act("start")}><RecordGlyph/>Record for research</button>}
      {phase==="recording"&&<button className={actionStyle} disabled={busy} onClick={()=>void act("stop")}><RecordGlyph active/>Stop &amp; save</button>}
      {(phase==="armed"||phase==="recording")&&<button className={actionStyle} disabled={busy} onClick={()=>void act("cancel")}>Cancel</button>}
      {!working&&<button className={`${actionStyle} ml-auto text-muted`} disabled={busy} onClick={()=>void act("open")}>Open folder</button>}
    </div>
    {!phase&&<p className="px-1 pb-1 text-[10px] leading-4 text-muted">Audio stays on this computer. Keeps the latest 5 recordings.</p>}
    {error&&<p role="alert" className="px-1 py-1 text-[11px] text-muted">{error}</p>}
  </div>;
}
