function step(bs,dt){const n=bs.length;for(let i=0;i<n;i++){const a=bs[i];for(let j=i+1;j<n;j++){const b=bs[j];const dx=a.x-b.x,dy=a.y-b.y,dz=a.z-b.z;const d2=dx*dx+dy*dy+dz*dz+0.01;const mag=dt/(d2*Math.sqrt(d2));a.vx-=dx*b.m*mag;a.vy-=dy*b.m*mag;a.vz-=dz*b.m*mag;b.vx+=dx*a.m*mag;b.vy+=dy*a.m*mag;b.vz+=dz*a.m*mag;}}
for(let i=0;i<n;i++){const a=bs[i];a.x+=dt*a.vx;a.y+=dt*a.vy;a.z+=dt*a.vz;}}
const bs=[];for(let i=0;i<5;i++)bs.push({x:i,y:i*0.5,z:-i*0.25,vx:0.1*i,vy:0,vz:0.05,m:1+i*0.1});
for(let s=0;s<400000;s++)step(bs,0.001);
let e=0;for(const b of bs)e+=b.x+b.y+b.z+b.vx+b.vy+b.vz;console.log(e);
