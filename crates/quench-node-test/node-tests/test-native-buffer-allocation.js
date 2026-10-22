'use strict';
const {internalBinding}=require('internal/test/binding'),b=internalBinding('buffer');
for(const name of ['createUnsafeArrayBuffer','arrayBufferAlignedOffset']){const f=b[name],d=Object.getOwnPropertyDescriptor(b,name);console.log('method',name,f.name,f.length,Object.hasOwn(f,'prototype'),d.writable,d.enumerable,d.configurable)}
for(const alignment of [1,2,8,16,64,256,4096]){
 const ab=b.createUnsafeArrayBuffer(8192+alignment),bytes=new Uint8Array(ab);bytes.fill(0);
 const offset=b.arrayBufferAlignedOffset(ab,alignment),view=new Uint8Array(ab,offset,32);
 const clone=structuredClone(ab);view.fill(17);const copy=new Uint8Array(clone);copy.fill(23);
 console.log('align',alignment,ab instanceof ArrayBuffer,ab.byteLength,offset>=0&&offset<alignment,offset===b.arrayBufferAlignedOffset(ab,alignment),view.every(v=>v===17),copy.every(v=>v===23));
 global.gc();console.log('gc',alignment,offset===b.arrayBufferAlignedOffset(ab,alignment),view[0]);
}
console.log('empty',b.arrayBufferAlignedOffset(b.createUnsafeArrayBuffer(0),64));
