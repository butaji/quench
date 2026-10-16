'use strict';
const assert=require('assert'),out=[];
for(const name of ['uv','fs','config','constants','os','zlib']){
 const first=process.binding(name);assert.strictEqual(first,process.binding(name));out.push([name,typeof first]);
}
const types=process.binding('util');assert.notStrictEqual(types,process.binding('util'));
assert.strictEqual(types.isPromise(Promise.resolve()),true);assert.strictEqual(types.isTypedArray(new Uint16Array(1)),true);
const uv=process.binding('uv');assert.strictEqual(uv.errname(uv.UV_EINVAL),'EINVAL');
for(const value of [undefined,null,Symbol('uv'),{toString(){return 'absent'}}]){
 try{process.binding(value);assert.fail('unknown binding must throw')}catch(error){out.push([error.name,error.message,error.code||null]);}
}
const reads=[];assert.strictEqual(process.binding({[Symbol.toPrimitive](hint){reads.push(hint);return 'uv'}}),uv);assert.deepStrictEqual(reads,['string']);
for(const name of ['binding']){const fn=process[name],d=Object.getOwnPropertyDescriptor(process,name);out.push([fn.name,fn.length,Object.hasOwn(fn,'prototype'),d.writable,d.enumerable,d.configurable]);}
console.log(JSON.stringify([out,Object.keys(types)]));
