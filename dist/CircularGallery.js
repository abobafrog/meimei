/* Framework-free adaptation of the CircularGallery interaction supplied by the user.
   Uses its circular arc geometry and easing; DOM cards keep wishes readable and accessible. */
(() => {
 const clamp=(n,a,b)=>Math.min(b,Math.max(a,n));
 const wrap=(n,length)=>((n%length)+length)%length;
 class CircularGallery {
  constructor(root,{bend=3,scrollEase=.05,onItemClick=()=>{}}={}){
   this.root=root;this.bend=bend;this.ease=scrollEase;this.onItemClick=onItemClick;this.items=[];this.scroll=0;this.target=0;this.raf=0;this.drag=null;this.visible=true;
   this.reduce=matchMedia('(prefers-reduced-motion: reduce)');
   root.className='circular-gallery';root.setAttribute('role','region');root.setAttribute('aria-roledescription','карусель');root.setAttribute('aria-label','Послания');
   this.stage=document.createElement('div');this.stage.className='circular-gallery-stage';this.stage.tabIndex=0;
   this.controls=document.createElement('div');this.controls.className='circular-gallery-controls';
   this.prev=this.button('←','Предыдущее пожелание',()=>this.move(-1));this.next=this.button('→','Следующее пожелание',()=>this.move(1));this.count=document.createElement('span');this.count.setAttribute('aria-live','polite');this.controls.append(this.prev,this.count,this.next);root.append(this.stage,this.controls);
   this.dialog=document.createElement('dialog');this.dialog.className='wish-dialog';this.dialog.setAttribute('aria-label','Пожелание');this.close=this.button('×','Закрыть пожелание',()=>this.dialog.close());this.name=document.createElement('h3');this.message=document.createElement('p');this.dialog.append(this.close,this.name,this.message);root.append(this.dialog);this.dialog.addEventListener('click',e=>{if(e.target===this.dialog){const r=this.dialog.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)this.dialog.close()}});
   this.stage.addEventListener('pointerdown',e=>{if(e.button!==0||this.items.length<2)return;this.drag={id:e.pointerId,x:e.clientX,y:e.clientY,target:this.target,moved:false}});
   this.stage.addEventListener('pointermove',e=>{const d=this.drag;if(!d||d.id!==e.pointerId)return;const dx=e.clientX-d.x,dy=e.clientY-d.y;if(!d.moved&&Math.abs(dy)>Math.abs(dx)&&Math.abs(dy)>8){this.drag=null;return}if(Math.abs(dx)>6){d.moved=true;this.stage.setPointerCapture(e.pointerId);this.stage.classList.add('dragging')}if(d.moved){this.target=d.target-dx/this.spacing;this.wake()}});
   const end=e=>{if(!this.drag||this.drag.id!==e.pointerId)return;this.suppressClick=this.drag.moved;this.drag=null;this.target=Math.round(this.target);this.stage.classList.remove('dragging');if(this.stage.hasPointerCapture(e.pointerId))this.stage.releasePointerCapture(e.pointerId);this.wake();setTimeout(()=>this.suppressClick=false,0)};
   this.stage.addEventListener('pointerup',end);this.stage.addEventListener('pointercancel',end);
   this.stage.addEventListener('wheel',e=>{if(this.items.length<2||(!e.shiftKey&&Math.abs(e.deltaX)<Math.abs(e.deltaY)))return;e.preventDefault();this.target+=(e.deltaX||e.deltaY)/this.spacing;this.wake();clearTimeout(this.snapTimer);this.snapTimer=setTimeout(()=>{this.target=Math.round(this.target);this.wake()},140)},{passive:false});
   this.stage.addEventListener('keydown',e=>{if(['ArrowLeft','ArrowRight','Home','End'].includes(e.key)){e.preventDefault();if(e.key==='Home')this.target=0;else if(e.key==='End')this.target=this.items.length-1;else this.target=Math.round(this.target)+(e.key==='ArrowRight'?1:-1);if(this.items.length)this.cards[wrap(Math.round(this.target),this.items.length)]?.focus({preventScroll:true});this.wake()}});
   new ResizeObserver(()=>this.wake()).observe(this.stage);new IntersectionObserver(e=>{this.visible=e[0].isIntersecting;if(this.visible)this.wake();else cancelAnimationFrame(this.raf)}).observe(root);this.reduce.addEventListener('change',()=>this.wake());
  }
  button(text,label,fn){const b=document.createElement('button');b.type='button';b.textContent=text;b.setAttribute('aria-label',label);b.onclick=fn;return b}
  setItems(items){if(JSON.stringify(items)===this.signature)return;this.signature=JSON.stringify(items);this.items=items;this.scroll=this.target=0;this.stage.replaceChildren();this.cards=items.map((item,index)=>{
   const button=this.button('',`от: ${item.text}`,()=>{if(this.suppressClick)return;this.open(index)});button.className='circular-wish';button.dataset.sender=item.text;button.style.setProperty('--sender-size',`${Math.max(12,Math.min(22,250/Math.max(item.text.length+4,10)))}px`);button.style.setProperty('--wish-hue',String(197+index*13%45));const star=document.createElement('span');star.className='wish-crystal';star.setAttribute('aria-hidden','true');const crystal=document.createElement('img');crystal.src='postcard-crystal.svg';crystal.alt='';star.append(crystal);const title=document.createElement('strong');title.textContent=item.text;const preview=document.createElement('span');preview.className='wish-preview';preview.textContent=item.description;const sender=document.createElement('span');sender.className='wish-sender';sender.textContent=`от: ${item.text}`;button.append(star,sender);this.stage.append(button);return button;
   });this.root.classList.toggle('gallery-empty',!items.length);this.controls.hidden=items.length<2;if(!items.length){const empty=document.createElement('p');empty.className='gallery-empty-copy';empty.textContent='Здесь появятся выбранные пожелания.';this.stage.append(empty)}this.wake();
  }
  move(step){this.target=Math.round(this.target)+step;this.wake()}
  open(index){const item=this.items[index];if(!item)return;this.name.textContent=item.text;this.message.textContent=item.description;this.dialog.showModal();this.onItemClick(item,index)}
  wake(){cancelAnimationFrame(this.raf);this.last=0;this.raf=requestAnimationFrame(t=>this.frame(t))}
  frame(time){if(!this.visible)return;const dt=this.last?Math.min((time-this.last)/1000,.05):1/60;this.last=time;this.scroll+=(this.target-this.scroll)*(this.reduce.matches?1:1-Math.pow(1-this.ease,dt*60));if(Math.abs(this.target-this.scroll)<.002)this.scroll=this.target;
   const width=this.stage.clientWidth,height=this.stage.clientHeight,cardWidth=Math.min(290,width*.72),cardHeight=cardWidth*1.12;this.spacing=cardWidth*1.19;const count=this.items.length,depth=Math.abs(this.bend)*height/16.57,half=Math.max(width,height*1.9)/2,radius=(half*half+depth*depth)/(2*depth||1),active=count?wrap(Math.round(this.target),count):0;
   this.cards?.forEach((card,i)=>{let distance=count>1?wrap(i-this.scroll+count/2,count)-count/2:0;const x=distance*this.spacing,reach=Math.min(Math.abs(x),half,radius*.999),y=this.bend?radius-Math.sqrt(radius*radius-reach*reach):0,angle=this.bend?Math.sign(x)*Math.asin(reach/radius):0;card.style.width=cardWidth+'px';card.style.height=cardHeight+'px';const sender=card.querySelector('.wish-sender');if(sender){const length=Array.from(this.items[i].text).length;sender.style.fontSize=Math.max(14,Math.min(21,cardWidth*(length>18?.061:length>12?.067:.074)))+'px'}card.style.transform=`translate(${width/2+x}px,${height*.46+y}px) translate(-50%,-50%) rotate(${angle}rad)`;card.style.zIndex=String(100-Math.round(Math.abs(distance)*10));card.tabIndex=i===active?0:-1;card.setAttribute('aria-current',String(i===active));card.style.opacity=Math.abs(x)>width/2+cardWidth?0:1});this.count.textContent=count?`${active+1} / ${count}`:'';
   if(this.scroll!==this.target)this.raf=requestAnimationFrame(t=>this.frame(t));
  }
 }
 const gallery=new CircularGallery(document.getElementById('best-snowflakes'),{bend:3,scrollEase:.08});
 function update(data){gallery.setItems((data?.notes||[]).map(note=>({text:note.nickname,description:note.message})))}
 update(window.froklaTreeData);window.addEventListener('tree-data',e=>update(e.detail));
})();
