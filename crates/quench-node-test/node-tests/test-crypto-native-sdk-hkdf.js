'use strict';
const assert=require('assert');
const crypto=require('crypto');
const {createHook,AsyncLocalStorage}=require('async_hooks');
const records=[];
const hex=value=>Buffer.from(value).toString('hex');
function failure(fn){try{fn();throw new Error('expected crypto failure')}catch(error){assert.notStrictEqual(error.message,'expected crypto failure');return[error.name,error.code??null,error.message,error.library??null,error.reason??null,error.opensslErrorStack??null]}}
for(const name of ['sha256','RSA-SHA256','sha256-192','SHA2-256/192','sha3-256','dss1','DSS1','sha256\0suffix']){
 const out=crypto.hkdfSync(name,Buffer.from('key'),Buffer.from('salt'),Buffer.from('info'),42);
 assert(out instanceof ArrayBuffer);records.push([name,hex(out)]);
}
const secret=crypto.createSecretKey(Buffer.from('key'));
records.push(['secret',hex(crypto.hkdfSync('sha256',secret,new Uint8Array([115,97,108,116]),new DataView(Uint8Array.from([105,110,102,111]).buffer),42))]);
for(const [name,length]of [['md4',32],['whirlpool',32],['NULL',32],['unknown',32],['sha256',0],['shake128',1],['sha256',8161]]){
 records.push(['failure',name,length,failure(()=>crypto.hkdfSync(name,new Uint8Array(),new ArrayBuffer(0),new DataView(new ArrayBuffer(0)),length))]);
}
records.push(['bad-key',failure(()=>crypto.hkdfSync('sha256',{},'salt','info',32))]);
records.push(['bad-info',failure(()=>crypto.hkdfSync('sha256','key','salt',Buffer.alloc(1025),32))]);
const als=new AsyncLocalStorage();
const requests=new Map();const events=[];
const hook=createHook({init(id,type){if(type==='DERIVEBITSREQUEST'){requests.set(id,true);events.push('init')}},before(id){if(requests.has(id))events.push('before')},after(id){if(requests.has(id))events.push('after')}}).enable();
(async()=>{
 let callbackRecord;
 await als.run({key:'hkdf-context'},()=>new Promise((resolve,reject)=>{
  const key=Buffer.from('key'),salt=Buffer.from('salt'),info=Buffer.from('info');let immediate=true;
  const returned=crypto.hkdf('sha256-192',key,salt,info,42,(error,value)=>{
   if(error)return reject(error);
   callbackRecord={deferred:!immediate,arrayBuffer:value instanceof ArrayBuffer,store:als.getStore(),hex:hex(value)};resolve();
  });
  assert.strictEqual(returned,undefined);key.fill(0);salt.fill(0);info.fill(0);immediate=false;
 }));
 await new Promise(resolve=>setImmediate(resolve));hook.disable();
 assert.deepStrictEqual(events,['init','before','after']);records.push(['async-snapshot-als',callbackRecord]);records.push(['native-resource',events]);
 const asyncFailure=await new Promise(resolve=>crypto.hkdf('md4','key','salt','info',32,error=>resolve([error.name,error.code,error.message,error.library,error.reason,error.opensslErrorStack])));
 records.push(['async-native-error',asyncFailure]);
 console.log(JSON.stringify(records));
})().catch(error=>{hook.disable();console.error(error);process.exitCode=1});
