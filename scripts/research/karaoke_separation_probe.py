"""Offline vocal-separation experiment on selected lyric windows.
Uses torchaudio 2.8 HDEMUCS_HIGH_MUSDB; downloads official weights into TORCH_HOME.
Input is already mono 16kHz: upsampling cannot recover discarded stereo detail.
Output preserves sample count/time map; do not use it as a new capture or app cache.
"""
import argparse,json,time,shutil,sys
from pathlib import Path
import numpy as np
import torch,torchaudio
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from karaoke_tap import parse_lrc
from research.karaoke_probe_safety import validate_disjoint_windows
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('dump',type=Path);p.add_argument('out',type=Path)
p.add_argument('--lines',type=int,nargs='+',required=True)
a=p.parse_args()
if a.dump.resolve()==a.out.resolve():raise ValueError('Output must differ from source')
torch.set_num_threads(2)
torch.set_num_interop_threads(1)
meta=json.loads((a.dump/'meta.json').read_text(encoding='utf-8'));m=meta['map']
pcm=np.fromfile(a.dump/'pcm.i16',dtype='<i2').astype(np.float32)/32768
sample=lambda ms:max(0,min(len(pcm),round((ms-m['intercept_ms'])/m['slope_ms'])))
lines=parse_lrc((a.dump/'lyrics.lrc').read_text(encoding='utf-8'))
windows=[]
for i,(seconds,_) in enumerate(lines):
 stamp=round(seconds*1000)
 if stamp not in a.lines:continue
 next_ms=lines[i+1][0]*1000 if i+1<len(lines) else m['intercept_ms']+len(pcm)*m['slope_ms']
 windows.append((sample(stamp-500),sample(min(next_ms+500,stamp+20000))))
validate_disjoint_windows(windows)
if a.out.exists():raise FileExistsError('Choose a new output fixture directory')
bundle=torchaudio.pipelines.HDEMUCS_HIGH_MUSDB
model=bundle.get_model().eval()
index=model.sources.index('vocals')
result=pcm.copy();measurements=[]
for i,(seconds,text) in enumerate(lines):
 stamp=round(seconds*1000)
 if stamp not in a.lines:continue
 next_ms=lines[i+1][0]*1000 if i+1<len(lines) else m['intercept_ms']+len(pcm)*m['slope_ms']
 # Two seconds of context beyond the aligner's half-second margin.
 start=sample(stamp-2500);end=sample(min(next_ms+2500,stamp+22500))
 wave=torch.from_numpy(pcm[start:end]).unsqueeze(0)
 wave=torchaudio.functional.resample(wave,16000,bundle.sample_rate).repeat(2,1).unsqueeze(0)
 started=time.perf_counter()
 with torch.inference_mode():
  vocals=model(wave)[0,index].mean(dim=0,keepdim=True)
  vocals=torchaudio.functional.resample(vocals,bundle.sample_rate,16000)[0].numpy()[:end-start]
 if len(vocals)!=end-start:raise ValueError('Separation changed sample count')
 gain=min(1.0,.95/max(float(np.max(np.abs(vocals))),1e-8))
 # Replace only the alignment window; contextual boundaries stay outside it.
 lo=sample(stamp-500);hi=sample(min(next_ms+500,stamp+20000))
 result[lo:hi]=vocals[lo-start:hi-start]*gain
 measurements.append({'line_t':stamp,'seconds':time.perf_counter()-started,'gain':gain})
 print(measurements[-1],flush=True)
if len(measurements)!=len(set(a.lines)):raise ValueError('Requested rows missing')
a.out.mkdir(parents=True,exist_ok=False)
for name in ['meta.json','lyrics.lrc']:shutil.copy2(a.dump/name,a.out/name)
(result*32768).clip(-32768,32767).astype('<i2').tofile(a.out/'pcm.i16')
(a.out/'separation.json').write_text(json.dumps({'model':'HDEMUCS_HIGH_MUSDB','input':'16k mono upsampled to 44.1k duplicated stereo','rows':measurements},indent=2),encoding='utf-8')