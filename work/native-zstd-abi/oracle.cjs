'use strict';
const {internalBinding}=require('internal/test/binding');const b=internalBinding('zlib'),c=internalBinding('constants').zlib;
const s=require('stream/iter'),z=require('zlib/iter'),input=Buffer.from('actual Zstd streamed bytes '.repeat(180));
const encode=(data,opts={})=>s.bytesSync(s.pullSync(s.fromSync(data),z.compressZstdSync(opts)));
const decode=(data,opts={})=>s.bytesSync(s.pullSync(s.fromSync(data),z.decompressZstdSync(opts)));
{
 const h=new b.ZstdCompress(),state=new Uint32Array(2),data=Buffer.from('actual native Zstd streaming bytes');
 h.onerror=(m,n,code)=>{throw new Error(code+':'+m)};h.init(new Uint32Array(0),undefined,state,()=>{},undefined);let offset=0;
 for(;;){const out=Buffer.alloc(7,255);h.writeSync(2,data,offset,data.length-offset,out,0,7);offset=data.length-state[1];console.log('raw step',state[1],state[0],JSON.stringify(Array.from(out)));if(state[0]>0)break}h.close();
}
(async()=>{
 for(const name of ['ZstdCompress','ZstdDecompress']){const h=new b[name]();h.onerror=()=>{};h.init(new Uint32Array(0),undefined,new Uint32Array(2),()=>{},undefined);console.log('constructor',name,b[name].name,b[name].length,h instanceof b[name]);h.close()}
 for(const opts of [{chunkSize:64},{chunkSize:64,params:{[c.ZSTD_c_nbWorkers]:1}},{chunkSize:64,pledgedSrcSize:input.length,params:{[c.ZSTD_c_compressionLevel]:7}}]){
  const packed=encode(input,opts);console.log('sync',Buffer.from(packed).toString('hex'),Buffer.from(decode(packed,{chunkSize:64})).equals(input));
  const compressed=await s.bytes(s.pull(s.from(input),z.compressZstd(opts)));const plain=await s.bytes(s.pull(s.from(compressed),z.decompressZstd({chunkSize:64})));console.log('async',Buffer.from(compressed).toString('hex'),Buffer.from(plain).equals(input));
 }
 const packed=Buffer.from(encode(input));console.log('concat',Buffer.from(decode(Buffer.concat([packed,packed]))).equals(Buffer.concat([input,input])));
 console.log('trailing',Buffer.from(decode(Buffer.concat([packed,Buffer.from([1,2,3,4,5])]))).equals(input));
 for(const data of [Buffer.from([1,2,3,4]),packed.subarray(0,packed.length-1)]){try{decode(data);console.log('decode no error')}catch(e){console.log('decode error',e.name,e.code,e.errno,e.message)}}
 try{encode(input,{pledgedSrcSize:input.length+1});console.log('pledge no error')}catch(e){console.log('pledge error',e.name,e.code,e.errno,e.message)}
 const dictionary=Buffer.alloc(2048);let seed=12345;for(let i=0;i<dictionary.length;i++){seed=(Math.imul(seed,1664525)+1013904223)>>>0;dictionary[i]=seed>>>24;}
 const data=dictionary.subarray(700,1700),compressed=encode(data,{dictionary});console.log('dictionary',compressed.length,encode(data).length,Buffer.from(decode(compressed,{dictionary})).equals(data));
 for(const pledgedSrcSize of [-1,1.5,NaN,Infinity,Number.MAX_SAFE_INTEGER+1,'2',null]){try{encode(input,{pledgedSrcSize});console.log('invalid pledge no error')}catch(e){console.log('invalid pledge',e.name,e.code,e.message)}}
 const h=new b.ZstdCompress();const params=new Uint32Array(1001).fill(0xffffffff);params[999]=1;h.onerror=(m,n,code)=>console.log('init callback',m,n,code);try{h.init(params,undefined,new Uint32Array(2),()=>{},undefined);console.log('init no throw')}catch(e){console.log('init throw',e.name,e.code,e.message)}h.close();
})().catch(e=>{console.error(e);process.exitCode=1});
