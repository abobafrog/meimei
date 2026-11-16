(()=>{
const input=document.getElementById('ink');if(!input)return;
const label=input.closest('label'),root=document.createElement('div');root.className='ink-picker';
label.replaceWith(root);label.className='ink-custom';label.firstChild.textContent='Свой цвет ';root.append(label);
const defaults=['#a9d9ff','#ffffff','#183e59','#f17994','#ffcf73','#76c6aa','#b294e5','#181b26'];
let recent=[];try{recent=JSON.parse(localStorage.getItem('frokla-recent-ink')||'[]').filter(c=>/^#[0-9a-f]{6}$/i.test(c)).slice(0,6)}catch{}
const palette=document.createElement('div');palette.className='ink-swatches';palette.setAttribute('role','group');palette.setAttribute('aria-label','Палитра');root.prepend(palette);
const history=document.createElement('div');history.className='ink-recent';root.append(history);
function swatch(color){const b=document.createElement('button');b.type='button';b.className='ink-swatch';b.style.setProperty('--ink',color);b.setAttribute('aria-label','Цвет '+color);b.setAttribute('aria-pressed',String(input.value.toLowerCase()===color.toLowerCase()));b.onclick=()=>{input.value=color;input.dispatchEvent(new Event('input',{bubbles:true}));};return b}
function render(){palette.replaceChildren(...defaults.map(swatch));history.replaceChildren();history.hidden=!recent.length;if(recent.length){const title=document.createElement('span');title.textContent='Последние';history.append(title,...recent.map(swatch))}}
function remember(){const c=input.value.toLowerCase();recent=[c,...recent.filter(v=>v.toLowerCase()!==c)].slice(0,6);try{localStorage.setItem('frokla-recent-ink',JSON.stringify(recent))}catch{}render()}
input.addEventListener('input',()=>{for(const b of root.querySelectorAll('.ink-swatch'))b.setAttribute('aria-pressed',String(b.style.getPropertyValue('--ink').toLowerCase()===input.value.toLowerCase()))});
input.addEventListener('change',remember);document.getElementById('drawing-board').addEventListener('pointerdown',()=>{if(document.querySelector('.canvas-viewport').dataset.mode==='draw')remember()});render();
})();
