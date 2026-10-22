'use strict';
const {internalBinding}=require('internal/test/binding');
const b=internalBinding('zlib'),c=internalBinding('constants').zlib;
const handle=new b.Zlib(c.GZIP),state=new Uint32Array(2),events=[];
handle.onerror=(message,errno,code)=>events.push([message,errno,code]);
console.log('constructor',b.Zlib.name,b.Zlib.length,handle instanceof b.Zlib);
for(const name of ['write','writeSync','close','init']){const f=b.Zlib.prototype[name],d=Object.getOwnPropertyDescriptor(b.Zlib.prototype,name);console.log('method',name,f.name,f.length,d.writable,d.enumerable,d.configurable);}
handle.init(15,4,9,0,state,()=>events.push('callback'),undefined);
const input=Buffer.from('streaming actual bytes'),chunks=[];let offset=0;
for(let i=0;i<20;i++){const out=Buffer.alloc(7,255);handle.writeSync(c.Z_FINISH,input,offset,input.length-offset,out,0,7);const n=7-state[0];offset=input.length-state[1];chunks.push(out.subarray(0,n));console.log('step',i,JSON.stringify(Array.from(state)),JSON.stringify(Array.from(out)));if(state[0]!==0)break;}
console.log('roundtrip',require('zlib').gunzipSync(Buffer.concat(chunks)).equals(input),JSON.stringify(events));handle.close();
(async()=>{
 const asyncHandle=new b.Zlib(c.GZIP),writeState=new Uint32Array(2),output=Buffer.alloc(1000),marker=[];
 let synchronous=true;
 const done=new Promise((resolve,reject)=>{asyncHandle.onerror=(m,n,code)=>reject(new Error(code+':'+m));asyncHandle.init(15,4,9,0,writeState,function(){marker.push(this===asyncHandle,!synchronous,writeState[1]===0);resolve()},undefined)});
 asyncHandle.write(c.Z_FINISH,input,0,input.length,output,0,output.length);asyncHandle.close();synchronous=false;
 await done;console.log('async close',JSON.stringify(marker),require('zlib').gunzipSync(output.subarray(0,output.length-writeState[0])).equals(input));
 const s=require('stream/iter'),z=require('zlib/iter'),source=new TextEncoder().encode('actual streamed bytes '.repeat(200));
 const compressed=await s.bytes(s.pull(s.from(source),z.compressGzip({chunkSize:64})));
 const decoded=await s.bytes(s.pull(s.from(compressed),z.decompressGzip({chunkSize:64})));
 console.log('iter async',compressed.length,decoded.length,Buffer.from(decoded).equals(source));
 const deflated=s.bytesSync(s.pullSync(s.fromSync(source),z.compressDeflateSync({chunkSize:64})));
 const inflated=s.bytesSync(s.pullSync(s.fromSync(deflated),z.decompressDeflateSync({chunkSize:64})));
 console.log('iter sync',deflated.length,inflated.length,Buffer.from(inflated).equals(source));
})().catch(error=>{console.error(error);process.exitCode=1});
