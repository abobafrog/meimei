(() => {
 const local=location.protocol==='file:';window.froklaLocal=local;
 const key='frokla-local-v1';
 const seedKey='frokla-test-wishes-seeded-v1';
 const authors=['Снежинка Лея','Зимний эльф','Мей-Мей','Полярная звезда','Лесной олень','Снежный кот','Мандаринка','Тихий снег','Ёлочный шар','Северный ветер'];
 const wishes=['Пусть в новом году сбывается то, о чём давно мечтаешь.','Желаю больше тёплых встреч и поводов улыбаться.','Пусть рядом будут любимые люди и уютный дом.','Пусть каждый день приносит маленькое чудо.','Желаю вдохновения, спокойствия и добрых новостей.','Пусть мечты находят дорогу к тебе.','Побольше смеха, снежных вечеров и какао.','Пусть новый год будет светлым и щедрым на радость.','Желаю смелости начать то, что давно откладывал.','Пусть счастье тихонько заглянет в каждый день.'];
 function seedTestWishes(){if(!local||localStorage.getItem(seedKey))return;const data=read();const stamp=Date.now();for(let i=0;i<100;i++){const id='test-wish-'+(i+1);data.notes.push({id:data.notes.length+1,requestId:id,nickname:authors[i%authors.length],message:wishes[(i*7+Math.floor(i/10))%wishes.length],approved:true,featured:true,rejected:false,hue:(i*137.507764+18)%360,testData:true,createdAt:stamp+i})}write(data);localStorage.setItem(seedKey,'1')}
 seedTestWishes();
 function read(){return JSON.parse(localStorage.getItem(key)||'{"strokes":[],"notes":[]}')}
 function write(data){localStorage.setItem(key,JSON.stringify(data))}
 window.froklaRequest=async(url,options={})=>{
  if(!local)return fetch(url,options);
  try{const data=read(),path=url.split('?')[0];let result;data.quota??={notes:data.notes.filter(n=>!n.testData).length};if(data.quota.ink!==undefined)delete data.quota.ink;const limits=()=>({notesRemaining:Math.max(0,2-data.quota.notes),notesLimit:2});const denied=error=>({ok:false,status:429,json:async()=>({error,limits:limits()})});
   if(options.method==='PUT'&&path.startsWith('/api/strokes/')){const id=decodeURIComponent(path.slice('/api/strokes/'.length));const row=data.strokes.find(s=>s.id===id);if(!row)return {ok:false,status:404,json:async()=>({error:'Рисунок не найден'})};const body=JSON.parse(options.body),segments=Array.isArray(body.segments)?body.segments:[];if(!segments.some(path=>Array.isArray(path)&&path.length)){data.strokes=data.strokes.filter(stroke=>stroke.id!==id);write(data);result={ok:true,id,deleted:true,limits:limits()}}else{row.segments=segments;row.points=segments[0];write(data);result={ok:true,id,segments:row.segments,limits:limits()}}window.dispatchEvent(new Event('local-drawings'))}
   else if(options.method==='POST'){
    const body=JSON.parse(options.body);
    if(path==='/api/strokes'){let row=data.strokes.find(s=>s.id===body.id);if(!row){data.nextStrokeSeq??=Math.max(0,...data.strokes.map(s=>Number(s.seq)||0))+1;row={...body,seq:data.nextStrokeSeq++,approved:1,auto_status:"not_required"};data.strokes.push(row);write(data)}result={...row,limits:limits()};window.dispatchEvent(new Event('local-drawings'))}
    else if(path==='/api/notes'){let row=data.notes.find(s=>s.requestId===body.id);if(!row){if(limits().notesRemaining<1)return denied('В этом браузере уже отправлены два послания.');data.quota.notes++;row={id:data.notes.length+1,requestId:body.id,nickname:body.nickname,message:body.message,approved:false,hue:(data.notes.length*137.507764+18)%360};data.notes.push(row);write(data)}result={id:row.id,pending:true};window.dispatchEvent(new Event('local-notes'))}
    else throw Error();
   }else if(path==='/api/strokes/mine'){const after=Number(new URLSearchParams(url.split('?')[1]).get('after')||0);const rows=data.strokes.filter(s=>s.approved!==false&&s.seq>after);result={strokes:rows.slice(0,2000),incremental:after>0,hasMore:rows.length>2000}}
   else if(path==='/api/strokes'){const after=Number(new URLSearchParams(url.split('?')[1]).get('after')||0);const rows=data.strokes.filter(s=>s.approved!==false&&s.seq>after);result={strokes:rows.slice(0,2000),limits:limits(),incremental:after>0,hasMore:rows.length>2000}}
   else if(path==='/api/limits')result=limits();
   else if(path==='/api/tree')result={balls:data.notes.map(({id,hue,nickname})=>({id,hue,nickname})),messageCount:data.notes.length,notes:data.notes.filter(s=>s.approved&&!s.rejected&&s.featured).map(({nickname,message})=>({nickname,message}))};
   else throw Error();
   return {ok:true,json:async()=>result};
  }catch{return {ok:false,json:async()=>({error:'Local storage unavailable or full'})}}
 };
 window.froklaVisitReady=local?Promise.resolve():fetch('/api/visit',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({source:document.referrer?new URL(document.referrer).origin:''})}).catch(()=>{});
})();
