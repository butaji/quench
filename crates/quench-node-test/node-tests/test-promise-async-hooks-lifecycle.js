'use strict';
const assert=require('assert'), hooks=require('async_hooks');
const labels=new WeakMap(), events=[];let count=0;
const hook=hooks.createHook({
 init(id,type,trigger,resource){if(type==='PROMISE'){const label=++count;labels.set(resource,{id,label});events.push(['init',label,trigger]);}},
 before(id){events.push(['before',id]);},
 after(id){events.push(['after',id]);},
 promiseResolve(id){events.push(['resolve',id]);}
}).enable();
const first=Promise.resolve(1);
const second=first.then(value=>{
 assert.strictEqual(value,1);
 assert.strictEqual(hooks.executionAsyncResource(),second);
 assert.strictEqual(hooks.executionAsyncId(),labels.get(second).id);
 assert.strictEqual(hooks.triggerAsyncId(),labels.get(first).id);
 events.push(['handler',labels.get(second).label]);
});
setImmediate(()=>{hook.disable();const ids=new Map([ [labels.get(first).id,'first'],[labels.get(second).id,'second'] ]);console.log(JSON.stringify(events.filter(e=>e[0]==='handler'||(e[0]==='init'?e[1]<=2:ids.has(e[1]))).map(e=>e[0]==='init'?[e[0],e[1],ids.get(e[2])||'root']:e[0]==='handler'?e:[e[0],ids.get(e[1])])));});
