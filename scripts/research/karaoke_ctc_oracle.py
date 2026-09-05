"""Generate synthetic CTC golden cases with torchaudio 2.8 (no song audio)."""
import argparse,json
from pathlib import Path
import torch,torchaudio
p=argparse.ArgumentParser();p.add_argument('out',type=Path);a=p.parse_args()
torch.manual_seed(162)
cases=[]
for i in range(64):
    classes=5; frames=8+i%9; targets=torch.randint(1,classes,(1,1+i%4))
    scores=torch.randn(1,frames,classes).log_softmax(-1)
    path,confidence=torchaudio.functional.forced_align(scores,targets,blank=0)
    spans=torchaudio.functional.merge_tokens(path[0],confidence[0].exp(),blank=0)
    assert [s.token for s in spans]==targets[0].tolist()
    cases.append({'classes':classes,'scores':scores.flatten().tolist(),'targets':targets[0].tolist(),
                  'spans':[{'start':s.start,'end':s.end,'confidence':s.score} for s in spans]})
a.out.parent.mkdir(parents=True,exist_ok=True)
a.out.write_text(json.dumps(cases,separators=(',',':')),encoding='utf-8')
print(len(cases),'oracle cases written')
