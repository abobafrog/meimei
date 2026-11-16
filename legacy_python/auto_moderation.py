"""Server-side image moderation; no client image or approval is trusted."""
import base64,json,os,struct,zlib,math,time,threading
from urllib.request import Request,urlopen

def rasterize(rows):
    w,h=1200,700; pixels=bytearray([16,17,20])*(w*h)
    def dot(x,y,r,color):
        for yy in range(max(0,int(y-r)),min(h,int(y+r)+2)):
            for xx in range(max(0,int(x-r)),min(w,int(x+r)+2)):
                if (xx-x)**2+(yy-y)**2<=r*r:
                    start=(yy*w+xx)*3;pixels[start:start+3]=color
    for row in rows:
        color=bytes.fromhex(row['color'][1:]);points=json.loads(row['points']);radius=max(.75,row['width']/8)
        previous=None
        for x,y in points:
            x/=4;y/=4
            if previous:
                a,b=previous;steps=max(1,math.ceil(math.hypot(x-a,y-b)))
                for i in range(steps+1):dot(a+(x-a)*i/steps,b+(y-b)*i/steps,radius,color)
            else:dot(x,y,radius,color)
            previous=x,y
    def chunk(kind,data):return struct.pack('!I',len(data))+kind+data+struct.pack('!I',zlib.crc32(kind+data)&0xffffffff)
    raw=b''.join(b'\0'+pixels[y*w*3:(y+1)*w*3] for y in range(h))
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!2I5B',w,h,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b'')

def classify(image,key):
    payload={'model':'omni-moderation-latest','input':[{'type':'image_url','image_url':{'url':'data:image/png;base64,'+base64.b64encode(image).decode()}}]}
    request=Request('https://api.openai.com/v1/moderations',data=json.dumps(payload).encode(),headers={'Authorization':'Bearer '+key,'Content-Type':'application/json'})
    with urlopen(request,timeout=30) as response:result=json.load(response)['results'][0]
    if type(result.get('flagged')) is not bool:raise ValueError('Invalid moderation response')
    return result

def scan(connect,key):
    with connect() as db:
        pending=db.execute("SELECT * FROM strokes WHERE approved=0 AND auto_status IN ('pending','error') ORDER BY seq LIMIT 100").fetchall()
        if not pending:return
        background=db.execute('SELECT * FROM strokes WHERE approved=1 ORDER BY seq').fetchall()
    ids=[row['seq'] for row in pending]
    try:
        result=classify(rasterize(list(background)+list(pending)),key)
        # A clean image still requires owner approval: image-only classifiers do not cover all text categories.
        status='flagged' if result['flagged'] else 'review'
        details=json.dumps(result.get('categories',{}))
    except Exception:
        status,details='error','{}'
    with connect() as db:
        db.executemany('UPDATE strokes SET auto_status=?,auto_details=? WHERE seq=? AND approved=0',[(status,details,i) for i in ids])

def start(connect):
    key=os.environ.get('OPENAI_API_KEY')
    if not key:return False
    def loop():
        while True:
            try:scan(connect,key)
            except Exception:pass
            time.sleep(15)
    threading.Thread(target=loop,daemon=True).start();return True
