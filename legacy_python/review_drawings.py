"""Owner-only local moderation: python3 review_drawings.py preview | approve ID | reject ID."""
import sys, html
from pathlib import Path
from server import initialize, connect
initialize()
command=sys.argv[1] if len(sys.argv)>1 else 'preview'
with connect() as db:
    if command in ('approve','reject'):
        ident=int(sys.argv[2]);db.execute('UPDATE strokes SET approved=? WHERE seq=?',(1 if command=='approve' else -1,ident))
        print('Updated',ident)
    elif command=='preview':
        import json
        parts=['<!doctype html><meta charset="utf-8"><title>Review drawings</title><style>body{background:#101720;color:#eee;font:16px system-ui;padding:30px}svg{width:100%;background:#172332;border:1px solid #456}section{margin:35px 0}</style><h1>Pending strokes</h1>']
        for row in db.execute('SELECT * FROM strokes WHERE approved=0 ORDER BY seq'):
            points=json.loads(row['points']);coords=' '.join(f'{float(x)},{float(y)}' for x,y in points)
            parts.append(f'<section><h2>ID {row["seq"]} — {html.escape(row["auto_status"])}</h2><p>{html.escape(row["auto_details"])}</p><svg viewBox="0 0 4800 2800"><polyline points="{coords}" fill="none" stroke="{html.escape(row["color"],quote=True)}" stroke-width="{float(row["width"])}" stroke-linecap="round" stroke-linejoin="round"/><circle cx="{points[0][0]}" cy="{points[0][1]}" r="{float(row["width"])/2}" fill="{html.escape(row["color"],quote=True)}"/></svg></section>')
        target=Path(__file__).parent/'drawing-review.html';target.write_text(''.join(parts));print(target)
    else:raise SystemExit('Use preview, approve ID, reject ID')
