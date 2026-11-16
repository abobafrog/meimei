"""Run locally: python3 moderate.py list | approve ID | reject ID."""
import sys
from server import initialize, connect
initialize()
command=sys.argv[1] if len(sys.argv)>1 else 'list'
with connect() as db:
    if command=='list':
        for row in db.execute('SELECT seq,nickname,message FROM notes WHERE approved=0 ORDER BY seq'):
            print(dict(row))
    elif command=='approve':
        db.execute('UPDATE notes SET approved=1 WHERE seq=?',(int(sys.argv[2]),))
    elif command=='feature':
        db.execute('UPDATE notes SET approved=1, featured=1 WHERE seq=?',(int(sys.argv[2]),))
    elif command=='unfeature':
        db.execute('UPDATE notes SET featured=0 WHERE seq=?',(int(sys.argv[2]),))
    elif command=='reject':
        db.execute('UPDATE notes SET approved=0 WHERE seq=?',(int(sys.argv[2]),))
    else: raise SystemExit('Use list, approve ID, reject ID')
