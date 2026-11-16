"""Shared canvas server. Python 3 standard library; SQLite persists strokes."""
import json, math, re, sqlite3, os
import admin, quotas
from pathlib import Path
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from urllib.parse import urlparse, parse_qs
ROOT = Path(__file__).resolve().parent
DB = ROOT / 'canvas.sqlite3'
def connect():
    con = sqlite3.connect(DB, timeout=20)
    con.row_factory = sqlite3.Row
    return con
def initialize():
    with connect() as con:
        con.execute('PRAGMA journal_mode=WAL')
        quotas.schema(con)
        con.execute("CREATE TABLE IF NOT EXISTS notes (seq INTEGER PRIMARY KEY AUTOINCREMENT, request_id TEXT UNIQUE NOT NULL, nickname TEXT NOT NULL, message TEXT NOT NULL, approved INTEGER NOT NULL DEFAULT 0, hue REAL NOT NULL)")
        con.execute('CREATE TABLE IF NOT EXISTS strokes (seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL, color TEXT NOT NULL, width REAL NOT NULL, points TEXT NOT NULL)')
    with connect() as con:
        note_columns={r['name'] for r in con.execute('PRAGMA table_info(notes)')}
        if 'featured' not in note_columns: con.execute('ALTER TABLE notes ADD COLUMN featured INTEGER NOT NULL DEFAULT 0')
        columns={r['name'] for r in con.execute('PRAGMA table_info(strokes)')}
        if 'auto_status' not in columns: con.execute("ALTER TABLE strokes ADD COLUMN auto_status TEXT NOT NULL DEFAULT 'pending'")
        if 'auto_details' not in columns: con.execute("ALTER TABLE strokes ADD COLUMN auto_details TEXT NOT NULL DEFAULT '{}'")
        if 'approved' not in columns: con.execute('ALTER TABLE strokes ADD COLUMN approved INTEGER NOT NULL DEFAULT 0')

    admin.schema(connect)

def validate_note(s):
    if not isinstance(s,dict): raise ValueError()
    if not isinstance(s.get('id'),str) or not re.fullmatch(r'[a-zA-Z0-9-]{16,64}',s['id']): raise ValueError()
    for key,limit in [('nickname',24),('message',120)]:
        v=s.get(key)
        if not isinstance(v,str) or not v.strip() or len(v.encode('utf-16-le'))//2>limit: raise ValueError()
        if re.search(r'https?://|www\.|\w+[.]\w+|<|> |!\[|\]\(|data:image|@',v,re.I): raise ValueError()
    return s

def validate(s):
    if not isinstance(s, dict): raise ValueError()
    if not isinstance(s.get('id'), str) or not re.fullmatch(r'[a-zA-Z0-9-]{16,64}', s['id']): raise ValueError()
    if not isinstance(s.get('color'), str) or not re.fullmatch(r'#[0-9a-fA-F]{6}', s['color']): raise ValueError()
    width = s.get('width')
    if type(width) not in (int, float) or not math.isfinite(width) or not 2 <= width <= 28: raise ValueError()
    points = s.get('points')
    if not isinstance(points, list) or not 1 <= len(points) <= 1800: raise ValueError()
    for p in points:
        if not isinstance(p, list) or len(p) != 2: raise ValueError()
        if any(type(v) not in (int, float) or not math.isfinite(v) for v in p): raise ValueError()
        if not 0 <= p[0] <= 4800 or not 0 <= p[1] <= 2800: raise ValueError()
    return s
def record(row):
    return dict(seq=row['seq'], id=row['id'], color=row['color'], width=row['width'], points=json.loads(row['points']))
