'use strict';
const assert = require('assert');
const {internalBinding} = require('internal/test/binding');
const binding = internalBinding('contextify');
const sdk = require('internal/vm');
const rows=[];
const C = binding.ContextifyScript;
rows.push([C.name,C.length,C.prototype.runInContext.name,C.prototype.runInContext.length,
  Object.getPrototypeOf(C)===Function.prototype,Object.getPrototypeOf(C.prototype)===Object.prototype]);
const id = sdk.getHostDefinedOptionId(undefined,'owned-script.js');
function compile(source,filename='owned-script.js') {
  return sdk.makeContextifyScript(source,filename,0,0,undefined,false,undefined,id,undefined);
}
globalThis.__ownedScriptCount=0;
const counter=compile('globalThis.__ownedScriptCount++; __ownedScriptCount');
assert(counter instanceof C);
rows.push([__ownedScriptCount,sdk.runScriptInThisContext(counter,true,false),
  sdk.runScriptInThisContext(counter,true,false),__ownedScriptCount]);
try {compile('var broken = ;')}catch(e){rows.push(['syntax',e instanceof SyntaxError,__ownedScriptCount]);}
globalThis.__ownedTemplate=x=>x;
const template=compile('__ownedTemplate`real template site`');
const first=sdk.runScriptInThisContext(template,true,false);
gc();
rows.push(['template',first===sdk.runScriptInThisContext(template,true,false),
  Object.isFrozen(first),Object.isFrozen(first.raw)]);
let escaped=compile('(function retained(){return 73})','retained-script.js');
const fn=sdk.runScriptInThisContext(escaped,true,false);escaped=null;gc();
rows.push(['escaped',fn(),fn.name]);
(function(){
  const privateCallerValue=42;
  const result=sdk.internalCompileFunction('return [a+1,typeof privateCallerValue]',
    'compiled-function.js',0,0,undefined,false,undefined,undefined,['a'],id,undefined);
  assert.strictEqual(typeof result.function,'function');
  rows.push(['function',result.function(2),result.function.name,result.function.length,
    Function.prototype.toString.call(result.function),Object.getPrototypeOf(result.function)===Function.prototype]);
  const two=sdk.internalCompileFunction('return a+b','two-parameters.js',0,0,
    undefined,false,undefined,undefined,['a','b'],id,undefined).function;
  rows.push(['two-parameters',two(3,4),two.name,two.length,two.toString()]);
  assert.strictEqual(privateCallerValue,42);
})();
try {sdk.runScriptInThisContext(compile('throw new Error("real script failure")','native-owned-error.js'),true,false)}
catch(e){rows.push(['throw',e.name,e.message,e.stack.includes('native-owned-error.js:1:')]);}
for(let i=0;i<8;i++) {
  const temporary=compile('({value:17})');
  assert.strictEqual(sdk.runScriptInThisContext(temporary,true,false).value,17);
  gc();
}
delete globalThis.__ownedTemplate;delete globalThis.__ownedScriptCount;
console.log(JSON.stringify(rows));
