const fs=require('fs'),{transcode}=require('buffer');
const rows=fs.readFileSync(0,'utf8').trim().split('\n').map(JSON.parse);
for(const r of rows){
 if(r.op==='transcode') {
  try{console.log(JSON.stringify({bytes:[...transcode(Buffer.from(r.bytes),r.from,r.to)]}));}
  catch(e){console.log(JSON.stringify({error:e.code,errno:e.errno}));}
 }else throw Error('width needs --expose-internals binding oracle');
}
