'use strict';
const assert=require('assert'),EventEmitter=require('events');
const publicConsole=require('console');
const sdkGlobal=require('internal/console/global');
const sdkConstructor=require('internal/console/constructor').Console;
assert.strictEqual(console,publicConsole);assert.strictEqual(console,sdkGlobal);
assert.strictEqual(console.Console,sdkConstructor);
const prototype=Object.getPrototypeOf(console);
const out=[Reflect.ownKeys(prototype).map(String),Object.getPrototypeOf(prototype)===Object.prototype,sdkConstructor.name,sdkConstructor.length,Object.hasOwn(sdkConstructor,'prototype')];
for(const name of ['log','error','assert','table','dirxml','group','groupCollapsed','groupEnd','count','countReset','time','timeLog','timeEnd','trace']){
 const method=console[name],descriptor=Object.getOwnPropertyDescriptor(console,name);
 out.push([name,method.name,method.length,Object.hasOwn(method,'prototype'),descriptor.enumerable,descriptor.writable,descriptor.configurable,Function.prototype.toString.call(method)]);
}
const sink=new EventEmitter(),writes=[];sink.write=(text,callback)=>{writes.push(text);callback?.();return true};
const instance=sdkConstructor({stdout:sink,stderr:sink,colorMode:false});
assert(instance instanceof sdkConstructor);assert.strictEqual(instance.constructor,sdkConstructor);
out.push(['instance-metadata',...['assert','table','dirxml','groupCollapsed'].map(key=>[key,instance[key].name,instance[key].length,sdkConstructor.prototype[key].length])]);
instance.group('heading');instance.log('line');instance.assert(false,'failed %d',1);instance.groupEnd();instance.count('counter');instance.countReset('counter');instance.table([{a:1}]);
const getters=[];
new sdkConstructor({
 get stdout(){getters.push('stdout');return sink},get stderr(){getters.push('stderr');return sink},
 get ignoreErrors(){getters.push('ignoreErrors');return true},get colorMode(){getters.push('colorMode');return false},
 get inspectOptions(){getters.push('inspectOptions');return undefined},get groupIndentation(){getters.push('groupIndentation');return 2}
});
for(const args of [[],[null],[undefined],[{}],[{stdout:{}}],[{stdout:sink,colorMode:7}],[{stdout:sink,groupIndentation:-1}]]){
 try{Reflect.construct(sdkConstructor,args);out.push(['accepted']);}catch(error){out.push([error.name,error.code,error.message]);}
}
const weakGet=WeakMap.prototype.get,poison={};
WeakMap.prototype.get=function(){throw poison};
try{instance.log('protected diagnostic state')}finally{WeakMap.prototype.get=weakGet}
out.push(getters,writes);
console.log(JSON.stringify(out));
