"""Offline MMS forced-alignment experiment; never writes the app cache.
Inference uses only PCM and synced LRC. Human labels are loaded afterward.
Requires torch/torchaudio 2.8 CPU and uroman 1.3.1.1 in an isolated environment.
"""
import argparse, hashlib, json, re, sys, time
from pathlib import Path
import numpy as np
import torch
import torchaudio
import uroman

parser=argparse.ArgumentParser()
parser.add_argument('--repo',type=Path,required=True)
parser.add_argument('--out',type=Path,required=True)
parser.add_argument('--threads',type=int,default=2)
parser.add_argument('--onnx',type=Path,help='Use exported ONNX acoustics; tokenization/CTC remain identical')
parser.add_argument('dumps',nargs='+',type=Path)
a=parser.parse_args()
sys.path.insert(0,str(a.repo/'scripts'))
from karaoke_tap import parse_lrc,legacy_tokens

torch.set_num_threads(a.threads)
torch.set_num_interop_threads(1)
a.out.mkdir(parents=True,exist_ok=True)
bundle=torchaudio.pipelines.MMS_FA
print('Loading MMS_FA (CPU)',flush=True)
t0=time.perf_counter()
if a.onnx:
 import onnxruntime as ort
 options=ort.SessionOptions();options.intra_op_num_threads=a.threads;options.inter_op_num_threads=1
 model=ort.InferenceSession(str(a.onnx),sess_options=options,providers=['CPUExecutionProvider'])
else:
 model=bundle.get_model(dl_kwargs={"progress": False}).eval()
print('Model ready',round(time.perf_counter()-t0,2),'s',flush=True)
tokenizer=bundle.get_tokenizer(); aligner=bundle.get_aligner(); roman=uroman.Uroman()
for dump in a.dumps:
 meta=json.loads((dump/'meta.json').read_text(encoding='utf-8'))
 m=meta['map']; raw=(dump/'pcm.i16').read_bytes(); pcm=np.frombuffer(raw,dtype='<i2').astype(np.float32)/32768
 lines=parse_lrc((dump/'lyrics.lrc').read_text(encoding='utf-8'))
 pos=lambda sample:m['intercept_ms']+m['slope_ms']*sample
 sample=lambda ms:max(0,min(len(pcm),round((ms-m['intercept_ms'])/m['slope_ms'])))
 predictions=[]; started=time.perf_counter()
 for li,(sec,text) in enumerate(lines):
  words=legacy_tokens(text)
  if not words:continue
  norm=[re.sub("[^a-z']",'',roman.romanize_string(w).lower()) for w in words]
  if any(not w for w in norm):raise ValueError(('unrepresentable token',li,words,norm))
  begin=sample(sec*1000-500)
  next_ms=lines[li+1][0]*1000 if li+1<len(lines) else pos(len(pcm))
  end=sample(min(next_ms+500,sec*1000+20000))
  if end-begin<400:raise ValueError(('empty audio window',li))
  with torch.inference_mode():
   if a.onnx:
    emissions=torch.from_numpy(model.run(None,{'waveform':pcm[None,begin:end]})[0])
   else:
    emissions,_=model(torch.from_numpy(pcm[begin:end]).unsqueeze(0))
   spans=aligner(emissions[0],tokenizer(['*']+norm+['*']))[1:-1]
  ratio=(end-begin)/emissions.shape[1]
  for text,normalized,chars in zip(words,norm,spans):
   count=sum(c.end-c.start for c in chars)
   predictions.append({'text':text,'line_index':li,'line_t':round(sec*1000),
    't':round(pos(begin+chars[0].start*ratio)),
    'end':round(pos(begin+chars[-1].end*ratio)),
    'confidence':sum(c.score*(c.end-c.start) for c in chars)/max(count,1),
    'normalized':normalized})
  print(dump.name,'line',li+1,'/',len(lines),'elapsed',round(time.perf_counter()-started,1),'s',flush=True)
 # Score only after predictions exist, preventing truth from influencing inference.
 label_bytes=(dump/'labels.txt').read_bytes(); label_text=label_bytes.decode('utf-8-sig')
 labels=[r.split('\t') for r in label_text.splitlines() if r.strip() and not r.startswith('#')]
 if len(labels)>len(predictions):raise ValueError('more labels than predictions')
 song_clock='# clock: song' in label_text.splitlines()
 deltas=[]; grouped={}
 for i,row in enumerate(labels):
  w=predictions[i]
  if len(row)!=3 or row[2].strip()!=w['text'].strip():raise ValueError(('label identity',i,row,w))
  truth=float(row[0])*1000 if song_clock else pos(float(row[0])*16000)
  delta=w['t']-truth; deltas.append(delta); grouped.setdefault(w['line_index'],[]).append(abs(delta))
 absd=sorted(abs(x) for x in deltas); n=len(deltas)
 pct=lambda q:absd[int((n-1)*q+0.5)]
 result={'model':a.onnx.name if a.onnx else 'torchaudio-2.8-MMS_FA','preprocessing':'full mix, LRC row +/-500ms, max20s, uroman per source token, star boundaries',
  'threads':a.threads,'seconds':time.perf_counter()-started,'audio_seconds':len(pcm)/16000,
  'pcm_sha256':hashlib.sha256(raw).hexdigest(),'labels_sha256':hashlib.sha256(label_bytes).hexdigest(),
  'metrics':{'tokens':n,'median_ms':pct(0.5),'p90_ms':pct(0.9),'bias_ms':sum(deltas)/n,
  'within100_percent':100*sum(x<=100 for x in absd)/n,'worst_line_ms':max(sum(v)/len(v) for v in grouped.values())},'words':predictions}
 path=a.out/(dump.name+'.json');path.write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
 print('RESULT',dump.name,json.dumps(result['metrics']), 'seconds',round(result['seconds'],2), 'output',path,flush=True)
