"""Approximate connection geography and explicitly selected interests."""
import json, math, os, time, threading
from urllib.request import Request, urlopen
from concurrent.futures import ThreadPoolExecutor
from urllib.parse import urlparse
import quotas
from client_ip import address
INTERESTS={'art':'Рисование','animals':'Животные','music':'Музыка','games':'Игры','travel':'Путешествия','crafts':'Творчество','winter':'Зимние праздники'}
POOL=ThreadPoolExecutor(max_workers=2,thread_name_prefix='geo')
SLOTS=threading.BoundedSemaphore(32)
LOCK=threading.Lock()
PENDING=set()

def schema(con):
    columns={row['name'] for row in con.execute('PRAGMA table_info(visits)')}
    for name,kind in [('peer','TEXT'),('geo_status',"TEXT DEFAULT 'unknown'"),('country','TEXT'),('region','TEXT'),('city','TEXT'),('lat','REAL'),('lon','REAL'),('network','TEXT')]:
        if name not in columns:con.execute(f'ALTER TABLE visits ADD COLUMN {name} {kind}')
    con.execute('CREATE TABLE IF NOT EXISTS geo_cache (peer TEXT PRIMARY KEY,payload TEXT,expires INTEGER)')
    con.execute('CREATE TABLE IF NOT EXISTS geo_requests (day TEXT PRIMARY KEY,total INTEGER NOT NULL)')
    con.execute('CREATE TABLE IF NOT EXISTS visitor_interests (visitor TEXT PRIMARY KEY,interests TEXT NOT NULL,updated INTEGER NOT NULL)')
    con.execute('CREATE INDEX IF NOT EXISTS visits_time_idx ON visits(created)')
    con.execute('CREATE INDEX IF NOT EXISTS visits_geo_idx ON visits(peer,geo_status)')

def lookup(ip):
    req=Request('https://ipwho.is/'+str(ip)+'?fields=success,country,region,city,latitude,longitude,connection.isp',headers={'User-Agent':'FroklaWorkshop/1.0','Accept':'application/json'})
    with urlopen(req,timeout=4) as response:data=json.loads(response.read(65536))
    if data.get('success') is not True:raise ValueError('Geo lookup unavailable')
    lat,lon=data.get('latitude'),data.get('longitude')
    if type(lat) not in (float,int) or type(lon) not in (float,int) or not math.isfinite(lat) or not math.isfinite(lon) or not -90<=lat<=90 or not -180<=lon<=180:raise ValueError('Invalid coordinates')
    return dict(geo_status='located',country=str(data.get('country',''))[:100],region=str(data.get('region',''))[:100],city=str(data.get('city',''))[:100],lat=round(lat,2),lon=round(lon,2),network=str((data.get('connection') or {}).get('isp',''))[:140])

def apply(con,peer,data):
    con.execute('UPDATE visits SET geo_status=?,country=?,region=?,city=?,lat=?,lon=?,network=? WHERE peer=? AND geo_status=\'pending\'',tuple(data.get(k) for k in ['geo_status','country','region','city','lat','lon','network'])+(peer,))

def schedule(connect,peer,ip):
    with LOCK:
        if peer in PENDING or not SLOTS.acquire(blocking=False):return
        PENDING.add(peer)
    def work():
        try:
            now=int(time.time());day=time.strftime('%Y-%m-%d',time.gmtime(now))
            with connect() as con:
                con.execute('BEGIN IMMEDIATE')
                row=con.execute('SELECT total FROM geo_requests WHERE day=?',(day,)).fetchone()
                allowed=not row or row[0]<900
                if allowed:con.execute('INSERT INTO geo_requests VALUES(?,1) ON CONFLICT(day) DO UPDATE SET total=total+1',(day,))
            try:data=lookup(ip) if allowed else {'geo_status':'unavailable'}
            except Exception:data={'geo_status':'unavailable'}
            ttl=7*86400 if data['geo_status']=='located' else 3600
            with connect() as con:
                con.execute('INSERT OR REPLACE INTO geo_cache VALUES(?,?,?)',(peer,json.dumps(data),now+ttl))
                apply(con,peer,data)
        finally:
            with LOCK:PENDING.discard(peer)
            SLOTS.release()
    POOL.submit(work)

