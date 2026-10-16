'use strict';
const assert=require('assert'),ah=require('async_hooks');
const root=ah.executionAsyncId(),ids=new Map(),trace=[];
const hook=ah.createHook({init(id,type,trigger,resource){
  if(type!=='PROMISE')return;
  assert(resource instanceof Promise);
  assert(!ids.has(id));
  const token=ids.size;ids.set(id,token);
  assert(trigger===root||ids.has(trigger));
  trace.push([token,trigger===root?'root':ids.get(trigger)]);
}}).enable();
async function main(){await null;await 0;}
main().then(()=>{hook.disable();console.log(JSON.stringify(trace));});
