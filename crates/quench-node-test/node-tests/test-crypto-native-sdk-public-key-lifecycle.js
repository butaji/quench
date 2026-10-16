const m=require('node:crypto');
const {types}=require('node:util');
const sdk=require('internal/crypto/webcrypto');
const records=[];
function record(name,value){records.push({name,value});}
async function check(name,fn){try{record(name,await fn())}catch(e){record(name,{failure:e.name,message:e.message,stack:e.stack})}}
(async()=>{
 await check('namespace',()=>({moduleGlobal:m.webcrypto===globalThis.crypto,moduleSource:m.webcrypto===sdk.crypto,subtleSource:m.subtle===sdk.crypto.subtle,cryptoConstructor:globalThis.Crypto===sdk.Crypto,keyConstructor:globalThis.CryptoKey===sdk.CryptoKey,subtleConstructor:globalThis.SubtleCrypto===sdk.SubtleCrypto,prototype:Object.getPrototypeOf(globalThis.crypto)===sdk.Crypto.prototype}));
 await check('descriptors',()=>['crypto','Crypto','SubtleCrypto','CryptoKey'].map(name=>{const d=Object.getOwnPropertyDescriptor(globalThis,name);return{name,enumerable:d.enumerable,configurable:d.configurable,writable:d.writable,get:typeof d.get}}));
 await check('random',()=>{const array=new Uint8Array(16);return{moduleIdentity:m.getRandomValues(array)===array,sourceIdentity:sdk.crypto.getRandomValues(array)===array,methodDistinct:m.getRandomValues!==sdk.crypto.getRandomValues}});
 for(const bytes of [16,24,32])for(const kind of ['typed','buffer','view'])await check('import-'+bytes+'-'+kind,async()=>{
  const data=new Uint8Array(bytes+4);data.fill(37);
  const input=kind==='typed'?data.subarray(2,bytes+2):kind==='buffer'?data.buffer.slice(2,bytes+2):new DataView(data.buffer,2,bytes);
  const key=await m.subtle.importKey('raw',input,'AES-GCM',true,['encrypt','decrypt']);
  data.fill(0);if(kind==='buffer')new Uint8Array(input).fill(0);
  const raw=new Uint8Array(await m.subtle.exportKey('raw',key));
  return{type:key.type,length:key.algorithm.length,name:key.algorithm.name,usages:key.usages,extractable:key.extractable,brand:types.isCryptoKey(key),instance:key instanceof sdk.CryptoKey,copied:raw.length===bytes&&raw.every(b=>b===37)};
 });
 for(const [bytes,usages]of [[15,['encrypt']],[16,['sign']],[16,[]]])await check('import-error-'+bytes+'-'+usages.join(),async()=>{try{await m.subtle.importKey('raw',new Uint8Array(bytes),'AES-GCM',true,usages);return{unexpectedSuccess:true}}catch(e){return{name:e.name,message:e.message}}});
 for(const bits of [128,192,256])await check('generate-'+bits,async()=>{const a=await m.subtle.generateKey({name:'AES-GCM',length:bits},true,['encrypt','decrypt']);const b=await m.subtle.generateKey({name:'AES-GCM',length:bits},true,['encrypt','decrypt']);const aa=new Uint8Array(await m.subtle.exportKey('raw',a));const bb=new Uint8Array(await m.subtle.exportKey('raw',b));return{type:a.type,name:a.algorithm.name,length:a.algorithm.length,usages:a.usages,extractable:a.extractable,brand:types.isCryptoKey(a),bytes:aa.length,independent:aa.some((x,i)=>x!==bb[i])}});
 console.log(JSON.stringify(records,null,2));
})().catch(e=>{console.error(e.stack);process.exitCode=1});
