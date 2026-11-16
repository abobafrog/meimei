"""Private moderation and first-party, pseudonymous visit statistics."""
import hashlib, hmac, secrets, json, time, threading
from pathlib import Path
from http.cookies import SimpleCookie
from urllib.parse import urlparse, parse_qs
import analytics
ROOT=Path(__file__).resolve().parent
CONFIG=ROOT/'.admin.json'
LOCK=threading.RLock()
SESSIONS={}
ATTEMPTS={}
def setup():
    if CONFIG.exists():
        print('Configuration already exists. Remove .admin.json explicitly to reset access.'); return
    password=secrets.token_urlsafe(24); salt=secrets.token_hex(16)
    config={'path':'/manage-'+secrets.token_urlsafe(24),'salt':salt,'hash':hashlib.pbkdf2_hmac('sha256',password.encode(),bytes.fromhex(salt),600000).hex(),'key':secrets.token_hex(32)}
    import os
    fd=os.open(CONFIG,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
    with os.fdopen(fd,'w') as f: json.dump(config,f)
    print('Private address: http://localhost:4173'+config['path']); print('Password (save it now): '+password)
def config():
    return json.loads(CONFIG.read_text()) if CONFIG.exists() else None
def cookie(h,name):
    try:
        c=SimpleCookie(h.headers.get('Cookie','')); return c[name].value if name in c else ''
    except Exception: return ''
def schema(connect):
    with connect() as c:
        c.execute('CREATE TABLE IF NOT EXISTS visits (id INTEGER PRIMARY KEY, visitor TEXT, created INTEGER, path TEXT, source TEXT, device TEXT, language TEXT)')
        c.execute('CREATE TABLE IF NOT EXISTS submissions (kind TEXT, item TEXT, visitor TEXT, created INTEGER, UNIQUE(kind,item))')
        analytics.schema(c)
def visitor(h):
    v=cookie(h,'frokla_visitor')
    return v if len(v)==32 and all(x in '0123456789abcdef' for x in v) else ''
def associate(h,connect,kind,item):
    with connect() as c: c.execute('INSERT OR IGNORE INTO submissions VALUES(?,?,?,?)',(kind,str(item),visitor(h),int(time.time())))
def handle(h,connect):
    cfg=config(); path=urlparse(h.path).path
    if path=='/api/visit' and h.command=='POST':
        if h.headers.get('Sec-Fetch-Site')=='cross-site':h.respond({},403);return True
        v=visitor(h) or secrets.token_hex(16)
        try:
            length=int(h.headers.get('Content-Length','0'))
            if not 0<=length<=1024:raise ValueError()
            payload=json.loads(h.rfile.read(length)) if length else {}
            if not isinstance(payload,dict):raise ValueError()
            analytics.record_visit(h,connect,v,payload)
        except (ValueError,TypeError):h.respond({'error':'Неверный запрос'},400);return True
        h.extra_cookie='frokla_visitor='+v+'; Path=/; HttpOnly; SameSite=Lax; Max-Age=7776000'+ ('; Secure' if h.headers.get('X-Forwarded-Proto')=='https' else '')
        h.respond({'ok':True});return True
    if path=='/api/interests' and h.command=='POST':
        value,code=analytics.interests(h,connect,visitor(h));h.respond(value,code);return True
    if not cfg or not (path==cfg['path'] or path.startswith(cfg['path']+'/')): return False
    base=cfg['path']; action=path[len(base):]
    def reply(value,code=200): h.respond(value,code); return True
    def body():
        length=int(h.headers.get('Content-Length','0'))
        if not 0<length<=8192: raise ValueError()
        data=json.loads(h.rfile.read(length))
        if not isinstance(data,dict): raise ValueError()
        return data
    if h.command=='GET' and not action:
        raw=(ROOT/'admin.html').read_bytes(); h.send_response(200); h.send_header('Content-Type','text/html; charset=utf-8'); h.send_header('Cache-Control','no-store'); h.send_header('Referrer-Policy','no-referrer'); h.send_header('X-Robots-Tag','noindex, nofollow'); h.send_header('Content-Length',str(len(raw))); h.end_headers(); h.wfile.write(raw); return True
    if h.command=='POST':
        origin=h.headers.get('Origin','')
        if urlparse(origin).netloc!=h.headers.get('Host') or h.headers.get('Sec-Fetch-Site')=='cross-site': return reply({'error':'Недопустимый источник запроса'},403)
    now=time.time()
    with LOCK:
        for key in list(SESSIONS):
            if SESSIONS[key]['expires']<now: del SESSIONS[key]
        session=SESSIONS.get(cookie(h,'frokla_admin'))
    if action=='/login' and h.command=='POST':
        ip=h.client_address[0]
        with LOCK:
            for key in list(ATTEMPTS):
                if ATTEMPTS[key][-1]<now-900: del ATTEMPTS[key]
            attempts=ATTEMPTS.setdefault(ip,[])
            if len(attempts)>=5: return reply({'error':'Слишком много попыток. Подожди 15 минут.'},429)
            attempts.append(now)
        try:
            password=body().get('password','')
            if not isinstance(password,str) or len(password)>256: raise ValueError()
            digest=hashlib.pbkdf2_hmac('sha256',password.encode(),bytes.fromhex(cfg['salt']),600000).hex()
            if not hmac.compare_digest(digest,cfg['hash']): return reply({'error':'Неверный пароль'},401)
        except (ValueError,TypeError): return reply({'error':'Неверный запрос'},400)
        token=secrets.token_urlsafe(32); csrf=secrets.token_urlsafe(32)
        with LOCK:
            SESSIONS[token]={'expires':now+28800,'csrf':csrf}; ATTEMPTS.pop(ip,None)
        h.extra_cookie='frokla_admin='+token+'; Path='+base+'; HttpOnly; SameSite=Strict; Max-Age=28800'+('; Secure' if h.headers.get('X-Forwarded-Proto')=='https' else '')
        return reply({'csrf':csrf})
    if not session: return reply({'error':'Войди по паролю'},401)
    if h.command=='POST' and not hmac.compare_digest(h.headers.get('X-CSRF-Token',''),session['csrf']): return reply({'error':'Обнови страницу'},403)
    if action in ('/analytics','/connections') and h.command=='GET':
        try:
            params=parse_qs(urlparse(h.path).query)
            if action=='/connections':return reply({'visits':analytics.connections(connect,params)})
            days=max(1,min(90,int(params.get('days',['90'])[0])))
            return reply(analytics.summary(connect,days))
        except (ValueError,TypeError):return reply({'error':'Неверный фильтр'},400)
    if action in ('/map.js','/map.css') and h.command=='GET':
        file=ROOT/('WorkshopMap.js' if action=='/map.js' else 'WorkshopMap.css');raw=file.read_bytes()
        h.send_response(200);h.send_header('Content-Type','text/javascript; charset=utf-8' if action=='/map.js' else 'text/css; charset=utf-8');h.send_header('Cache-Control','no-store');h.send_header('Content-Length',str(len(raw)));h.end_headers();h.wfile.write(raw);return True
    if action=='/data' and h.command=='GET':
        with connect() as c:
            notes=[dict(r) for r in c.execute('SELECT seq,nickname,message,approved,featured FROM notes ORDER BY seq DESC LIMIT 500')]
            strokes=[dict(r) for r in c.execute('SELECT seq,id,color,width,points,approved,auto_status FROM strokes ORDER BY seq DESC LIMIT 2000')]
            visits=[dict(r) for r in c.execute('SELECT visitor,created,device,language,source FROM visits ORDER BY id DESC LIMIT 200')]
            stats=dict(c.execute('SELECT COUNT(*) views,COUNT(DISTINCT visitor) visitors FROM visits').fetchone())
            days=[dict(r) for r in c.execute("SELECT date(created,'unixepoch') day,COUNT(*) views FROM visits GROUP BY day ORDER BY day DESC LIMIT 30")]
            submissions=[dict(r) for r in c.execute('SELECT * FROM submissions ORDER BY created DESC LIMIT 500')]
        return reply({'csrf':session['csrf'],'notes':notes,'strokes':strokes,'visits':visits,'stats':stats,'days':days,'submissions':submissions})
    if action=='/moderate' and h.command=='POST':
        try:
            data=body(); kind=data['kind']; ident=int(data['id']); operation=data['action']
            if kind not in ('notes','strokes') or operation not in ('approve','reject','feature','unfeature'): raise ValueError()
            if kind=='strokes' and operation in ('feature','unfeature'): raise ValueError()
            with connect() as c:
                if operation in ('feature','unfeature'): cur=c.execute('UPDATE notes SET featured=?,approved=1 WHERE seq=?',(int(operation=='feature'),ident))
                elif kind=='notes': cur=c.execute('UPDATE notes SET approved=?,featured=0 WHERE seq=?',(1 if operation=='approve' else -1,ident))
                else: cur=c.execute('UPDATE strokes SET approved=? WHERE seq=?',(1 if operation=='approve' else -1,ident))
                if not cur.rowcount: return reply({'error':'Запись не найдена'},404)
            return reply({'ok':True})
        except (ValueError,KeyError,TypeError): return reply({'error':'Неверный запрос'},400)
    if action=='/logout' and h.command=='POST':
        with LOCK: SESSIONS.pop(cookie(h,'frokla_admin'),None)
        h.extra_cookie='frokla_admin=; Path='+base+'; HttpOnly; SameSite=Strict; Max-Age=0'; return reply({'ok':True})
    return reply({'error':'Не найдено'},404)
if __name__=='__main__': setup()
