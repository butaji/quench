const assert=require('assert'),vm=require('vm');
Error.prepareStackTrace=(_,frames)=>frames;
function first(source){return(0,eval)(source)}
function second(source){return eval.call(null,source)}
const source='new Error().stack';
const a=first(source)[0],b=second(source)[0];
assert.strictEqual(a.getEvalOrigin(),b.getEvalOrigin());
for(const code of ['(function(){return 1})','({answer:1})','(class {})'])assert.notStrictEqual(first(code),second(code));
const idSource="require('util').getCallSites(1)[0].scriptId";
assert.strictEqual(first(idSource),second(idSource));
const strict='"use strict";new Error().stack';
assert.notStrictEqual(first(strict)[0].getEvalOrigin(),second(strict)[0].getEvalOrigin());
globalThis.evalTag=parts=>parts;
assert.notStrictEqual(first('evalTag`value`'),second('evalTag`value`'));
for(const code of ['evalTag`value`;new Error().stack','function unused(){return evalTag`value`};new Error().stack']){
 assert.notStrictEqual(first(code)[0].getEvalOrigin(),second(code)[0].getEvalOrigin());
}
assert.strictEqual(first('typeof evalIdentityLexical'),'undefined');
vm.runInThisContext('let evalIdentityLexical=1');
assert.strictEqual(second('typeof evalIdentityLexical'),'number');
const foreign=vm.runInNewContext('eval');
const realmSource='Error.prepareStackTrace=(_,frames)=>frames;new Error().stack';
function localRealm(){return(0,eval)(realmSource)[0].getEvalOrigin()}
function foreignFirst(){return foreign(realmSource)[0].getEvalOrigin()}
function foreignSecond(){return foreign.call(null,realmSource)[0].getEvalOrigin()}
const local=localRealm(),other=foreignFirst();assert.strictEqual(local,other);assert.strictEqual(other,foreignSecond());
const localFunction=first('(function(){return Array})');
const foreignFunction=foreign('(function(){return Array})');
assert.notStrictEqual(localFunction(),foreignFunction());
assert.strictEqual(foreignFunction(),foreign('Array'));
assert.strictEqual(Object.getPrototypeOf(foreignFunction),foreign('Function.prototype'));
for(const [code,prototype] of [['[1,2]','Array.prototype'],['({value:1})','Object.prototype']]){
 first(code);assert.strictEqual(Object.getPrototypeOf(foreign(code)),foreign(prototype));
}
first('(class {})');
const foreignClass=foreign('(class {})');
assert.strictEqual(Object.getPrototypeOf(foreignClass),foreign('Function.prototype'));
assert.strictEqual(Object.getPrototypeOf(foreignClass.prototype),foreign('Object.prototype'));
console.log('indirect eval script identity and isolation');
