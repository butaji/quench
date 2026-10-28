'use strict';
const {internalBinding}=require('internal/test/binding');
const b=internalBinding('zlib'),c=internalBinding('constants').zlib;
for(const name of ['BrotliEncoder','BrotliDecoder']){const h=new b[name](name==='BrotliEncoder'?c.BROTLI_ENCODE:c.BROTLI_DECODE);console.log('constructor',name,b[name].name,b[name].length,h instanceof b[name]);h.onerror=()=>{};h.init(new Uint32Array(10).fill(0xffffffff),new Uint32Array(2),()=>{},undefined);h.close()}
const s=require('stream/iter'),z=require('zlib/iter'),input=Buffer.from('actual Brotli streamed bytes '.repeat(180));
function options(dictionary){return {chunkSize:64,params:{[c.BROTLI_PARAM_QUALITY]:4},dictionary}}
(async()=>{
 for(const dictionary of [undefined,Buffer.from('actual Brotli streamed bytes dictionary')]){
  const opts=options(dictionary),compressed=s.bytesSync(s.pullSync(s.fromSync(input),z.compressBrotliSync(opts)));
  const decoded=s.bytesSync(s.pullSync(s.fromSync(compressed),z.decompressBrotliSync({chunkSize:64,dictionary})));
  console.log('sync',Buffer.from(compressed).toString('hex'),Buffer.from(decoded).equals(input));
  const packed=await s.bytes(s.pull(s.from(input),z.compressBrotli(opts)));
  const unpacked=await s.bytes(s.pull(s.from(packed),z.decompressBrotli({chunkSize:64,dictionary})));
  console.log('async',Buffer.from(packed).toString('hex'),Buffer.from(unpacked).equals(input));
 }
 for(const bytes of [Buffer.from([255,255,255]),Buffer.from([11])]){try{s.bytesSync(s.pullSync(s.fromSync(bytes),z.decompressBrotliSync()));console.log('error','none')}catch(e){console.log('error',e.name,e.code,e.errno,e.message)}}
 const dictionary=Buffer.alloc(2048);let seed=12345;for(let i=0;i<dictionary.length;i++){seed=(Math.imul(seed,1664525)+1013904223)>>>0;dictionary[i]=seed>>>24;}
 const dictionaryInput=dictionary.subarray(700,1700);
 const packed=s.bytesSync(s.pullSync(s.fromSync(dictionaryInput),z.compressBrotliSync({dictionary,params:{[c.BROTLI_PARAM_QUALITY]:11}})));
 const plain=s.bytesSync(s.pullSync(s.fromSync(packed),z.decompressBrotliSync({dictionary})));
 const without=s.bytesSync(s.pullSync(s.fromSync(dictionaryInput),z.compressBrotliSync({params:{[c.BROTLI_PARAM_QUALITY]:11}})));
 console.log('dictionary reference',packed.length,without.length,Buffer.from(plain).equals(dictionaryInput));
 const h=new b.BrotliEncoder(c.BROTLI_ENCODE),params=new Uint32Array(256).fill(0xffffffff);params[255]=1;
 h.onerror=(message,errno,code)=>console.log('init callback',message,errno,code);
 try{h.init(params,new Uint32Array(2),()=>{},undefined);console.log('init no throw')}catch(e){console.log('init throw',e.name,e.code,e.message)}h.close();
})().catch(e=>{console.error(e);process.exitCode=1});
