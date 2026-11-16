"""Persistent per-peer quotas, charged in the submission's SQLite transaction."""
import hashlib, hmac, ipaddress, math, secrets
from client_ip import address as client_address
NOTE_LIMIT = 2
INK_LIMIT = 60000

def schema(con):
    con.execute('CREATE TABLE IF NOT EXISTS quota_settings (key TEXT PRIMARY KEY,value TEXT NOT NULL)')
    con.execute('INSERT OR IGNORE INTO quota_settings VALUES(?,?)', ('secret',secrets.token_hex(32)))
    con.execute('CREATE TABLE IF NOT EXISTS quotas (peer TEXT PRIMARY KEY,notes INTEGER NOT NULL DEFAULT 0,ink REAL NOT NULL DEFAULT 0)')

def peer(con, handler):
    address = client_address(handler)
    if isinstance(address, ipaddress.IPv6Address) and address.ipv4_mapped:
        address = address.ipv4_mapped
    # Forwarded headers are not trusted; the connected peer is authoritative.
    secret = con.execute("SELECT value FROM quota_settings WHERE key='secret'").fetchone()[0]
    return hmac.new(secret.encode(),str(address).encode(),hashlib.sha256).hexdigest()

def limits(con, key):
    row = con.execute('SELECT notes,ink FROM quotas WHERE peer=?',(key,)).fetchone()
    notes, ink = row if row else (0,0)
    return dict(notesRemaining=max(0,NOTE_LIMIT-notes),notesLimit=NOTE_LIMIT,
                inkRemaining=max(0,INK_LIMIT-ink),inkLimit=INK_LIMIT)

def cost(stroke):
    points, width = stroke['points'], stroke['width']
    return math.pi*width*width/4 + width*sum(math.dist(a,b) for a,b in zip(points,points[1:]))

def charge(con,key,kind,amount):
    con.execute('INSERT OR IGNORE INTO quotas(peer) VALUES(?)',(key,))
    if kind=='notes': con.execute('UPDATE quotas SET notes=notes+? WHERE peer=?',(amount,key))
    else: con.execute('UPDATE quotas SET ink=ink+? WHERE peer=?',(amount,key))
