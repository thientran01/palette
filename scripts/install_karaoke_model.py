"""Install already-exported, verified local karaoke assets; no network access.

MMS_FA weights are CC-BY-NC 4.0. This installer does not distribute weights.
Obtain/export them with scripts/research/karaoke_mms_export.py for local use.
Close Palette before installing; model selection is fixed for one app session.
"""
import argparse,hashlib,os,shutil,tempfile
from pathlib import Path
ASSETS={
    'mms-fa-int8.onnx':'50128ba8db1150101b9e7d3610cdfda5c8fd2637b918d151ff6aa6fde2b9c2de',
    'onnxruntime.dll':'c7151fd9844ad7c7d18525f1177e9ef62d91e4a6ac3583d0be700554a2b2b1d6',
}
def digest(path):
    with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--model',type=Path,required=True)
    p.add_argument('--runtime',type=Path,required=True)
    p.add_argument('--destination',type=Path,required=True)
    a=p.parse_args()
    sources={'mms-fa-int8.onnx':a.model,'onnxruntime.dll':a.runtime}
    for name,source in sources.items():
        if digest(source)!=ASSETS[name]:raise ValueError(f'Unrecognized asset: {source}')
    a.destination.mkdir(parents=True,exist_ok=True)
    for name,source in sources.items():
        target=a.destination/name
        if target.is_file() and digest(target)==ASSETS[name]:continue
        fd,tmp=tempfile.mkstemp(prefix=name+'.',suffix='.installing',dir=a.destination)
        os.close(fd)
        try:
            shutil.copyfile(source,tmp)
            if digest(Path(tmp))!=ASSETS[name]:raise ValueError('Copy checksum mismatch')
            os.replace(tmp,target)
        finally:
            Path(tmp).unlink(missing_ok=True)
    (a.destination/'NOTICE.txt').write_text(
        'MMS_FA model: Meta, CC-BY-NC 4.0. Local research/personal use.\n'
        'https://docs.pytorch.org/audio/main/generated/torchaudio.pipelines.MMS_FA.html\n'
        'ONNX Runtime 1.22.1: Microsoft, MIT.\n'
        'https://github.com/microsoft/onnxruntime/blob/v1.22.1/LICENSE\n',encoding='utf-8')
    (a.destination/'enabled').write_text('mms-int8/1\n',encoding='utf-8')
    print(f'Installed verified local karaoke assets in {a.destination}. Restart Palette.')
if __name__=='__main__':main()
