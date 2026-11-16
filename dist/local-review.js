(() => {
 if(!window.froklaLocal)return;
 const key='frokla-local-v1';
 const read=()=>JSON.parse(localStorage.getItem(key)||'{"strokes":[],"notes":[]}');
 const panel=document.createElement('details');panel.className='local-moderation drawing-review';
 const title=document.createElement('summary');title.textContent='Модерация рисунков · на этом устройстве';
 const content=document.createElement('div');panel.append(title,content);document.getElementById('drawing').append(panel);
 function render(){
  content.replaceChildren();let data;
  try{data=read()}catch{content.textContent='Хранилище недоступно.';return}
  const pending=data.strokes.filter(s=>s.approved!==true&&s.approved!==false);
  if(!pending.length){content.textContent='Нет рисунков на проверке.';return}
  const note=document.createElement('p');note.textContent='Проверь рисунок целиком. Решение применяется ко всем показанным новым штрихам.';
  const canvas=document.createElement('canvas');canvas.width=1200;canvas.height=700;canvas.style.cssText='width:100%;height:auto;background:#101114;border-radius:12px';const ctx=canvas.getContext('2d');ctx.scale(.25,.25);
  for(const s of data.strokes.filter(s=>s.approved!==false)){ctx.strokeStyle=s.color;ctx.fillStyle=s.color;ctx.lineWidth=s.width;ctx.lineCap='round';ctx.lineJoin='round';ctx.beginPath();ctx.arc(...s.points[0],s.width/2,0,Math.PI*2);ctx.fill();ctx.beginPath();ctx.moveTo(...s.points[0]);for(const p of s.points.slice(1))ctx.lineTo(...p);ctx.stroke()}
  const actions=document.createElement('div');actions.className='review-actions';
  for(const [label,approved] of [['Одобрить рисунок',true],['Отклонить рисунок',false]]){const button=document.createElement('button');button.type='button';button.textContent=label;button.onclick=()=>{try{const current=read(),ids=new Set(pending.map(s=>s.id));for(const s of current.strokes)if(ids.has(s.id))s.approved=approved;localStorage.setItem(key,JSON.stringify(current));window.dispatchEvent(new Event('local-drawings-reviewed'));render()}catch{note.textContent='Не удалось сохранить решение.'}};actions.append(button)}
  content.append(note,canvas,actions);
 }
 panel.addEventListener('toggle',()=>{if(panel.open)render()});window.addEventListener('local-drawings',()=>{if(panel.open)render()});render();
})();
