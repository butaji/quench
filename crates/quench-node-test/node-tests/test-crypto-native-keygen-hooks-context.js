const {internalBinding}=require('internal/test/binding');
const {SecretKeyGenJob,kCryptoJobAsync}=internalBinding('crypto');
const {createHook,AsyncLocalStorage,executionAsyncId,triggerAsyncId,executionAsyncResource}=require('node:async_hooks');
const assert=require('node:assert/strict');
const als=new AsyncLocalStorage(); const events=[];const ids=new Set();let resource;
const hook=createHook({init(id,type,trigger,r){if(type==='KEYGENREQUEST'){ids.add(id);resource=r;events.push(['init',id,trigger,als.getStore()]);}},before(id){if(ids.has(id))events.push(['before',id,als.getStore()]);},after(id){if(ids.has(id))events.push(['after',id,als.getStore()]);},destroy(id){if(ids.has(id))events.push(['destroy',id]);}}).enable();
(async()=>{
 let job;
 await als.run('owner',()=>new Promise((resolve,reject)=>{
  const parent=executionAsyncId();job=new SecretKeyGenJob(kCryptoJobAsync,128);assert.equal(resource,job);
  job.ondone=function(e,key){try{assert.equal(e,undefined);assert.equal(this,job);assert.equal(executionAsyncResource(),job);assert.equal(als.getStore(),'owner');assert.equal(triggerAsyncId(),parent);events.push(['callback',executionAsyncId(),als.getStore()]);als.run('reentrant',()=>{assert.equal(als.getStore(),'reentrant')});assert.equal(als.getStore(),'owner');resolve()}catch(e){reject(e)}};
  job.run();
 }));
 assert.equal(als.getStore(),undefined);
 await new Promise(resolve=>setImmediate(resolve));
 hook.disable();assert.deepEqual(events.map(e=>e[0]),['init','before','callback','after','destroy']);
 console.log(JSON.stringify({events:events.map(event=>event[0]),stores:events.filter(event=>event.length>2).map(event=>event[event.length-1])},null,2));
})().catch(e=>{console.error(e);process.exitCode=1});
