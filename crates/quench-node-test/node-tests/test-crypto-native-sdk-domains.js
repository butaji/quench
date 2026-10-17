'use strict';
const assert = require('node:assert');
const domain = require('node:domain');
const crypto = require('node:crypto');
const { createHook, executionAsyncResource, AsyncLocalStorage } = require('node:async_hooks');
const { EventEmitter } = require('node:events');
const { WeakReference } = require('internal/util');
const records=[];
const storage=new AsyncLocalStorage();
const marker={}; const weak=new WeakReference(marker);
assert.strictEqual(weak.get(),marker);
records.push(['weak',weak.incRef(),weak.incRef(),weak.decRef(),weak.decRef(),weak.get()===marker]);
const d=domain.create(); let job;
const hook=createHook({init(id,type,trigger,resource){if(type==='PBKDF2REQUEST'){job=resource;records.push(['init',resource.domain===d,Object.getOwnPropertyDescriptor(resource,'domain').enumerable]);}}}).enable();
const failure=new Error('native-domain-callback');
d.on('error',error=>{
  assert.strictEqual(error,failure);
  records.push(['error',error.domain===d,error.domainThrown,storage.getStore(),executionAsyncResource()===job,process.domain==null]);
});
storage.run('store',()=>d.run(()=>{
  const emitter=new EventEmitter();
  records.push(['emitter',emitter.domain===d]);
  emitter.on('event',()=>records.push(['event',process.domain===d]));
  emitter.emit('event');
  crypto.pbkdf2('password','salt',1,8,'sha256',(error,bytes)=>{
    assert.ifError(error);assert.strictEqual(bytes.length,8);
    records.push(['callback',process.domain===d,storage.getStore(),executionAsyncResource()===job]);
    throw failure;
  });
}));
process.on('beforeExit',()=>{hook.disable();console.log(JSON.stringify(records));});
