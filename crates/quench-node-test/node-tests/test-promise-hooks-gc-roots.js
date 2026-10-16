'use strict';
const assert=require('assert');
const hook=require('async_hooks').createHook({init(id,type,trigger,resource){if(type==='PROMISE'){assert(resource instanceof Promise);gc();}},before(){gc();},after(){gc();},promiseResolve(){gc();}}).enable();
(async()=>{const first=await Promise.resolve({value:42});assert.strictEqual(first.value,42);await new Promise(resolve=>setImmediate(resolve));const values=await Array.fromAsync([1,2]);assert.deepStrictEqual(values,[1,2]);hook.disable();console.log('promise-gc-ok');})().catch(error=>{hook.disable();console.error(error);process.exitCode=1;});
