"""Install already-exported, verified local karaoke assets; no network access.

MMS_FA weights are CC-BY-NC 4.0. This installer does not distribute weights.
Obtain/export them with scripts/research/karaoke_mms_export.py for local use.
Close Palette before installing; model selection is fixed for one app session.
--vocal-model optionally adds Kim_Vocal_2.onnx (UVR's public model repository)
for the development-only vocal trial (PALETTE_VOCAL_PREVIEW=1).
--japanese-dictionary optionally adds OpenJTalk's naist-jdic (BSD) so Japanese
songs' kanji are aligned with their readings: pass naist-jdic-jpreprocess.tar.gz
from https://github.com/jpreprocess/jpreprocess/releases/tag/v0.15.0
--model and --runtime may be left out when the destination already holds them.
"""
import argparse,hashlib,os,shutil,tarfile,tempfile
from pathlib import Path
ASSETS={
    'mms-fa-int8.onnx':'50128ba8db1150101b9e7d3610cdfda5c8fd2637b918d151ff6aa6fde2b9c2de',
    'onnxruntime.dll':'c7151fd9844ad7c7d18525f1177e9ef62d91e4a6ac3583d0be700554a2b2b1d6',
    'Kim_Vocal_2.onnx':'ce74ef3b6a6024ce44211a07be9cf8bc6d87728cc852a68ab34eb8e58cde9c8b',
}
# jpreprocess v0.15.0's prebuilt naist-jdic, the version acoustic::japanese pins.
DICTIONARY_ARCHIVE='8a930bbc57bf4adcf521d53544c7dc9ab8ab3aa997a591b1b1608dc5539017b8'
DICTIONARY_DIR='naist-jdic'
DICTIONARY={
    'char_def.bin':'cd3dbb6fa448906590c2e5ef2600378e4bd074c4d1d6fc5c049d4fd8c2d2d556',
    'dict.da':'26f14fe3affdb0966e1696ae6cd4b7faac91bf85b7ca57a5d8cdc045da58d043',
    'dict.vals':'7fb308e1909be247779be0a4c81070416635c4ff0ab266d48a5e964baffd345f',
    'dict.words':'7ef63438285a3dd50299aa826f3d62d9657a907184e055a179af3e00eae820aa',
    'dict.wordsidx':'67b30a728ab8816def37d9e2775d99dc98bf8eaa6f74a821dd3ee2fd009e8b61',
    'matrix.mtx':'a64b58ac89b2ca21f3ae6c56ca772f8d4488cc636f47b821b4cca9c0c67c95d2',
    'metadata.json':'8fa56e3d131c9d7cabc3633bb7c2add979469a1e22be90ecd714e81df20a5dd4',
    'unk.bin':'1ff6b44ab3145cd8ac731b9cb0500804feae5fa257aee8bc78a65a8dc80bb454',
}
def digest(path):
    with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def install_dictionary(archive,destination):
    """Unpack only the pinned files, verified, then swap the folder in whole."""
    if digest(archive)!=DICTIONARY_ARCHIVE:raise ValueError(f'Unrecognized dictionary: {archive}')
    target=destination/DICTIONARY_DIR
    if all((target/n).is_file() and digest(target/n)==h for n,h in DICTIONARY.items()):return
    staging=Path(tempfile.mkdtemp(prefix=DICTIONARY_DIR+'.',suffix='.installing',dir=destination))
    try:
        with tarfile.open(archive,'r:gz') as tar:
            for name,sha in DICTIONARY.items():
                member=tar.getmember(f'{DICTIONARY_DIR}/{name}')
                if not member.isfile():raise ValueError(f'Dictionary entry is not a file: {name}')
                with tar.extractfile(member) as src,(staging/name).open('wb') as dst:shutil.copyfileobj(src,dst)
                if digest(staging/name)!=sha:raise ValueError(f'Dictionary checksum mismatch: {name}')
        if target.exists():shutil.rmtree(target)
        os.replace(staging,target)
    finally:
        if staging.exists():shutil.rmtree(staging)
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--model',type=Path)
    p.add_argument('--runtime',type=Path)
    p.add_argument('--destination',type=Path,required=True)
    p.add_argument('--vocal-model',type=Path)
    p.add_argument('--japanese-dictionary',type=Path)
    a=p.parse_args()
    sources={}
    if a.model:sources['mms-fa-int8.onnx']=a.model
    if a.runtime:sources['onnxruntime.dll']=a.runtime
    if a.vocal_model:sources['Kim_Vocal_2.onnx']=a.vocal_model
    for name in ('mms-fa-int8.onnx','onnxruntime.dll'):
        if name not in sources:
            held=a.destination/name
            if not (held.is_file() and digest(held)==ASSETS[name]):
                p.error(f'--{"model" if name.startswith("mms") else "runtime"} is required: {held} is missing')
    for name,source in sources.items():
        if digest(source)!=ASSETS[name]:raise ValueError(f'Unrecognized asset: {source}')
    a.destination.mkdir(parents=True,exist_ok=True)
    if a.japanese_dictionary:install_dictionary(a.japanese_dictionary,a.destination)
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
        'https://github.com/microsoft/onnxruntime/blob/v1.22.1/LICENSE\n'
        'Kim_Vocal_2 (optional): UVR public models. Local personal use.\n'
        'https://github.com/TRvlvr/model_repo/releases/tag/all_public_uvr_models\n'
        'naist-jdic (optional): Nara Institute of Science and Technology, BSD-style licence,\n'
        'as built by jpreprocess v0.15.0 (BSD 3-clause).\n'
        'https://github.com/jpreprocess/jpreprocess/releases/tag/v0.15.0\n',encoding='utf-8')
    (a.destination/'enabled').write_text('mms-int8/1\n',encoding='utf-8')
    print(f'Installed verified local karaoke assets in {a.destination}. Restart Palette.')
if __name__=='__main__':main()
