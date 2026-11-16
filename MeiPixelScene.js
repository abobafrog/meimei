/* A crisp, responsive pixel winter landscape; one scene behind the hero. */
(()=>{
[document.getElementById('mei-pixel-art')].filter(Boolean).forEach(canvas=>{
const ctx=canvas.getContext('2d'),reduce=matchMedia('(prefers-reduced-motion: reduce)');let frame=0,visible=true,w=200,h=150;
const box=(x,y,a,b,c)=>{ctx.fillStyle=c;ctx.fillRect(Math.round(x),Math.round(y),Math.ceil(a),Math.ceil(b))};
const pixelGlyphs={
  ' ':['00000','00000','00000','00000','00000','00000','00000'],
  '-':['00000','00000','00000','11111','00000','00000','00000'],
  '6':['01110','10000','10000','11110','10001','10001','01110'],
  '7':['11111','00001','00010','00100','01000','01000','01000'],
  'а':['00000','01110','00001','01111','10001','10011','01101'],
  'б':['00111','01000','01000','01110','01001','01001','01110'],
  'в':['00000','11110','10001','11110','10001','10001','11110'],
  'г':['00000','00000','11111','10000','10000','10000','10000'],
  'д':['00000','00110','01010','01010','01010','11111','10001'],
  'и':['10001','10011','10101','10101','11001','10001','10001'],
  'к':['10000','10000','10010','10100','11000','10100','10010'],
  'л':['00000','00110','01010','01010','01010','10001','10001'],
  'н':['10001','10001','10001','11111','10001','10001','10001'],
  'о':['00000','01110','10001','10001','10001','10001','01110'],
  'р':['00000','10110','11001','10001','11001','10110','10000'],
  'с':['00000','01111','10000','10000','10000','10000','01111'],
  'т':['00000','11111','00100','00100','00100','00100','00100'],
  'ч':['10001','10001','10001','01111','00001','00001','00001']
};
function pixelTextWidth(text){return Math.max(0,Array.from(text).length*6-1)}
function drawPixelText(text,x,centerY,color,alpha){const top=Math.round(centerY-3.5),left=Math.round(x);ctx.globalAlpha=alpha;Array.from(text.toLowerCase()).forEach((character,index)=>{const rows=pixelGlyphs[character]||pixelGlyphs[' '];rows.forEach((row,y)=>{for(let column=0;column<5;column++){if(row[column]==='1'){const px=left+index*6+column,py=top+y;box(px+1,py+1,1,1,'#061321');box(px,py,1,1,color)}}})})}
function flybyWords(t){ctx.save();const words=['6-7','Галда','Коч братан','Кстати бтв'],period=24,duration=7;words.forEach((word,i)=>{const phase=(t+i*6)%period;if(reduce.matches||phase<duration){const width=pixelTextWidth(word),x=reduce.matches?Math.round(w*(.12+(i%2)*.42)):Math.round((w+width)*phase/duration-width),y=Math.round(h*(.12+(i%4)*.115));drawPixelText(word,x,y,i===0?'#ffe29a':'#d5f0ff',reduce.matches?.85:.95)}});ctx.restore()}
function fir(x,y,size,front=false){const colors=front?['#173d44','#24565b','#3c7476']:['#142c3b','#1e3d4c','#345469'];for(let tier=0;tier<5;tier++){const top=y+tier*size*2;for(let row=0;row<4;row++){const width=(tier*1.6+row+1)*size;box(x-width/2,top+row*size,width,size,colors[row%3])}box(x-(tier*1.6+2)*size/2,top+size,(tier*1.6+2)*size,size,'#83a6b5')}box(x-size/2,y+size*12,size,size*2,'#3a4851')}
function draw(now){const t=reduce.matches?0:now/1000;box(0,0,w,h,'#0a1423');box(0,h*.43,w,h*.57,'#101f30');box(0,h*.68,w,h*.32,'#192f42');
for(let i=1;i<45;i++){const x=(i*71+19)%w,y=(i*31+7)%Math.max(1,Math.floor(h*.65));box(x,y,1,1,Math.sin(t+i)>.3?'#a2c8da':'#49627d')}
flybyWords(t);
const moon=Math.min(w-25,Math.round(w*.82));
for(let row=-9;row<=9;row++){const span=Math.floor(Math.sqrt(100-row*row));box(moon-span,26+row,span*2+1,1,'#e0eadc')}
box(moon-4,22,3,2,'#b7cbd0');box(moon+3,28,4,3,'#c1d3d3');box(moon-5,30,2,2,'#c1d3d3');
// A brief shooting star, then a long quiet pause.
const shooting=t%19;if(shooting<1.5&&!reduce.matches){const sx=w*.12+shooting*w*.35,sy=12+shooting*14;for(let k=0;k<9;k++)box(sx-k*2,sy-k,1,1,k<3?'#edfaff':'#52758b')}
// Santa and three reindeer cross the sky occasionally; all details share the pixel grid.
const flight=(t+10)%48;if(flight<14&&!reduce.matches){const sx=-70+(w+150)*flight/14,sy=40+Math.round(Math.sin(flight*.65)*3);for(let deer=0;deer<3;deer++){const dx=sx+24+deer*17,dy=sy-3;box(dx,dy,9,4,'#bd9165');box(dx+8,dy-5,3,7,'#bd9165');box(dx+10,dy-4,3,3,'#e8c698');box(dx+9,dy-8,1,3,'#ebd5a1');box(dx+7,dy-8,3,1,'#ebd5a1');const step=Math.floor(t*6)%2;box(dx+step*2,dy+4,1,3,'#c8a57b');box(dx+7-step*2,dy+4,1,3,'#c8a57b');box(dx-7,dy+2,7,1,'#a99069')}box(sx,sy+3,17,5,'#bc435e');box(sx+14,sy,3,5,'#df6376');box(sx-2,sy+9,21,1,'#edcb85');box(sx+18,sy+7,2,2,'#edcb85');box(sx+5,sy-4,6,7,'#d84c66');box(sx+5,sy-8,5,4,'#efd1a6');box(sx+8,sy-6,3,4,'#f3f5ed');box(sx+4,sy-10,7,2,'#d84c66');box(sx-3,sy-1,6,5,'#4c927e')}

for(let i=0;i<Math.ceil(w/15)+1;i++)fir(i*15,h*.72-8+(i%3)*4,1.2);
for(let i=0;i<w;i+=6)box(i,h*.88+Math.round(Math.sin(i*.04)*3),6,h,'#385366');
// Little chalet, tucked below the copy, with snow-covered eaves.
const x=Math.round(w*.18),y=Math.round(h*.84);box(x-13,y-17,27,21,'#614c55');for(let j=0;j<5;j++)box(x-13,y-15+j*4,27,1,'#816064');
for(let j=0;j<9;j++){box(x-18+j*2,y-17-j,36-j*4,1,'#acc8d4');box(x-16+j*2,y-15-j,32-j*4,1,'#476378')}
box(x-10,y-12,6,7,'#ffc681');box(x+5,y-12,6,7,'#ffc681');box(x-8,y-12,1,7,'#a06854');box(x+7,y-12,1,7,'#a06854');box(x-2,y-7,5,11,'#303d51');box(x-17,y+4,35,2,'#c0d7df');
for(let i=0;i<3;i++){const rise=(t*3+i*5)%15;box(x-8+Math.round(Math.sin(i+t)),y-28-rise,3,2,'#607789')}
fir(w*.42,h*.76,1.8,true);fir(w-7,h*.78,2,true);fir(6,h*.84,1.6,true);
for(let i=0;i<12;i++){const yy=h*.76+i*1.6,xx=w*.42+Math.sin(i*2)*i*.65;box(xx-1,yy-1,2,2,['#ffd789','#f2869a','#9ddcda'][i%3])}
if(visible&&!document.hidden&&!reduce.matches)frame=requestAnimationFrame(draw)}
function start(){cancelAnimationFrame(frame);if(visible&&!document.hidden)frame=requestAnimationFrame(draw)}
new ResizeObserver(()=>{const r=canvas.getBoundingClientRect();w=Math.max(80,Math.round(r.width/4));h=Math.max(100,Math.round(r.height/4));canvas.width=w;canvas.height=h;ctx.imageSmoothingEnabled=false;start()}).observe(canvas);
new IntersectionObserver(e=>{visible=e[0].isIntersecting;start()}).observe(canvas);reduce.addEventListener('change',start);document.addEventListener('visibilitychange',start);start();
});
})();
