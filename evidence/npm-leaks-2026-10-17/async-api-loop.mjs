import fs from 'node:fs'; import fsp from 'node:fs/promises'; import http from 'node:http'; import { spawnSync, execFileSync } from 'node:child_process'; import path from 'node:path'; import os from 'node:os';
const r=()=>Math.round(process.memoryUsage.rss()/1048576);
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'lk5-')); fs.writeFileSync(path.join(dir,'m.mjs'),'export default 1'); fs.writeFileSync(path.join(dir,'f.txt'),'x'.repeat(100));
const server=http.createServer((q,s)=>s.end('ok')); await new Promise(res=>server.listen(0,res)); const port=server.address().port;
const cases={
 readFileP: ()=>fsp.readFile(path.join(dir,'f.txt')),
 timeout: ()=>new Promise(res=>setTimeout(res,0)),
 importCached: ()=>import(path.join(dir,'m.mjs')),
 httpGet: ()=>new Promise(res=>http.get(`http://127.0.0.1:${port}/`,s=>{s.resume();s.on('end',res)})),
 readStream: ()=>new Promise(res=>fs.createReadStream(path.join(dir,'f.txt')).on('data',()=>{}).on('end',res)),
};
const counts={readFileP:20000,timeout:20000,importCached:20000,httpGet:2000,readStream:5000};
const which=process.argv[2]; const f=cases[which]; const out=[];
for (let round=0; round<4; round++) { for (let i=0;i<counts[which];i++) await f(); out.push(r()); }
console.log(which.padEnd(14), out.join(' ')); server.close(); fs.rmSync(dir,{recursive:true,force:true});
