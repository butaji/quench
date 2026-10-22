const assert=require('assert');const ah=require('async_hooks');const dns=require('dns');const crypto=require('crypto');
const storage=new ah.AsyncLocalStorage();
const resources=new Map();
const hook=ah.createHook({init(id,type,trigger,resource){if(type==='GETADDRINFOREQWRAP'||type==='KEYGENREQUEST')resources.set(id,{type,trigger,weak:new WeakRef(resource)});}}).enable();
let completed=0;
function check(type){
 assert.strictEqual(storage.getStore(),'retained-context');
 const id=ah.executionAsyncId();const resource=resources.get(id);
 assert.ok(resource);assert.strictEqual(resource.type,type);assert.strictEqual(ah.triggerAsyncId(),resource.trigger);
 assert.strictEqual(resource.weak.deref(),ah.executionAsyncResource());
 if(++completed===2){hook.disable();console.log('native-pending-work-gc-ok');}
}
storage.run('retained-context',()=>{
 dns.lookup('localhost',(err,address)=>{assert.ifError(err);assert.strictEqual(typeof address,'string');check('GETADDRINFOREQWRAP');});
 crypto.generateKey('hmac',{length:128},(err,key)=>{assert.ifError(err);assert.strictEqual(key.type,'secret');check('KEYGENREQUEST');});
});
for(let i=0;i<3;i++)global.gc();
