(() => {
 const status=document.getElementById('tree-status'),notes=document.getElementById('best-snowflakes');let busy=false,lastNotes='';
 async function refresh(){if(busy)return;busy=true;try{const r=await window.froklaRequest('/api/tree',{cache:'no-store'});if(!r.ok)throw Error();const data=await r.json();window.froklaTreeData=data;window.dispatchEvent(new CustomEvent('tree-data',{detail:data}));status.textContent='Новогодних игрушек: '+data.balls.length;}catch{status.textContent='Не удалось обновить ёлку.'}finally{busy=false}}
 window.addEventListener('tree-update',refresh);refresh();setInterval(()=>{if(!document.hidden)refresh()},5000);
})();