class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs): super().__init__(*args, directory=str(ROOT/'dist'), **kwargs)
    def respond(self, value, code=200):
        body=json.dumps(value, ensure_ascii=False).encode()
        self.send_response(code);
        if getattr(self,'extra_cookie',None): self.send_header('Set-Cookie',self.extra_cookie); self.extra_cookie=None
        self.send_header('Content-Type','application/json; charset=utf-8'); self.send_header('Cache-Control','no-store'); self.send_header('Content-Length',str(len(body))); self.end_headers(); self.wfile.write(body)
    def end_headers(self):
        self.send_header("X-Content-Type-Options","nosniff")
        self.send_header("X-Frame-Options","DENY")
        self.send_header("Referrer-Policy","no-referrer")
        super().end_headers()
    def log_message(self, format, *args):
        # Never log the private admin URL or visitor IP to the console.
        pass
    def list_directory(self, path):
        self.send_error(404); return None
    def do_GET(self):
        if admin.handle(self,connect): return
        parsed=urlparse(self.path)
        if parsed.path == '/api/limits':
            with connect() as con: result=quotas.limits(con,quotas.peer(con,self))
            return self.respond(result)
        if parsed.path == '/api/tree':
            with connect() as con:
                balls=con.execute('SELECT seq AS id,hue,nickname FROM notes WHERE approved>=0 ORDER BY seq').fetchall()
                notes=con.execute('SELECT nickname,message FROM notes WHERE approved=1 AND featured=1 ORDER BY seq DESC LIMIT 100').fetchall()
            return self.respond({'balls':[dict(r) for r in balls],'messageCount':len(balls),'notes':[dict(r) for r in notes]})
        if parsed.path == '/api/strokes':
            try:
                after=max(0,int(parse_qs(parsed.query).get('after',['0'])[0]))
                with connect() as con: rows=con.execute('SELECT * FROM strokes WHERE approved=1 ORDER BY seq',()).fetchall()
                with connect() as con: limits=quotas.limits(con,quotas.peer(con,self))
                self.respond({'strokes':[record(r) for r in rows],'limits':limits})
            except (ValueError, OverflowError): self.respond({'error':'Invalid cursor'},400)
        elif parsed.path.startswith('/api/'): self.respond({'error':'Not found'},404)
        else: super().do_GET()
    def do_POST(self):
        if admin.handle(self,connect): return
        if urlparse(self.path).path == '/api/notes':
            try:
                length=int(self.headers.get('Content-Length','0'))
                if not 0 < length <= 4096: return self.respond({'error':'Invalid size'},413)
                note=validate_note(json.loads(self.rfile.read(length)))
                with connect() as con:
                    con.execute('BEGIN IMMEDIATE')
                    existing=con.execute('SELECT seq FROM notes WHERE request_id=?',(note['id'],)).fetchone()
                    if existing: return self.respond({'id':existing['seq'],'pending':True})
                    peer=quotas.peer(con,self)
                    if quotas.limits(con,peer)['notesRemaining']<1:
                        return self.respond({'error':'С этого IP уже отправлены два послания.','limits':quotas.limits(con,peer)},429)
                    count=con.execute('SELECT COUNT(*) FROM notes').fetchone()[0]
                    cur=con.execute('INSERT INTO notes(request_id,nickname,message,hue) VALUES(?,?,?,?)',(note['id'],note['nickname'].strip(),note['message'].strip(),(count*137.507764+18)%360))
                    ident=cur.lastrowid
                    quotas.charge(con,peer,'notes',1)
                admin.associate(self,connect,'notes',ident)
                return self.respond({'id':ident,'pending':True},201)
            except (ValueError,TypeError): return self.respond({'error':'Invalid snowflake'},400)
        if urlparse(self.path).path != '/api/strokes': return self.respond({'error':'Not found'},404)
        try:
            length=int(self.headers.get('Content-Length','0'))
            if not 0 < length <= 120000: return self.respond({'error':'Invalid body size'},413)
            s=validate(json.loads(self.rfile.read(length)))
            with connect() as con:
                con.execute('BEGIN IMMEDIATE')
                peer=quotas.peer(con,self)
                row=con.execute('SELECT * FROM strokes WHERE id=?',(s['id'],)).fetchone()
                if row is None:
                    cost=quotas.cost(s)
                    if cost>quotas.limits(con,peer)['inkRemaining']:
                        return self.respond({'error':'Недостаточно краски для этого штриха.','limits':quotas.limits(con,peer)},429)
                    con.execute('INSERT INTO strokes(id,color,width,points) VALUES(?,?,?,?)',(s['id'],s['color'],s['width'],json.dumps(s['points'])))
                    quotas.charge(con,peer,'ink',cost)
                    row=con.execute('SELECT * FROM strokes WHERE id=?',(s['id'],)).fetchone()
                limits=quotas.limits(con,peer)
            admin.associate(self,connect,'strokes',row['seq'])
            self.respond({**record(row),'limits':limits},201)
        except (ValueError, TypeError, KeyError): self.respond({'error':'Invalid stroke'},400)
if __name__ == '__main__':
    initialize()
    from local_ai.worker import start
    print('Local AI:', 'enabled' if start(connect) else 'not trained or dependencies missing; manual review active',flush=True)
    port=int(os.environ.get('PORT','4173'))
    print(f'Canvas server: http://localhost:{port}',flush=True)
    ThreadingHTTPServer((os.environ.get('HOST','127.0.0.1'),port), Handler).serve_forever()
