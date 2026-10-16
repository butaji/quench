'use strict';
const fs=require('fs'),os=require('os'),path=require('path');
(async()=>{
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'fh-read-')),file=path.join(dir,'f');fs.writeFileSync(file,'xyz');
try{
for(const mode of ['none','undefined','null','buffer','positional-undefined','positional-null','object','buffer-options','offset-null','zero','coercion','invalid-offset','getter-buffer','getter-options']){
 const handle=await fs.promises.open(file,'r');const trace=[];const buffer=Buffer.alloc(4);let args;
 switch(mode){
 case 'none':args=[];break;case 'undefined':args=[undefined];break;case 'null':args=[null];break;
 case 'buffer':args=[buffer];break;case 'positional-undefined':args=[buffer,0,undefined,0];break;case 'positional-null':args=[buffer,0,null,0];break;
 case 'object':args=[{buffer,offset:1,length:null,position:0}];break;case 'buffer-options':args=[buffer,{offset:1,length:null,position:0}];break;
 case 'offset-null':args=[buffer,null,null,0];break;case 'zero':args=[Buffer.alloc(0),0,0,0];break;
 case 'coercion':args=[buffer,0,{valueOf(){trace.push('length');return 2}},0];break;
 case 'invalid-offset':Object.defineProperty(buffer,'byteLength',{get(){trace.push('byteLength');return 4}});args=[buffer,-1,null,0];break;
 case 'getter-buffer':Object.defineProperty(buffer,'byteLength',{get(){trace.push('byteLength');return 4}});args=[buffer,0,null,0];break;
 case 'getter-options':args=[{get buffer(){trace.push('buffer');return buffer},get offset(){trace.push('offset');return 0},get length(){trace.push('length');return null},get position(){trace.push('position');return 0}}];break;
 }
 try{const r=await handle.read(...args);console.log(JSON.stringify([mode,r.bytesRead,r.buffer.length,r.buffer.subarray(0,Math.min(4,r.buffer.length)).toString('hex'),Object.getPrototypeOf(r)===null,trace]))}
 catch(e){console.log(JSON.stringify([mode,e.name,e.code,e.message,trace]))}
 await handle.close();
}
}finally{fs.rmSync(dir,{recursive:true,force:true})}
})().catch(e=>{console.error(e);process.exitCode=1});
