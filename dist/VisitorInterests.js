(()=>{
 const form=document.getElementById('visitor-interests');if(!form)return;const result=form.querySelector('[role=status]'),button=form.querySelector('button');
 try{const saved=JSON.parse(localStorage.getItem('frokla-interests')||'[]');for(const input of form.querySelectorAll('input'))input.checked=saved.includes(input.value)}catch{}
 form.onsubmit=async event=>{event.preventDefault();button.disabled=true;result.textContent='Сохраняем…';const interests=[...form.querySelectorAll('input:checked')].map(i=>i.value);
 try{if(location.protocol!=='file:'){await window.froklaVisitReady;const response=await fetch('/api/interests',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({interests})});const data=await response.json();if(!response.ok)throw Error(data.error||'Не удалось сохранить')}
 localStorage.setItem('frokla-interests',JSON.stringify(interests));result.textContent=location.protocol==='file:'?'Сохранено в этом браузере.':interests.length?'Спасибо! Твои интересы сохранены.':'Выбор интересов удалён.';
 }catch(e){result.textContent=e.message||'Не удалось сохранить. Попробуй позже.'}finally{button.disabled=false}};
})();
