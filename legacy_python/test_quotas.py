import unittest, tempfile
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
import server, admin, quotas
from test_admin import Request
class QuotaTests(unittest.TestCase):
 def setUp(self):
  self.temp=tempfile.TemporaryDirectory();self.db,self.cfg=server.DB,admin.CONFIG
  server.DB=Path(self.temp.name)/'db';admin.CONFIG=Path(self.temp.name)/'none';server.initialize()
 def tearDown(self):server.DB,admin.CONFIG=self.db,self.cfg;self.temp.cleanup()
 def req(self,path,body=None,ip='127.0.0.1'):
  r=Request(path,'POST' if body is not None else 'GET',body);r.client_address=(ip,1);r.headers['X-Forwarded-For']='9.8.7.6';return r.run()
 def note(self,i):return dict(id=f'quota-note-{i:016d}',nickname='Эльф',message='С Новым годом!')
 def test_notes_retry_and_persistence(self):
  for i in range(2):self.assertEqual(self.req('/api/notes',self.note(i)).code,201)
  self.assertEqual(self.req('/api/notes',self.note(0)).code,200);server.initialize()
  self.assertEqual(self.req('/api/notes',self.note(3)).code,429)
  self.assertEqual(self.req('/api/notes',self.note(4),'127.0.0.2').code,201)
  self.assertEqual(self.req('/api/limits').result['notesRemaining'],0)
 def test_concurrent(self):
  with ThreadPoolExecutor(max_workers=6) as pool:codes=list(pool.map(lambda i:self.req('/api/notes',self.note(i)).code,range(6)))
  self.assertEqual(codes.count(201),2);self.assertEqual(codes.count(429),4)
 def test_ink(self):
  s=dict(id='quota-stroke-123456',color='#ffffff',width=5,points=[[0,0],[100,100]])
  r=self.req('/api/strokes',s);self.assertEqual(r.code,201);left=r.result['limits']['inkRemaining'];self.assertLess(left,quotas.INK_LIMIT)
  self.assertEqual(self.req('/api/strokes',s).result['limits']['inkRemaining'],left)
  big={**s,'id':'quota-stroke-654321','width':28,'points':[[0,0],[4800,2800]]}
  self.assertEqual(self.req('/api/strokes',big).code,429)
  self.assertEqual(self.req('/api/limits').result['inkRemaining'],left)
  self.assertEqual(self.req('/api/limits',ip='127.0.0.2').result['inkRemaining'],quotas.INK_LIMIT)
  with server.connect() as con:self.assertEqual(con.execute('SELECT COUNT(*) FROM strokes').fetchone()[0],1)
 def test_invalid(self):
  self.assertEqual(self.req('/api/notes',{**self.note(0),'message':''}).code,400)
  self.assertEqual(self.req('/api/limits').result['notesRemaining'],2)
