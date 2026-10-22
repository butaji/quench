'use strict';
const h=require('async_hooks'),events=[];let target;
const hook=h.createHook({init(id,type,trigger,resource){if(type==='Timeout'&&target===undefined)target={id,resource}},before(id){if(target&&id===target.id)events.push('before')},after(id){if(target&&id===target.id)events.push('after')}}).enable();
process.once('uncaughtException',()=>{events.push(['uncaught',h.executionAsyncId()===target.id,h.executionAsyncResource()===target.resource]);setImmediate(()=>{hook.disable();console.log(JSON.stringify(events))});});
setTimeout(()=>{throw new Error('probe')},1);
