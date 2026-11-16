/* A small scheduled sweeper that clears the PixelSnow panel drifts. */
(() => {
  class SnowVacuum {
    constructor(snow) {
      this.snow=snow;this.node=document.createElement('div');this.node.className='snow-vacuum';this.node.setAttribute('aria-hidden','true');
      const sprite=['                         tt t          ','                        tt ttt         ','                         ttt           ','                    b    ttb  b        ','                    bb   bbbbb         ','                     bbbbbbbbb         ','       gggrggg        bbbbbwebbb       ','      ggggrgggg       bbbbbbbccc       ','      rrrrrrrrr       bbbbbcccccR      ','      gggrggggg      rrrrrrcccc        ','      gggrggggg  bbbbrgrrrbb           ','      gggrgggggbbbbbbbbbbbb           ','      gggrgggggbbbbbbbbbbbb           ','       ddddd  bbbbbbbbbbbbb           ','              bbbcccccbbbb           ','              bb       bb            '];
      const colors={t:'#ebc98a',b:'#b67c4b',c:'#e8bd86',w:'#fff3dc',e:'#14283a',R:'#ff5c70',r:'#bd3e59',g:'#43a185',d:'#714753'};
      let pixels='';sprite.forEach((row,y)=>[...row].forEach((v,x)=>{if(colors[v])pixels+=`<rect x="${x*3}" y="${y*3}" width="3" height="3" fill="${colors[v]}"/>`}));
      this.node.innerHTML=`<svg viewBox="0 0 124 64" shape-rendering="crispEdges" focusable="false"><g class="pixel-deer-bob">${pixels}<path class="pixel-deer-leg leg-a" d="M46 46v8h-5v3h8V46" fill="#98613f"/><path class="pixel-deer-leg leg-b" d="M52 46v7h5v3h-8V46" fill="#d6a471"/><path class="pixel-deer-leg leg-a" d="M74 46v8h5v3h-8V46" fill="#98613f"/><path class="pixel-deer-leg leg-b" d="M79 46v6h-5v3h8V46" fill="#d6a471"/></g><path d="M15 56v3h25v-3M18 53v3m18-3v3" fill="none" stroke="#e9c57f" stroke-width="2"/><path d="M80 58h8m4-3h4m-11 6h5" stroke="#d9f4ff" stroke-width="2"/></svg>`;
      document.body.append(this.node);this.running=false;this.frame=0;this.timer=0;this.motion=matchMedia('(prefers-reduced-motion: reduce)');
      this.motion.addEventListener?.('change',()=>{if(this.motion.matches)this.stop()});
      this.timer=setTimeout(()=>{this.run();this.timer=setInterval(()=>this.run(),60000)},15000);
    }
    stop(){cancelAnimationFrame(this.frame);this.running=false;this.node.classList.remove('is-running')}
    run(){
      if(this.running||document.hidden||this.motion.matches)return;
      const snow=this.snow;
      const visible=snow.surfaces.filter(s=>s.el.isConnected&&s.width>180&&s.top-scrollY>-20&&s.top-scrollY<innerHeight-20);
      const candidates=visible.filter(s=>s.pile.reduce((sum,value)=>sum+value,0)>.3);
      candidates.sort((a,b)=>b.pile.reduce((sum,value)=>sum+value,0)-a.pile.reduce((sum,value)=>sum+value,0));
      const primary=candidates[0];if(!primary)return;
      const row=visible.filter(s=>Math.abs(s.top-primary.top)<58&&(s.left+s.width>=primary.left-90&&s.left<=primary.left+primary.width+90));
      const leftEdge=Math.min(...row.map(s=>s.left))-scrollX;
      const rightEdge=Math.max(...row.map(s=>s.left+s.width))-scrollX;
      const span=rightEdge-leftEdge;
      const start=performance.now(),duration=Math.max(4500,Math.min(9500,span/90*1000));
      this.running=true;this.node.classList.add('is-running');
      const tick=now=>{
        if(document.hidden){this.stop();return}
        const t=Math.min(1,(now-start)/duration),left=leftEdge-85+t*(span+24);
        const mouthX=left+73;
        const surface=row.find(s=>mouthX>=s.left-scrollX&&mouthX<=s.left-scrollX+s.width)||primary;
        const surfaceLeft=surface.left-scrollX,local=mouthX-surfaceLeft;
        const column=Math.max(0,Math.min(surface.pile.length-1,Math.floor(local/surface.width*surface.pile.length)));
        const trackY=surface.top-scrollY-surface.pile[column]-49;
        this.node.style.transform=`translate3d(${left}px,${trackY}px,0)`;
        snow.collectSnowAt(mouthX,trackY+50,row);
        if(t<1)this.frame=requestAnimationFrame(tick);
        else{this.stop();this.node.style.transform='translate3d(-125px,-100px,0)'}
      };
      this.node.style.transform='translate3d(-125px,-100px,0)';this.frame=requestAnimationFrame(tick);
    }
  }
  window.SnowVacuum=SnowVacuum;
})();
