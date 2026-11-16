import unittest, tempfile, time, json, io, os
from pathlib import Path
from unittest.mock import patch
import admin, server, analytics
from test_admin import Request
from client_ip import address

class ImmediatePool:
 def submit(self,fn):fn()

class AnalyticsTests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.db,self.cfg=server.DB,admin.CONFIG
  server.DB=Path(self.tmp.name)/'db';admin.CONFIG=Path(self.tmp.name)/'cfg'
  admin.CONFIG.write_text(json.dumps({'path':'/private-test'}));server.initialize()
  admin.SESSIONS['testsession']={'expires':time.time()+100,'csrf':'testcsrf'}
 def tearDown(self):
  server.DB,admin.CONFIG=self.db,self.cfg;admin.SESSIONS.clear();self.tmp.cleanup()
 def req(self,path,body=None,cookie='',ip='127.0.0.1'):
  r=Request(path,'POST' if body is not None else 'GET',body,cookie);r.client_address=(ip,10);return r.run()
 def private(self,path):return self.req('/private-test'+path,cookie='frokla_admin=testsession')
 def test_private_map_endpoints(self):
  for route in ['/analytics','/connections','/map.js','/map.css']:
   self.assertEqual(self.req('/private-test'+route).code,401)
  self.assertEqual(self.private('/analytics?days=abc').code,400)
  self.assertEqual(self.private('/connections?lat=nan&lon=1').code,400)
 def test_local_connections_and_opt_in(self):
  with patch.object(analytics,'schedule') as schedule:
   visit=self.req('/api/visit',{'source':'https://example.com/path?private=1'});schedule.assert_not_called()
  cookie=visit.extra_cookie.split(';')[0];rows=self.private('/connections?unknown=1').result['visits']
  self.assertEqual(rows[0]['geo_status'],'local');self.assertEqual(rows[0]['source'],'example.com');self.assertEqual(rows[0]['interests'],[])
  self.assertEqual(self.req('/api/interests',{'interests':['art','animals']},cookie).code,200)
  data=self.private('/analytics').result;self.assertEqual(data['respondents'],1);self.assertEqual(data['locations'],[])
  self.assertEqual(self.req('/api/interests',{'interests':['unapproved-category']},cookie).code,400)
  self.assertEqual(self.req('/api/interests',{'interests':[]},cookie).code,200)
  self.assertEqual(self.private('/analytics').result['respondents'],0)
  self.assertEqual(self.req('/api/interests',{'interests':['art']}).code,401)
 def test_geo_cache_and_stats(self):
  geo=dict(geo_status='located',country='Example',region='Region',city='City',lat=40.12,lon=25.42,network='Network')
  with patch.object(analytics,'POOL',ImmediatePool()),patch.object(analytics,'lookup',return_value=geo) as lookup:
   self.req('/api/visit',{},ip='8.8.8.8');self.req('/api/visit',{},ip='8.8.8.8');self.assertEqual(lookup.call_count,1)
  data=self.private('/analytics?days=7').result;self.assertEqual(data['totals']['located'],2);self.assertEqual(data['locations'][0]['views'],2)
  rows=self.private('/connections?lat=40.12&lon=25.42').result['visits'];self.assertEqual(len(rows),2)
  with server.connect() as con:
   self.assertNotEqual(con.execute('SELECT peer FROM visits LIMIT 1').fetchone()[0],'8.8.8.8')
 def test_geo_failure(self):
  with patch.object(analytics,'POOL',ImmediatePool()),patch.object(analytics,'lookup',side_effect=OSError('offline')):
   self.assertEqual(self.req('/api/visit',{},ip='8.8.8.8').code,200)
  self.assertEqual(self.private('/connections?unknown=1').result['visits'][0]['geo_status'],'unavailable')
 def test_invalid_coordinates(self):
  with patch.object(analytics,'urlopen',return_value=io.BytesIO(json.dumps({'success':True,'latitude':999,'longitude':0}).encode())):
   with self.assertRaises(ValueError):analytics.lookup('8.8.8.8')
 def test_proxy_trust(self):
  h=Request('/api/visit');h.headers['X-Forwarded-For']='8.8.8.8'
  with patch.dict(os.environ,{'TRUSTED_PROXIES':''}):self.assertEqual(str(address(h)),'127.0.0.1')
  with patch.dict(os.environ,{'TRUSTED_PROXIES':'127.0.0.1/32'}):self.assertEqual(str(address(h)),'8.8.8.8')
 def test_retention(self):
  with server.connect() as con:
   con.execute('INSERT INTO visits(visitor,created) VALUES(?,?)',('old',int(time.time())-91*86400))
   con.execute('INSERT INTO visitor_interests VALUES(?,?,?)',('old','["art"]',int(time.time())-91*86400))
  self.req('/api/visit',{})
  with server.connect() as con:
   self.assertEqual(con.execute('SELECT COUNT(*) FROM visits WHERE visitor="old"').fetchone()[0],0)
   self.assertEqual(con.execute('SELECT COUNT(*) FROM visitor_interests').fetchone()[0],0)
