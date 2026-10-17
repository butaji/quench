'use strict';
const assert=require('node:assert/strict');
const {setTimeout:sleep}=require('node:timers/promises');
(async()=>{
 const signal=AbortSignal.timeout(1);let event;
 signal.addEventListener('abort',e=>event=e,{once:true});await sleep(10);
 assert.equal(signal.aborted,true);assert.equal(event.target,signal);assert.equal(signal.reason.name,'TimeoutError');assert.equal(signal.reason.code,23);
 console.log('actual-timeout-event-and-DOMException OK');
 const timeout=AbortSignal.timeout(2),combined=AbortSignal.any([timeout]);await sleep(10);assert.equal(combined.aborted,true);assert.equal(combined.reason,timeout.reason);
 console.log('timeout-composite-snapshot OK');
 const deref=WeakRef.prototype.deref,unregister=FinalizationRegistry.prototype.unregister;
 WeakRef.prototype.deref=()=>{throw Error('poisoned public weak deref')};FinalizationRegistry.prototype.unregister=()=>{throw Error('poisoned public registry')};
 try{const protectedSignal=AbortSignal.timeout(1);await sleep(10);assert.equal(protectedSignal.aborted,true);console.log('original-safe-weak-and-registry-methods OK');}
 finally{WeakRef.prototype.deref=deref;FinalizationRegistry.prototype.unregister=unregister;}
 const callback=()=>{};let signal2=AbortSignal.timeout(1_200_000);const reference=new WeakRef(signal2);signal2.addEventListener('abort',callback);signal2=null;
 await sleep(1);gc();assert.notEqual(reference.deref(),undefined);reference.deref().removeEventListener('abort',callback);
 await sleep(1);gc();assert.equal(reference.deref(),undefined);console.log('last-strong-listener-controls-persistence OK');
})();
