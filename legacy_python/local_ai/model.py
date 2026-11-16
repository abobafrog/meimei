"""Offline CLIP features + trainable binary head. No automatic publication."""
import argparse,json,hashlib,random
from pathlib import Path
ROOT=Path(__file__).resolve().parent

def load(download=False):
    import torch
    from transformers import CLIPModel,CLIPProcessor
    path='openai/clip-vit-base-patch32' if download else str(ROOT/'weights')
    model=CLIPModel.from_pretrained(path,local_files_only=not download).eval()
    processor=CLIPProcessor.from_pretrained(path,local_files_only=not download)
    if download:
        model.save_pretrained(ROOT/'weights');processor.save_pretrained(ROOT/'weights')
    return torch,model,processor

def features(image,objects):
    from PIL import ImageOps
    torch,model,processor=objects
    image=ImageOps.pad(image.convert('RGB'),(448,448),color='#101114')
    with torch.inference_mode():
        f=model.get_image_features(**processor(images=image,return_tensors='pt'))
        return (f/f.norm(dim=-1,keepdim=True)).squeeze(0)

def train(manifest):
    from PIL import Image
    objects=load();torch=objects[0];torch.manual_seed(17)
    rows=json.loads(Path(manifest).read_text());groups={0:[],1:[]};seen={}
    for row in rows:
        path=Path(manifest).resolve().parent/row['file'];label=row['label']
        if label not in (0,1):raise ValueError('Labels must be 0=allowed or 1=forbidden')
        digest=hashlib.sha256(path.read_bytes()).hexdigest()
        if digest in seen:
            if seen[digest]!=label:raise ValueError('Conflicting labels for identical images')
            continue
        seen[digest]=label
        with Image.open(path) as image:f=features(image,objects)
        groups[label].append((f,label))
    if min(map(len,groups.values()))<20:raise ValueError('Need at least 20 distinct images per class; this is a starting minimum, not a quality guarantee')
    training=[];validation=[];rng=random.Random(17)
    for group in groups.values():
        rng.shuffle(group);cut=max(5,len(group)//5);validation+=group[:cut];training+=group[cut:]
    x=torch.stack([f for f,y in training]);y=torch.tensor([y for f,y in training],dtype=torch.float32)
    head=torch.nn.Linear(x.shape[1],1);optimizer=torch.optim.AdamW(head.parameters(),lr=.01,weight_decay=.1)
    for _ in range(200):
        optimizer.zero_grad();loss=torch.nn.functional.binary_cross_entropy_with_logits(head(x).squeeze(1),y);loss.backward();optimizer.step()
    with torch.no_grad():
        scores=head(torch.stack([f for f,y in validation])).squeeze(1).sigmoid().tolist()
    labels=[y for f,y in validation];report={'samples':len(seen),'validation_samples':len(labels),'false_negatives':sum(y==1 and s<.5 for s,y in zip(scores,labels)),'false_positives':sum(y==0 and s>=.5 for s,y in zip(scores,labels)),'note':'Experimental classifier. No automatic publishing. Scores are not calibrated probabilities.'}
    checkpoint={'weight':head.weight.detach().tolist()[0],'bias':head.bias.item(),'report':report}
    (ROOT/'classifier.json').write_text(json.dumps(checkpoint,indent=2));print(json.dumps(report,indent=2))

def predict(image,objects=None):
    objects=objects or load();torch=objects[0]
    checkpoint=json.loads((ROOT/'classifier.json').read_text());f=features(image,objects)
    score=torch.sigmoid((f*torch.tensor(checkpoint['weight'])).sum()+checkpoint['bias']).item()
    return {'score':score,'decision':'flagged' if score>=.8 else 'review','auto_publish':False}

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('command',choices=['download','train','predict']);p.add_argument('file',nargs='?');a=p.parse_args()
    if a.command=='download':load(True)
    elif a.command=='train':train(a.file)
    else:
        from PIL import Image
        with Image.open(a.file) as im:print(json.dumps(predict(im)))