def record_visit(h,connect,visitor,payload):
    now=int(time.time());ip=address(h)
    source=(urlparse(str(payload.get('source',''))).hostname or 'direct')[:120]
    ua=h.headers.get('User-Agent','');device='tablet' if 'iPad' in ua or 'Tablet' in ua else 'mobile' if 'Mobile' in ua else 'desktop'
    with connect() as con:
        peer=quotas.peer(con,h)
        for table,col in [('visits','created'),('submissions','created'),('visitor_interests','updated')]:con.execute(f'DELETE FROM {table} WHERE {col}<?',(now-90*86400,))
        con.execute('DELETE FROM geo_cache WHERE expires<?',(now,));con.execute('DELETE FROM geo_requests WHERE day<?',(time.strftime('%Y-%m-%d',time.gmtime(now-2*86400)),))
        con.execute("UPDATE visits SET geo_status='unavailable' WHERE geo_status='pending' AND created<?",(now-120,))
        cache=con.execute('SELECT payload FROM geo_cache WHERE peer=?',(peer,)).fetchone()
        geo=json.loads(cache[0]) if cache else {'geo_status':'pending' if ip.is_global else 'local'}
        if os.environ.get('GEOLOOKUP_ENABLED','1')!='1':geo={'geo_status':'disabled'}
        recent=con.execute('SELECT 1 FROM visits WHERE visitor=? AND created>?',(visitor,now-30)).fetchone()
        if not recent:
            con.execute('INSERT INTO visits(visitor,created,path,source,device,language,peer,geo_status,country,region,city,lat,lon,network) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)',
                        (visitor,now,'/',source,device,h.headers.get('Accept-Language','')[:32],peer)+tuple(geo.get(k) for k in ['geo_status','country','region','city','lat','lon','network']))
    if geo['geo_status']=='pending':schedule(connect,peer,ip)

def interests(h,connect,visitor):
    origin=h.headers.get('Origin','')
    if h.headers.get('Sec-Fetch-Site')=='cross-site' or urlparse(origin).netloc!=h.headers.get('Host'):return {'error':'Недопустимый источник'},403
    if not visitor:return {'error':'Обнови страницу и попробуй ещё раз'},401
    try:
        length=int(h.headers.get('Content-Length','0'))
        if not 0<length<=1024:raise ValueError()
        data=json.loads(h.rfile.read(length));values=data.get('interests')
        if not isinstance(values,list) or len(values)>len(INTERESTS) or any(not isinstance(v,str) or v not in INTERESTS for v in values):raise ValueError()
    except (ValueError,AttributeError,TypeError):return {'error':'Выбери интересы из списка'},400
    with connect() as con:
        if values:con.execute('INSERT OR REPLACE INTO visitor_interests VALUES(?,?,?)',(visitor,json.dumps(sorted(set(values))),int(time.time())))
        else:con.execute('DELETE FROM visitor_interests WHERE visitor=?',(visitor,))
    return {'ok':True},200

def summary(connect,days=90):
    since=int(time.time())-days*86400
    with connect() as con:
        rows=[dict(r) for r in con.execute('SELECT country,region,city,lat,lon,COUNT(*) views,COUNT(DISTINCT visitor) visitors,MAX(created) latest FROM visits WHERE created>=? AND geo_status=\'located\' GROUP BY country,region,city,lat,lon ORDER BY views DESC',(since,))]
        totals=dict(con.execute("SELECT COUNT(*) views,COUNT(DISTINCT visitor) visitors,SUM(CASE WHEN geo_status='located' THEN 1 ELSE 0 END) located FROM visits WHERE created>=?",(since,)).fetchone())
        sources=[dict(r) for r in con.execute('SELECT source label,COUNT(*) value FROM visits WHERE created>=? GROUP BY source ORDER BY value DESC LIMIT 15',(since,))]
        devices=[dict(r) for r in con.execute('SELECT device label,COUNT(*) value FROM visits WHERE created>=? GROUP BY device ORDER BY value DESC',(since,))]
        profiles=con.execute('SELECT interests FROM visitor_interests WHERE visitor IN (SELECT visitor FROM visits WHERE created>=?)',(since,)).fetchall()
        selected={k:0 for k in INTERESTS}
        for row in profiles:
            for k in json.loads(row[0]):
                if k in selected:selected[k]+=1
    return dict(days=days,locations=rows,totals=totals,sources=sources,devices=devices,interests=[{'label':INTERESTS[k],'value':v} for k,v in selected.items()],respondents=len(profiles))

def connections(connect,params):
    days=int(params.get('days',['90'])[0]);days=max(1,min(90,days));since=int(time.time())-days*86400
    sql='SELECT v.visitor,v.created,v.device,v.language,v.source,v.country,v.region,v.city,v.geo_status,v.network,p.interests FROM visits v LEFT JOIN visitor_interests p ON p.visitor=v.visitor WHERE v.created>=?'
    values=[since]
    if 'lat' in params and 'lon' in params:
        lat,lon=float(params['lat'][0]),float(params['lon'][0])
        if not math.isfinite(lat) or not math.isfinite(lon):raise ValueError()
        sql+=' AND v.lat=? AND v.lon=?';values.extend([lat,lon])
    if params.get('unknown')==['1']:sql+=" AND v.geo_status!='located'"
    with connect() as con:rows=[dict(r) for r in con.execute(sql+' ORDER BY v.created DESC LIMIT 200',values)]
    for r in rows:r['interests']=[INTERESTS.get(k,k) for k in json.loads(r['interests'] or '[]')]
    return rows
