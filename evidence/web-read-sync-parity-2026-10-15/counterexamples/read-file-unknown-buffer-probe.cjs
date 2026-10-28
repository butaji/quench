const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),out=[];
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'quench-read-unknown-')),file=path.join(directory,'empty');fs.writeFileSync(file,'');
try{for(const size of [0,3,6,7]){
 const fd=fs.openSync(file,'r'),original=fs.readSync,source=Buffer.from('abcdef');let pos=0;
 fs.readSync=function(fd,buffer,offset,length){const n=Math.min(length,source.length-pos);buffer.set(source.subarray(pos,pos+n),offset);pos+=n;return n};
 try{const b=fs.readFileSync(fd,{buffer:Buffer.alloc(size)});out.push(['value',b.toString(),pos])}catch(e){out.push(['error',e.name,e.code,e.message,Reflect.ownKeys(e).filter(k=>k!=='stack'),Object.getPrototypeOf(e).constructor.name,Reflect.ownKeys(Object.getPrototypeOf(e)),pos])}finally{fs.readSync=original;fs.closeSync(fd)}
}}finally{fs.rmSync(directory,{recursive:true,force:true})}console.log(JSON.stringify(out));
