'use strict';
const assert=require('assert'),hooks=require('async_hooks');
const ids=new WeakMap();let sourceId,release,armed=false;
const hook=hooks.createHook({init(id,type,trigger,resource){if(type!=='PROMISE')return;ids.set(resource,id);if(armed&&trigger===sourceId){armed=false;release({answer:42});}}}).enable();
const source=new Promise(resolve=>{release=resolve});sourceId=ids.get(source);armed=true;
(async()=>{const value=await source;assert.strictEqual(value.answer,42);await 1;hook.disable();console.log('reentrant-init-ok');})().catch(error=>{hook.disable();console.error(error);process.exitCode=1;});
