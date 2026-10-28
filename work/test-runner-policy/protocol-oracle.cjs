'use strict';
const {internalBinding}=require('internal/test/binding');
for (const [name, key] of [['errors','exitCodes'], ['modules','compileCacheStatus']]) {
 const binding=internalBinding(name),value=binding[key];
 console.log(name,key,JSON.stringify(value),JSON.stringify(Object.getOwnPropertyDescriptor(binding,key)),Object.getPrototypeOf(value)===Object.prototype, Object.isFrozen(value));
 if(name==='errors')console.log(JSON.stringify(Object.getOwnPropertyDescriptors(value)));
}
const get=internalBinding('util').getCallerLocation;
function capture(){return get()}
function user(){return capture()}
const site=user();
console.log('caller',site[0],site[1],site[2]===__filename,get.name,get.length,Object.hasOwn(get,'prototype'));
console.log('eval',eval('(function outer(){return (function inner(){return get()})()})()'));
