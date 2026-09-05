"""Export the offline MMS_FA candidate to dynamic-length ONNX and int8.
Requires torch/torchaudio 2.8 CPU, onnx 1.18, onnxruntime 1.22.1.
MMS_FA weights are CC-BY-NC 4.0; this research script does not bundle them.
Set TORCH_HOME to an isolated model cache before running. Float parity is
checked against the original bundle; quantized word accuracy must still be
measured separately with karaoke_mms_probe.py and the Rust scorer.
"""
from pathlib import Path
import argparse,json,time
import numpy as np
import torch,torchaudio,onnxruntime as ort
from onnxruntime.quantization import quantize_dynamic,QuantType
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--out',type=Path,required=True)
parser.add_argument('--pcm',type=Path,required=True,help='16 kHz mono signed int16 little-endian PCM for parity checks')
a=parser.parse_args()
root=a.out;root.mkdir(parents=True,exist_ok=True)
torch.set_num_threads(2)
model=torchaudio.pipelines.MMS_FA.get_model(dl_kwargs={'progress':False}).eval()
class Acoustic(torch.nn.Module):
 def __init__(self,model):super().__init__();self.model=model
 def forward(self,waveform):
  # Equivalent to bundle waveform layer_norm for batch size one, expressed
  # with reductions so ONNX does not require a constant waveform length.
  normalized=(waveform-waveform.mean(dim=-1,keepdim=True))*torch.rsqrt(waveform.var(dim=-1,keepdim=True,unbiased=False)+1e-5)
  logits=self.model.model(normalized)[0]
  emission=torch.log_softmax(logits,dim=-1)
  return torch.cat([emission,torch.zeros_like(emission[:,:,:1])],dim=-1)
w=Acoustic(model).eval();start=time.perf_counter()
print('Exporting float ONNX',flush=True)
with torch.inference_mode():
 torch.onnx.export(w,(torch.randn(1,64000),),str(root/'mms-fa.onnx'),input_names=['waveform'],output_names=['log_probs'],dynamic_axes={'waveform':{1:'samples'},'log_probs':{1:'frames'}},opset_version=17,dynamo=False)
print('Float export seconds',time.perf_counter()-start,flush=True)
options=ort.SessionOptions();options.intra_op_num_threads=2;options.inter_op_num_threads=1
session=ort.InferenceSession(str(root/'mms-fa.onnx'),sess_options=options,providers=['CPUExecutionProvider'])
raw=np.fromfile(a.pcm,dtype='<i2').astype(np.float32)/32768
if len(raw)<248000:raise ValueError('Parity PCM must contain at least 248000 samples')
for length in [32000,64123,128000]:
 data=raw[120000:120000+length][None,:]
 with torch.inference_mode():reference=model(torch.from_numpy(data))[0].numpy()
 actual=session.run(None,{'waveform':data})[0]
 err=float(np.max(np.abs(actual-reference)))
 print('Float parity',length,reference.shape,actual.shape,'maxlogdiff',err,flush=True)
 assert actual.shape==reference.shape and err<.02
session=None
print('Quantizing linear matrices to int8',flush=True)
quantize_dynamic(str(root/'mms-fa.onnx'),str(root/'mms-fa-int8.onnx'),per_channel=True,weight_type=QuantType.QInt8,op_types_to_quantize=['MatMul','Gemm'])
print('Model byte sizes',[(p.name,p.stat().st_size) for p in root.glob('*.onnx')],flush=True)
session=ort.InferenceSession(str(root/'mms-fa-int8.onnx'),sess_options=options,providers=['CPUExecutionProvider'])
data=raw[120000:248000][None,:]
actual=session.run(None,{'waveform':data})[0]
assert np.isfinite(actual).all()
print('Int8 output finite',bool(np.isfinite(actual).all()),'shape',actual.shape,flush=True)
(root/'vocab.json').write_text(json.dumps(torchaudio.pipelines.MMS_FA.get_dict()),encoding='utf-8')
print('Done seconds',time.perf_counter()-start,flush=True)
