(() => {
 const reduced=matchMedia('(prefers-reduced-motion: reduce)');
 const entrance=document.querySelector('.mei-entrance');
 const paw=document.createElement('img');paw.src='mei-paw.png';paw.alt='';paw.className='mei-paw';entrance.append(paw);
 const cat=document.getElementById('portrait-frame');
 const group=document.createElement('div');group.className='mini-meis';group.setAttribute('aria-hidden','true');document.body.append(group);
 const announcement=document.createElement('span');announcement.className='sr-only';announcement.setAttribute('role','status');document.body.append(announcement);
 const preload=new Image();preload.src='mini-mei-holiday.png';preload.decode?.().catch(()=>{});
 let timer,frame=0,last=0,actors=[],clicks=0;
 function stop(){cancelAnimationFrame(frame);actors=[];group.replaceChildren()}
 function animate(now){const dt=Math.min((now-last)/1000||0,.035);last=now;for(const a of actors){a.vy+=1100*dt;a.x+=a.vx*dt;a.y+=a.vy*dt;a.angle+=a.spin*dt;a.img.style.transform=`translate3d(${a.x}px,${a.y}px,0) rotate(${a.angle}deg)`;if(a.y>innerHeight+160)a.img.remove()}actors=actors.filter(a=>a.img.isConnected);if(actors.length)frame=requestAnimationFrame(animate)}
 function summon(){clearTimeout(timer);cancelAnimationFrame(frame);clicks++;const base=Math.max(4,Math.min(16,Math.round(innerWidth*innerHeight/100000)));const count=Math.min(60,base+(clicks-1)*3);while(actors.length+count>120){const old=actors.shift();old?.img.remove();}const bounds=cat.getBoundingClientRect(),w=innerWidth<600?68:94;for(let i=0;i<count;i++){const img=document.createElement('img');img.src=preload.src;img.alt='';img.className='mini-mei roaming';img.style.width=w+'px';const x=Math.max(0,Math.min(innerWidth-w,bounds.left+bounds.width/2-w/2+(Math.random()-.5)*80)),y=Math.min(innerHeight-w*1.35,Math.max(0,bounds.top+bounds.height*.4));img.style.transform=`translate3d(${x}px,${y}px,0)`;group.append(img);actors.push({img,x,y,vx:(Math.random()-.5)*450,vy:-430-Math.random()*230,angle:0,spin:(Math.random()-.5)*180})}announcement.textContent=`Появились маленькие Мей-Мей: ${count}.`;last=performance.now();if(!reduced.matches)frame=requestAnimationFrame(animate);timer=setTimeout(stop,reduced.matches?2000:3000)}
 reduced.addEventListener('change',stop);document.addEventListener('visibilitychange',()=>{if(document.hidden)stop()});
 cat.addEventListener('click',summon);cat.addEventListener('keydown',e=>{if(!e.repeat&&(e.key==='Enter'||e.key===' ')){e.preventDefault();summon()}});
 entrance.addEventListener('animationend',e=>{if(e.target===entrance)entrance.remove()});
})();
