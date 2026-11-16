function upd(ps,n,reps){for(let r=0;r<reps;r++){for(let i=0;i<n;i++){const p=ps[i];p.x+=p.vx;p.y+=p.vy;if(p.x>100)p.vx=-p.vx;if(p.y>100)p.vy=-p.vy;p.k=(p.k+1)|0;}}}
const n=1024,ps=[];for(let i=0;i<n;i++)ps.push({x:i%50,y:i%70,vx:0.5,vy:0.25,k:0});
upd(ps,n,2000);let t=0;for(const p of ps)t+=p.x+p.y+p.k;console.log(t);
