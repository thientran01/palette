"""Build a tap-timing page for one karaoke evidence dump.

Hand-marking word onsets in Audacity took ~5 minutes per 10 seconds of
song (2026-09-04). Tapping along beats it: the page plays the dump slowed
down, shows the next token, and Space stamps the current song time
(minus a calibrated reaction delay). Download gives an Audacity-format
label file the scorer reads.

    python scripts/karaoke_tap.py <dump-dir> [out-dir]

<dump-dir> is app-data/karaoke-dumps/<key>/ (needs pcm.i16, lyrics.lrc,
meta.json and labels.template.txt — run
`cargo run --example karaoke_score -- template <dump-dir>` first).
Writes <out-dir>/song.wav and <out-dir>/tap.html (default out-dir: the
dump dir). Open tap.html in a browser; the audio is embedded so the page
is self-contained (~8MB for four minutes).
"""

import base64
import json
import re
import sys
import wave
from pathlib import Path

PAGE = r"""<!doctype html><html><head><meta charset="utf-8"><title>Tap: __TITLE__</title>
<style>
body{margin:0;background:#151311;color:#f5efe6;font:16px/1.4 system-ui,sans-serif}
.wrap{max-width:900px;margin:0 auto;padding:24px}
.big{font-size:56px;font-weight:600;min-height:80px;margin:16px 0}
.next{color:#a1988c;font-size:22px;min-height:60px}
.line{color:#a1988c;margin-top:8px}
.bar{display:flex;gap:12px;align-items:center;flex-wrap:wrap;margin:16px 0}
button,select{font:inherit;padding:8px 14px;border-radius:8px;border:1px solid #444;background:#222;color:#f5efe6;cursor:pointer}
button.primary{background:#f5efe6;color:#151311;border-color:#f5efe6}
.done{color:#7fc47f}.hint{color:#a1988c;font-size:14px}
.prog{height:6px;background:#2a2622;border-radius:3px;overflow:hidden}.prog>i{display:block;height:100%;background:#f5efe6;width:0}
kbd{background:#2a2622;border:1px solid #444;border-radius:4px;padding:1px 6px}
</style></head><body><div class="wrap">
<h2 style="margin:0">Tap the words: __TITLE__</h2>
<p class="hint">Press <kbd>Space</kbd> the moment each word or syllable is SUNG. <kbd>Backspace</kbd> undoes the last tap and rewinds a little. <kbd>&larr;</kbd> goes back one line. Slower speed is fine and more accurate: stamps are in song time. Taps autosave; refreshing keeps them. Stop whenever you like and download; a partial file still scores.</p>
<div class="bar">
  <button id="cal" class="primary">1. Calibrate (tap 8 clicks)</button>
  <span id="calv" class="hint">reaction: not measured</span>
</div>
<div class="bar">
  <button id="start" class="primary">2. Start / Pause (or press P)</button>
  <label>Speed <select id="rate"><option value="0.5">0.5&times;</option><option value="0.65">0.65&times;</option><option value="0.8" selected>0.8&times;</option><option value="1">1&times;</option></select></label>
  <button id="prev">&#9664; Back a line (&larr;)</button>
  <button id="back">Redo this line</button>
  <button id="restart">Restart from the top</button>
  <span id="time" class="hint">0.00s</span>
</div>
<div class="prog"><i id="pi"></i></div>
<div class="line" id="line"></div>
<div class="big" id="cur"></div>
<div class="next" id="nxt"></div>
<div class="bar"><button id="dl" class="primary">3. Download labels.txt</button><span id="stat" class="hint"></span><label class="hint">Resume from a file: <input type="file" id="load" accept=".txt"></label></div>
<audio id="a" src="data:audio/wav;base64,__WAV__"></audio>
</div><script>
const TOK=__TOK__, LINES=__LINES__, KEY='tap-'+__KEY__;
const a=document.getElementById('a'), cur=document.getElementById('cur'), nxt=document.getElementById('nxt'), lineEl=document.getElementById('line'), stat=document.getElementById('stat'), timeEl=document.getElementById('time'), pi=document.getElementById('pi');
let i=0, stamps=[], reaction=0.12, done=false;
const LEAD=1.2;
function save(){try{localStorage.setItem(KEY,JSON.stringify(stamps));}catch(e){}}
function render(){
  save();
  const t=TOK[i];
  if(!t){cur.textContent='- end -';cur.className='big done';nxt.textContent='';done=true;a.pause();stat.textContent=stamps.length+' of '+TOK.length+' tapped';return;}
  lineEl.textContent='Line '+(t.line+1)+' of '+LINES.length+': '+LINES[t.line];
  cur.textContent=t.text;
  nxt.textContent=TOK.slice(i+1,i+7).map(x=>x.text).join('  ');
  stat.textContent=stamps.length+' of '+TOK.length+' tapped';
  pi.style.width=(100*i/TOK.length)+'%';
}
function stamp(){
  if(done||a.paused)return;
  stamps.push(Math.max(0,a.currentTime-reaction*a.playbackRate)); i++; render();
}
function undo(){
  if(!stamps.length)return;
  stamps.pop(); i--; done=false; cur.className='big';
  a.currentTime=Math.max(0,TOK[i].t-LEAD); render();
}
function rewindToLine(target){
  while(stamps.length&&TOK[i-1]&&TOK[i-1].line>=target){stamps.pop();i--;}
  done=false;cur.className='big';
  a.currentTime=Math.max(0,TOK[i].t-LEAD); render(); if(a.paused)a.play();
}
function redoLine(){ if(!TOK[i]&&!stamps.length)return; rewindToLine(TOK[Math.min(i,TOK.length-1)].line); }
function prevLine(){ if(!stamps.length)return; rewindToLine(Math.max(0,TOK[Math.min(i,TOK.length-1)].line-1)); }
function restart(){ if(stamps.length&&!confirm('Wipe all '+stamps.length+' taps and start over?'))return; stamps=[];i=0;done=false;cur.className='big';a.pause();a.currentTime=Math.max(0,TOK[0].t-LEAD);render(); }
document.getElementById('rate').onchange=e=>{a.playbackRate=+e.target.value};
a.playbackRate=0.8;
document.getElementById('start').onclick=()=>{ if(a.paused){ if(i===0&&!stamps.length)a.currentTime=Math.max(0,TOK[0].t-LEAD); a.play(); } else a.pause(); };
document.getElementById('back').onclick=redoLine;
document.getElementById('prev').onclick=prevLine;
document.getElementById('restart').onclick=restart;
document.getElementById('load').onchange=e=>{const f=e.target.files[0];if(!f)return;const r=new FileReader();r.onload=()=>{const rows=String(r.result).split(String.fromCharCode(10)).filter(x=>x.trim());const st=rows.map(x=>parseFloat(x.split(String.fromCharCode(9))[0])).filter(x=>!isNaN(x));if(st.length&&st.length<=TOK.length){stamps=st;i=st.length;done=false;cur.className='big';a.currentTime=Math.max(0,TOK[Math.min(i,TOK.length-1)].t-LEAD);render();}};r.readAsText(f);};
document.addEventListener('keydown',e=>{
  if(e.code==='Space'){e.preventDefault();stamp();}
  else if(e.code==='Backspace'){e.preventDefault();undo();}
  else if(e.code==='ArrowLeft'){e.preventDefault();prevLine();}
  else if(e.key==='p'||e.key==='P'){document.getElementById('start').click();}
});
setInterval(()=>{timeEl.textContent=a.currentTime.toFixed(2)+'s'},100);
document.getElementById('cal').onclick=async()=>{
  const ctx=new (window.AudioContext||window.webkitAudioContext)();
  const clicks=[], taps=[]; const t0=ctx.currentTime+0.5;
  for(let k=0;k<8;k++){const o=ctx.createOscillator();const g=ctx.createGain();o.frequency.value=1000;g.gain.value=0.3;o.connect(g).connect(ctx.destination);o.start(t0+k);o.stop(t0+k+0.04);clicks.push(t0+k);}
  const calv=document.getElementById('calv'); calv.textContent='tap along with each click...';
  const h=e=>{ if(e.code==='Space'){e.preventDefault(); taps.push(ctx.currentTime);} };
  document.addEventListener('keydown',h,true);
  await new Promise(r=>setTimeout(r,9500));
  document.removeEventListener('keydown',h,true);
  const d=[]; for(const c of clicks){ const near=taps.filter(t=>t>c-0.3&&t<c+0.6); if(near.length)d.push(near[0]-c); }
  if(d.length>=4){ reaction=d.reduce((x,y)=>x+y,0)/d.length; calv.textContent='reaction: '+Math.round(reaction*1000)+' ms (subtracted from every tap)'; }
  else calv.textContent='not enough taps caught - try again';
};
document.getElementById('dl').onclick=()=>{
  const rows=stamps.map((s,k)=>s.toFixed(3)+String.fromCharCode(9)+s.toFixed(3)+String.fromCharCode(9)+TOK[k].text).join(String.fromCharCode(10))+String.fromCharCode(10);
  const b=new Blob([rows],{type:'text/plain'}); const u=URL.createObjectURL(b);
  const l=document.createElement('a'); l.href=u; l.download='labels.txt'; document.body.appendChild(l); l.click(); l.remove();
};
try{const sv=JSON.parse(localStorage.getItem(KEY)||'[]');if(Array.isArray(sv)&&sv.length&&sv.length<=TOK.length){stamps=sv;i=sv.length;}}catch(e){}
render();
</script></body></html>"""


