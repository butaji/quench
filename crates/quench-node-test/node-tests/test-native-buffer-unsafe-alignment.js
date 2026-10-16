'use strict';
const {Buffer,constants}=require('buffer');
const {internalBinding}=require('internal/test/binding');
const {arrayBufferAlignedOffset}=internalBinding('buffer');
const {isMarkedAsUntransferable}=require('worker_threads');
for(const name of ['allocUnsafe','allocUnsafeSlow']){
 const a=Buffer[name](3),b=Buffer[name](3),d=Object.getOwnPropertyDescriptor(Buffer,name);
 console.log('method',name,Buffer[name].name,Buffer[name].length,Object.hasOwn(Buffer[name],'prototype'),d.writable,d.enumerable,d.configurable);
 console.log('ordinary',name,a.buffer===b.buffer,a.buffer.byteLength,a.length,isMarkedAsUntransferable(a.buffer));
 for(const alignment of [1,8,16,64,256,4096]){
  const x=Buffer[name](17,alignment);x.fill(17);
  const snapshot=structuredClone(x.buffer);x.fill(23);global.gc();
  const physical=arrayBufferAlignedOffset(x.buffer,alignment);
  console.log('aligned',name,alignment,x.length,x.buffer.byteLength,(x.byteOffset-physical+alignment)%alignment===0,isMarkedAsUntransferable(x.buffer),x.every(v=>v===23),new Uint8Array(snapshot,x.byteOffset,17).every(v=>v===17));
 }
 console.log('zero',name,Buffer[name](0,4096).buffer.byteLength);
 for(const alignment of [0,3,1.5,NaN,Infinity,'8',null,{},64n]){
  try{Buffer[name](3,alignment);console.log('accepted',name,String(alignment));}catch(e){console.log('invalid',name,String(alignment),e.name,e.code,e.message);}
 }
 for(const size of [-1,constants.MAX_LENGTH+1]){try{Buffer[name](size,64);}catch(e){console.log('size',name,e.name,e.code,e.message);}}
 try{Buffer[name](constants.MAX_LENGTH,4096);}catch(e){console.log('padded',name,e.name,e.code,e.message);}
}
const anchor=Buffer.from([1]);const joined=Buffer.concat([anchor,new Uint8Array([2,3])]);console.log('concat',joined.buffer===anchor.buffer,joined.toString('hex'),joined.buffer.byteLength);
