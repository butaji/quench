const r=()=>Math.round(process.memoryUsage.rss()/1048576);
const cases={
 uptime:()=>process.uptime(), hrtime:()=>process.hrtime(), cwd:()=>process.cwd(), rss:()=>process.memoryUsage.rss(),
 strRoot:()=>require('path').resolve('a'), envRead:()=>process.env.HOME, statNo:()=>{try{require('fs').statSync('/nope')}catch{}},
 stat:()=>require('fs').statSync('/tmp'), fstat:(()=>{const fs=require('fs');const fd=fs.openSync('/tmp/x-lk','w');return ()=>fs.fstatSync(fd)})(),
};
const f=cases[process.argv[2]]; const out=[];
for (let round=0; round<4; round++) { for (let i=0;i<50000;i++) f(); out.push(r()); }
console.log(process.argv[2].padEnd(10), out.join(' '));