def parse_lrc(text):
    stamps = []
    for line in text.splitlines():
        m = re.match(r"^((?:\[\d+:\d+(?:\.\d+)?\])+)(.*)$", line.strip())
        if not m or not m.group(2).strip():
            continue
        for mm, ss in re.findall(r"\[(\d+):(\d+(?:\.\d+)?)\]", m.group(1)):
            stamps.append((int(mm) * 60 + float(ss), m.group(2).strip()))
    stamps.sort()
    return stamps


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    dump = Path(sys.argv[1])
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else dump
    out.mkdir(parents=True, exist_ok=True)
    template = dump / "labels.template.txt"
    if not template.is_file():
        sys.exit(f"missing {template} — run: cargo run --example karaoke_score -- template {dump}")
    meta = json.loads((dump / "meta.json").read_text(encoding="utf-8"))
    title = f"{meta.get('artist', '')} — {meta.get('title', '')}".strip(" —")

    raw = (dump / "pcm.i16").read_bytes()
    wav_path = out / "song.wav"
    with wave.open(str(wav_path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(16000)
        w.writeframes(raw)

    rows = [l.split("\t") for l in template.read_text(encoding="utf-8").splitlines() if l.strip()]
    tokens = [{"t": float(r[0]), "text": r[2]} for r in rows]
    stamps = parse_lrc((dump / "lyrics.lrc").read_text(encoding="utf-8"))
    for tok in tokens:
        li = 0
        for i, (t, _) in enumerate(stamps):
            if t <= tok["t"]:
                li = i
        tok["line"] = li
    lines = [s[1] for s in stamps]

    html = (
        PAGE.replace("__TITLE__", title)
        .replace("__WAV__", base64.b64encode(wav_path.read_bytes()).decode())
        .replace("__TOK__", json.dumps(tokens, ensure_ascii=False))
        .replace("__LINES__", json.dumps(lines, ensure_ascii=False))
        .replace("__KEY__", json.dumps(dump.name))
    )
    page = out / "tap.html"
    page.write_text(html, encoding="utf-8")
    print(f"{title}: {len(tokens)} tokens over {len(lines)} lines")
    print(f"  {wav_path}")
    print(f"  {page}  <- open this in a browser")


if __name__ == "__main__":
    main()
