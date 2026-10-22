const fs=require('node:fs'),assert=require('node:assert');
for(const method of ['statSync','lstatSync','statfsSync']){
 for(const kind of ['buffer-hook','uint8','null']){
  const p=kind==='buffer-hook'?Buffer.from('/'):kind==='uint8'?new Uint8Array([47]):'a\0b';
  if(kind==='buffer-hook')p.toString=()=>{throw 'toString-marker'};
  try{console.log(method,kind,!!fs[method](p))}catch(e){console.log(method,kind,typeof e,e?.name,e?.code,e?.message??e)}
 }
}
