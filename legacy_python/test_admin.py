import unittest,tempfile,io,json,hashlib
from pathlib import Path
import server,admin
class Request(server.Handler):
 def __init__(self,path,method='GET',body=None,cookie='',csrf='',origin='http://localhost:4173'):
  self.path=path;self.command=method;raw=json.dumps(body).encode() if body is not None else b''
  self.headers={'Host':'localhost:4173','Origin':origin,'Content-Length':str(len(raw)),'Cookie':cookie,'X-CSRF-Token':csrf,'User-Agent':'Test Mobile','Accept-Language':'ru'}
  self.rfile=io.BytesIO(raw);self.wfile=io.BytesIO();self.client_address=('127.0.0.1',123);self.extra_cookie=None
 def respond(self,value,code=200):self.result=value;self.code=code
 def run(self):
  if self.command=='GET':self.do_GET()
  else:self.do_POST()
  return self
class AdminTests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.db,self.cfg=server.DB,admin.CONFIG
  server.DB=Path(self.tmp.name)/'test.sqlite';admin.CONFIG=Path(self.tmp.name)/'config.json';salt='ab'*16
  admin.CONFIG.write_text(json.dumps({'path':'/private-test','salt':salt,'hash':hashlib.pbkdf2_hmac('sha256',b'pass',bytes.fromhex(salt),600000).hex()}));admin.SESSIONS.clear();admin.ATTEMPTS.clear();server.initialize()
 def tearDown(self):server.DB=self.db;admin.CONFIG=self.cfg;self.tmp.cleanup()
 def login(self):
  r=Request('/private-test/login','POST',{'password':'pass'}).run();self.assertEqual(r.code,200);return r.extra_cookie.split(';')[0],r.result['csrf']
 def test_protection(self):
  self.assertEqual(Request('/private-test/data').run().code,401)
  self.assertEqual(Request('/private-test/login','POST',{'password':'pass'},origin='https://evil.example').run().code,403)
  cookie,csrf=self.login();self.assertEqual(Request('/private-test/moderate','POST',{},cookie).run().code,403)
  self.assertEqual(Request('/private-test/data',cookie=cookie).run().code,200)
  Request('/private-test/logout','POST',{},cookie,csrf).run();self.assertEqual(Request('/private-test/data',cookie=cookie).run().code,401)
 def test_moderation_and_statistics(self):
  visit=Request('/api/visit','POST').run();vc=visit.extra_cookie.split(';')[0]
  note=Request('/api/notes','POST',{'id':'test-note-123456789','nickname':'Эльф','message':'С Новым годом!'},vc).run();self.assertEqual(note.code,201)
  self.assertEqual(Request('/api/tree').run().result['notes'],[]);cookie,csrf=self.login()
  for action,expected in [('feature',1),('unfeature',0),('feature',1),('reject',0)]:
   r=Request('/private-test/moderate','POST',{'kind':'notes','id':note.result['id'],'action':action},cookie,csrf).run();self.assertEqual(r.code,200);self.assertEqual(len(Request('/api/tree').run().result['notes']),expected)
  self.assertEqual(Request('/api/tree').run().result['balls'],[])
  s=Request('/api/strokes','POST',{'id':'test-stroke-123456','color':'#ffffff','width':4,'points':[[1,2],[10,20]]},vc).run();self.assertEqual(s.code,201)
  for action,expected in [('approve',1),('reject',0)]:
   Request('/private-test/moderate','POST',{'kind':'strokes','id':s.result['seq'],'action':action},cookie,csrf).run();self.assertEqual(len(Request('/api/strokes').run().result['strokes']),expected)
  data=Request('/private-test/data',cookie=cookie).run().result;self.assertEqual(data['stats'],{'views':1,'visitors':1});self.assertEqual(len(data['submissions']),2)
 def test_lockout(self):
  for _ in range(5):self.assertEqual(Request('/private-test/login','POST',{'password':'bad'}).run().code,401)
  self.assertEqual(Request('/private-test/login','POST',{'password':'pass'}).run().code,429)
if __name__=='__main__':unittest.main()
