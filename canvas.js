(() => {
 const board=document.getElementById('drawing-board'),ctx=board.getContext('2d'),status=document.getElementById('drawing-status');
 const viewport=document.querySelector('.canvas-viewport'),panButton=document.getElementById('canvas-pan'),eraserButton=document.getElementById('canvas-eraser');
 let pan=false,erasing=false,panPoint=null,erasingBusy=false;
 function updateMode(){
  panButton.setAttribute('aria-pressed',String(pan));
  panButton.textContent=pan?'Режим: перемещение':'Режим: рисование';
  eraserButton.setAttribute('aria-pressed',String(erasing));
  document.getElementById('brush-label').textContent=erasing?'Размер ластика':'Кисть';
  board.style.cursor=pan?'grab':erasing?'cell':'crosshair';
  viewport.classList.toggle('pan-mode',pan);
  viewport.classList.toggle('eraser-mode',erasing);
  viewport.dataset.mode=pan?'pan':erasing?'erase':'draw';
 }
 panButton.onclick=()=>{pan=!pan;if(pan)erasing=false;updateMode()};
 eraserButton.onclick=()=>{erasing=!erasing;if(erasing)pan=false;updateMode();status.textContent=erasing?'Нажми на свой штрих, чтобы стереть его.':'Общий холст · рисунки появляются сразу'};
 viewport.dataset.mode='draw';
 viewport.addEventListener('wheel',e=>{if(!pan)e.preventDefault()},{passive:false});
 viewport.addEventListener('touchmove',e=>{if(!pan)e.preventDefault()},{passive:false});
 viewport.addEventListener('keydown',e=>{if(!pan&&['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','PageUp','PageDown','Home','End',' '].includes(e.key))e.preventDefault()});
 let active=null,eraseStroke=null,ready=false,polling=false,cursor=0,lastReconcile=0,lastMineSync=0,mineReady=false,mineAuthorized=false,pending=[];
 const strokes=new Map(),own=new Map(),mine=new Map();
 function paintPath(path,s){if(!path?.length)return;ctx.strokeStyle=s.color;ctx.fillStyle=s.color;ctx.lineWidth=s.width;ctx.lineCap='round';ctx.lineJoin='round';ctx.beginPath();if(path.length===1){ctx.arc(path[0][0],path[0][1],s.width/2,0,Math.PI*2);ctx.fill();return}ctx.moveTo(...path[0]);for(const p of path.slice(1))ctx.lineTo(...p);ctx.stroke()}
 function paint(s){const paths=s.segments===undefined||s.segments===null?[s.points]:s.segments;for(const path of paths)paintPath(path,s)}
 function render(){ctx.clearRect(0,0,board.width,board.height);for(const s of strokes.values())paint(s);for(const s of own.values())if(!strokes.has(s.id))paint(s);for(const s of pending)paint(s);if(active)paint(active);if(eraseStroke?.points.length){const p=eraseStroke.points.at(-1),radius=eraserRadius();ctx.save();ctx.strokeStyle='#d4ecffcc';ctx.lineWidth=1.2*4800/board.getBoundingClientRect().width;ctx.beginPath();ctx.arc(p[0],p[1],radius,0,Math.PI*2);ctx.stroke();ctx.restore()}}
 async function refreshMine(){
  try{
   const fresh=new Map();let after=0;
   do{const r=await window.froklaRequest('/api/strokes/mine?after='+after,{cache:'no-store'});if(!r.ok){lastMineSync=Date.now();if(r.status===401){mine.clear();mineAuthorized=false;mineReady=true}return false}const data=await r.json();let pageCursor=after;for(const s of data.strokes){fresh.set(s.id,s);pageCursor=Math.max(pageCursor,s.seq)}if(!data.hasMore||pageCursor===after)break;after=pageCursor}while(true);
   for(const id of mine.keys())if(!fresh.has(id)){mine.delete(id);own.delete(id)}
   for(const [id,s]of fresh){mine.set(id,s);strokes.set(id,s)}
   mineReady=true;mineAuthorized=true;lastMineSync=Date.now();render();return true
  }catch{lastMineSync=Date.now();return false}
 }
 async function sync(){
  if(polling||document.hidden||eraseStroke||erasingBusy)return;polling=true;
  try{
   await(window.froklaVisitReady||Promise.resolve());
   const full=cursor===0||Date.now()-lastReconcile>30000;let after=full?0:cursor,first=true;
   do{const r=await window.froklaRequest('/api/strokes?after='+after,{cache:'no-store'});if(!r.ok)throw Error();const data=await r.json();if(first&&!data.incremental){strokes.clear();lastReconcile=Date.now()}first=false;let pageCursor=after;for(const s of data.strokes){strokes.set(s.id,s);pageCursor=Math.max(pageCursor,s.seq);cursor=Math.max(cursor,s.seq)}if(!data.hasMore||pageCursor===after)break;after=pageCursor}while(true);
   if(!mineReady||Date.now()-lastMineSync>30000)await refreshMine();
   ready=true;eraserButton.disabled=!mineAuthorized;status.textContent=window.froklaLocal?'Холст сохранён в этом браузере':pending.length?'Есть неотправленные штрихи. Повторяем отправку…':'Общий холст · рисунки появляются сразу';render();
  }catch{ready=false;status.textContent='Нет связи с общим холстом. Для совместного рисования открой сайт через сервер'}finally{polling=false}
 }
 let sending=false;
 async function flush(){if(sending||!pending.length)return;sending=true;try{await(window.froklaVisitReady||Promise.resolve());while(pending.length){const s=pending[0],r=await window.froklaRequest('/api/strokes',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(s)}),saved=await r.json();if(!r.ok){if(r.status===429){pending=[];active=null;status.textContent=saved.error;return}if(r.status>=400&&r.status<500){pending.shift();status.textContent='Штрих отклонён сервером';continue}throw Error()}strokes.set(saved.id,saved);pending.shift();receiveLimits(saved.limits)}await refreshMine();status.textContent=window.froklaLocal?'Холст сохранён в этом браузере':'Рисунок опубликован на общем холсте'}catch{status.textContent=window.froklaLocal?'Не сохранено: хранилище недоступно или заполнено':'Штрих пока не сохранён. Отправка повторится'}finally{sending=false;render()}}
 function point(e){const r=board.getBoundingClientRect();return[Math.max(0,Math.min(4800,(e.clientX-r.left)*4800/r.width)),Math.max(0,Math.min(2800,(e.clientY-r.top)*2800/r.height))]}
 function segmentDistance(p,a,b){const dx=b[0]-a[0],dy=b[1]-a[1],length=dx*dx+dy*dy;if(!length)return Math.hypot(p[0]-a[0],p[1]-a[1]);const t=Math.max(0,Math.min(1,((p[0]-a[0])*dx+(p[1]-a[1])*dy)/length));return Math.hypot(p[0]-(a[0]+t*dx),p[1]-(a[1]+t*dy))}
 function compactErasePath(points){if(points.length<=512)return points;const keep=new Uint8Array(points.length);keep[0]=keep[points.length-1]=1;const stack=[[0,points.length-1]],tolerance=eraserRadius()*.2;while(stack.length){const[start,end]=stack.pop();let farthest=0,index=-1;for(let i=start+1;i<end;i++){const distance=segmentDistance(points[i],points[start],points[end]);if(distance>farthest){farthest=distance;index=i}}if(index>=0&&farthest>tolerance){keep[index]=1;stack.push([start,index],[index,end])}}let reduced=points.filter((_,i)=>keep[i]);if(reduced.length>512){const stride=Math.ceil((reduced.length-1)/511);reduced=reduced.filter((_,i)=>i%stride===0);if(reduced.at(-1)!==points.at(-1))reduced.push(points.at(-1))}return reduced}
 function eraserRadius(){return(.8+Number(document.getElementById('brush').value)*.35)*4800/board.getBoundingClientRect().width}
 function erasedAt(p,erasePoints,radius){if(erasePoints.length===1)return Math.hypot(p[0]-erasePoints[0][0],p[1]-erasePoints[0][1])<=radius;for(let i=1;i<erasePoints.length;i++)if(segmentDistance(p,erasePoints[i-1],erasePoints[i])<=radius)return true;return false}
 function lerpPoint(a,b,t){return[a[0]+(b[0]-a[0])*t,a[1]+(b[1]-a[1])*t]}
 function clipPath(path,erasePoints,radius){
  if(!path.length)return[];
  if(path.length===1)return erasedAt(path[0],erasePoints,radius)?[]:[[path[0]]];
  const result=[];let current=null;
  const append=(list,p)=>{const last=list.at(-1);if(!last||Math.hypot(last[0]-p[0],last[1]-p[1])>.01)list.push(p)};
  for(let segment=1;segment<path.length;segment++){
   const a=path[segment-1],b=path[segment],length=Math.hypot(b[0]-a[0],b[1]-a[1]),steps=Math.max(1,Math.ceil(length/Math.max(6,radius*.25))),cuts=[0];let previousT=0,previousHit=erasedAt(a,erasePoints,radius);
   for(let i=1;i<=steps;i++){const t=i/steps,hit=erasedAt(lerpPoint(a,b,t),erasePoints,radius);if(hit!==previousHit){let lo=previousT,hi=t;for(let n=0;n<12;n++){const mid=(lo+hi)/2;if(erasedAt(lerpPoint(a,b,mid),erasePoints,radius)===previousHit)lo=mid;else hi=mid}cuts.push((lo+hi)/2)}previousT=t;previousHit=hit}
   cuts.push(1);
   for(let i=1;i<cuts.length;i++){const from=cuts[i-1],to=cuts[i];if(to-from<1e-6)continue;const outside=!erasedAt(lerpPoint(a,b,(from+to)/2),erasePoints,radius),start=lerpPoint(a,b,from),end=lerpPoint(a,b,to);if(outside){if(!current)current=[];append(current,start);append(current,end)}else if(current){result.push(current);current=null}}
  }
  if(current)result.push(current);return result
 }
 function bounds(paths){let left=Infinity,top=Infinity,right=-Infinity,bottom=-Infinity;for(const path of paths)for(const p of path){left=Math.min(left,p[0]);top=Math.min(top,p[1]);right=Math.max(right,p[0]);bottom=Math.max(bottom,p[1])}return{left,top,right,bottom}}
 function overlaps(a,b){return a.left<=b.right&&a.right>=b.left&&a.top<=b.bottom&&a.bottom>=b.top}
 function applyErasePreview(){
  const session=eraseStroke;if(!session)return;session.preview.clear();session.tooComplex=false;const radius=eraserRadius(),eraseBounds=bounds([session.points]);eraseBounds.left-=radius;eraseBounds.top-=radius;eraseBounds.right+=radius;eraseBounds.bottom+=radius;
  for(const [id,original]of session.base){const base=original.segments===undefined||original.segments===null?[original.points]:original.segments,box=bounds(base);box.left-=original.width/2;box.top-=original.width/2;box.right+=original.width/2;box.bottom+=original.width/2;if(!overlaps(box,eraseBounds))continue;const segments=base.flatMap(path=>clipPath(path,session.points,radius+original.width/2));const pointCount=segments.reduce((total,path)=>total+path.length,0);if(segments.length>1801||pointCount>4000){session.tooComplex=true;continue}if(JSON.stringify(segments)!==JSON.stringify(base)){const updated={...original,segments};session.preview.set(id,updated);mine.set(id,updated);strokes.set(id,updated)}}
  for(const [id,original]of session.base)if(!session.preview.has(id)){mine.set(id,original);strokes.set(id,original)}render()
 }
 function startErase(e){if(erasingBusy||!mineAuthorized){status.textContent='Список своих рисунков пока недоступен';return}eraseStroke={pointer:e.pointerId,points:[point(e)],base:new Map(mine),preview:new Map()};applyErasePreview()}
 async function finishErase(){const session=eraseStroke;if(!session)return;eraseStroke=null;render();if(!session.preview.size){status.textContent=session.tooComplex?'Проведи ластиком по этой части короче':'Под кистью нет своих рисунков';return}erasingBusy=true;status.textContent='Стираем фрагменты…';let failed=0;try{for(const[id,updated]of session.preview){const segments=updated.segments.map(path=>path.map(p=>p.map(value=>Math.round(value*10)/10))),payload=window.froklaLocal?{segments}:{path:compactErasePath(session.points),radius:eraserRadius()},r=await window.froklaRequest('/api/strokes/'+encodeURIComponent(id),{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify(payload)});if(!r.ok){failed++;const original=session.base.get(id);mine.set(id,original);strokes.set(id,original);continue}const savedData=await r.json(),savedSegments=window.froklaLocal?segments:savedData.segments;const saved={...updated,segments:savedSegments};mine.set(id,saved);strokes.set(id,saved)}status.textContent=failed||session.tooComplex?'Часть фрагментов не стёрлась. Проведи ластиком по ним короче':'Фрагменты стёрты'}catch{await refreshMine();status.textContent='Не удалось связаться с холстом'}finally{erasingBusy=false;render()}}
 board.addEventListener('pointerdown',e=>{board.setPointerCapture(e.pointerId);if(active||eraseStroke)return;if(!pan)e.preventDefault();if(pan){panPoint={x:e.clientX,y:e.clientY,id:e.pointerId};return}if(erasing){startErase(e);return}if(!ready){status.textContent='Загружаем общий холст…';return}active={id:(crypto.randomUUID?crypto.randomUUID():Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('')),color:document.getElementById('ink').value,width:Number(document.getElementById('brush').value),points:[point(e)],pointer:e.pointerId};render()});
 board.addEventListener('pointermove',e=>{if(panPoint?.id===e.pointerId){viewport.scrollLeft-=e.clientX-panPoint.x;viewport.scrollTop-=e.clientY-panPoint.y;panPoint.x=e.clientX;panPoint.y=e.clientY;return}if(eraseStroke?.pointer===e.pointerId){const next=point(e),last=eraseStroke.points.at(-1);if(Math.hypot(next[0]-last[0],next[1]-last[1])>Math.max(4,eraserRadius()*.18)){eraseStroke.points.push(next);applyErasePreview()}return}if(!active||active.pointer!==e.pointerId)return;if(active.points.length<1800){const next=point(e);active.points.push(next)}render()});
 function finish(e){if(panPoint?.id===e.pointerId)panPoint=null;if(eraseStroke?.pointer===e.pointerId){finishErase();return}if(!active||active.pointer!==e.pointerId)return;delete active.pointer;pending.push(active);active=null;flush()}board.addEventListener('pointerup',finish);board.addEventListener('pointercancel',finish);
 document.getElementById('fullscreen-art').onclick=async()=>{try{if(document.fullscreenElement)await document.exitFullscreen();else await document.querySelector('.drawing').requestFullscreen()}catch{status.textContent='Полноэкранный режим недоступен в этом браузере'}};
 window.addEventListener('local-drawings-reviewed',()=>{own.clear();mine.clear();mineReady=false;sync()});
 window.addEventListener('beforeunload',e=>{if(pending.length||active||eraseStroke||erasingBusy){e.preventDefault();e.returnValue=''}});sync();setInterval(()=>{sync();flush()},2000);
 const cat=document.getElementById('portrait-frame');cat.tabIndex=0;cat.setAttribute('role','button');cat.setAttribute('aria-label','Поздороваться с Мей-Мей');function wave(){cat.classList.remove('cat-wave');void cat.offsetWidth;cat.classList.add('cat-wave')}cat.addEventListener('click',wave);cat.addEventListener('keydown',e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();wave()}});
})();
