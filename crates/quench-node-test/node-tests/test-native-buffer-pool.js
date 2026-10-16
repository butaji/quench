const {Buffer}=require('buffer');const wt=require('worker_threads');
console.log('pool size',Buffer.poolSize);
const a=Buffer.from('abc'),b=Buffer.from([1,2,3]),c=Buffer.copyBytesFrom(new Uint16Array([258,772]));
console.log('pooled',a.buffer===b.buffer,b.buffer===c.buffer,b.byteOffset-a.byteOffset,c.byteOffset-b.byteOffset,a.buffer.byteLength,a.byteOffset%8,b.byteOffset%8,c.byteOffset%8);
console.log('bytes',a.toString('hex'),b.toString('hex'),c.toString('hex'));
console.log('marked',wt.isMarkedAsUntransferable(a.buffer));
try{structuredClone(a.buffer,{transfer:[a.buffer]});console.log('transfer accepted')}catch(e){console.log('transfer',e.name)}
console.log('empty',Buffer.from('').buffer.byteLength,Buffer.from([]).buffer.byteLength);
const ab=new ArrayBuffer(16),view=Buffer.from(ab,2,4);view[0]=9;console.log('zero copy',view.buffer===ab,new Uint8Array(ab)[2]);
const size=Buffer.poolSize;Buffer.poolSize=16;const d=Buffer.from('abc'),large=Buffer.from('x'.repeat(20)),e=Buffer.from([4]);console.log('changed',d.buffer===e.buffer,large.buffer===d.buffer,d.toString(),large.length,e[0]);Buffer.poolSize=size;
const originalSize=Buffer.poolSize;Buffer.poolSize=128;
let last;for(let i=0;i<40000;i++)last=Buffer.from([i&255]);
const anchor=Buffer.from([1]),another=Buffer.from([2]);console.log('exhaustion',last.buffer.byteLength,anchor.buffer===another.buffer,another.byteOffset-anchor.byteOffset,anchor.buffer.byteLength);
const snapshot=structuredClone(anchor.buffer);anchor[0]=77;console.log('snapshot',new Uint8Array(snapshot)[anchor.byteOffset],anchor[0]);
let nested;const outer=Buffer.from({length:2,get 0(){nested=Buffer.from([8,9]);return 3},get 1(){return 4}});console.log('reentrant',outer.toString('hex'),nested.toString('hex'),outer.buffer===nested.buffer,outer.byteOffset===nested.byteOffset);
Buffer.poolSize=originalSize;
const s=require('stream/iter');(async()=>{const data=Buffer.from('binary data here');const result=await s.bytes(s.from(data));console.log('consumer',result.constructor.name,Buffer.isBuffer(result),Array.from(result).join(','));})().catch(e=>{console.error(e);process.exitCode=1});
