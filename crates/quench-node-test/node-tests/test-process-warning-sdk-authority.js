'use strict';
const assert=require('assert');
const sdk=require('internal/process/warning');
assert.strictEqual(process.emitWarning,sdk.emitWarning);
process.removeAllListeners('warning');
const trace=[],warnings=[];
process.on('warning',warning=>warnings.push([warning.name,warning.message,warning.code,warning.detail]));
const options=new Proxy({type:'Custom',code:'CODE',detail:'detail'}, {
 get(target,key,receiver){trace.push(String(key));return Reflect.get(target,key,receiver)}
});
process.emitWarning('probe',options);
assert.deepStrictEqual(trace,['ctor','code','detail','detail','type']);
const marker={};
process.emitWarning('guarded',new Proxy({}, {get(target,key){if(String(key).startsWith('\0'))throw marker;return Reflect.get(target,key)}}));
process.nextTick(()=>console.log(JSON.stringify([trace,warnings])));
