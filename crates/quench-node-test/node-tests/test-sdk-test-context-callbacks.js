'use strict';
const assert=require('assert');const api=require('node:test');
api.test('context across native timer and immediate',async t=>{
 assert.strictEqual(api.getTestContext(),t);
 await Promise.resolve();
 await new Promise(resolve=>process.nextTick(()=>{assert.strictEqual(api.getTestContext(),t);resolve();}));
 await new Promise(resolve=>setTimeout(()=>{assert.strictEqual(api.getTestContext(),t);resolve();},1));
 assert.strictEqual(api.getTestContext(),t);
 await new Promise(resolve=>setImmediate(()=>{assert.strictEqual(api.getTestContext(),t);resolve();}));
 assert.strictEqual(api.getTestContext(),t);
});
