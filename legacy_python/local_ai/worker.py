import io,json,time,threading
from pathlib import Path

def start(connect):
    root=Path(__file__).parent
    if not (root/'classifier.json').exists() or not (root/'weights').exists():return False
    try:
        from .model import load,predict
        from PIL import Image
        from auto_moderation import rasterize
        objects=load()
    except (ImportError,OSError):return False
    def run():
        while True:
            try:
                with connect() as db:
                    rows=db.execute("SELECT * FROM strokes WHERE approved=0 AND auto_status IN ('pending','error') ORDER BY seq LIMIT 100").fetchall()
                    background=db.execute('SELECT * FROM strokes WHERE approved=1 ORDER BY seq').fetchall()
                if rows:
                    image=Image.open(io.BytesIO(rasterize(list(background)+list(rows))))
                    result=predict(image,objects)
                    with connect() as db:db.executemany('UPDATE strokes SET auto_status=?,auto_details=? WHERE seq=? AND approved=0',[(result['decision'],json.dumps(result),r['seq']) for r in rows])
            except Exception:pass
            time.sleep(15)
    threading.Thread(target=run,daemon=True).start();return True
