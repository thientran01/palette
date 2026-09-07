// Synthetic text with captured timing offsets. Uses the real lyric renderer.
import React from "react";
import {createRoot} from "react-dom/client";
import {LyricsPanel} from "../src/LyricsPanel";
import {attachWords, type LyricLine} from "../src/lib/lrc";
import * as clock from "../src/lib/posClock";
import "../src/index.css";
const lines:LyricLine[]=[{t:136770,text:"One two three four five six seven eight"},{t:139980,text:"The next line"}];
const words=lines[0].text.split(" ").map((text,i)=>({t:139718+i*80,end:139798+i*80,text:text+" ",line_t:136770}));
const old=[{...lines[0],words},lines[1]], guarded=attachWords(lines,words);
let seq=0;
function setClock(playing:boolean) {clock.ingest({seq:++seq,app_id:"preview",player:"spotify",title:"Timing fixture",artist:"",album:"",status:playing?"playing":"paused",position_ms:136770,position_at_ms:Date.now(),duration_ms:144000,can_seek:true,art_id:null});}
setClock(false);
function Preview(){return <main style={{padding:32,color:"rgb(var(--fg))",background:"rgb(var(--surface))",height:"100vh"}}><h1 className="text-lg">Missed entrance: original timing vs source fallback</h1><p className="mt-2 text-sm text-muted">Synthetic text; timing offsets from a saved alignment. Pause shows the exact source entrance.</p><div className="mt-4 flex gap-3"><button className="rounded bg-fg/10 px-3 py-2" onClick={()=>setClock(true)}>Replay</button><button className="rounded bg-fg/10 px-3 py-2" onClick={()=>setClock(false)}>Source entrance</button></div><div className="mt-6 flex gap-6">{[["Saved word timing",old],["Source fallback",guarded]].map(([label,rows])=><section key={label as string} className="w-[350px] rounded-xl border border-border/15 p-4"><h2 className="mb-4 text-sm text-muted">{label as string}</h2><div className="flex h-[200px] flex-col"><LyricsPanel lines={rows as LyricLine[]} seekable={false} leadMs={0}/></div></section>)}</div></main>;}
createRoot(document.getElementById("root")!).render(<Preview/>);
