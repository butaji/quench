
const assert=require('node:assert'),buffer=require('node:buffer');
assert.strictEqual(buffer.btoa,btoa);assert.strictEqual(buffer.atob,atob);
assert.strictEqual(btoa.name,'btoa');assert.strictEqual(atob.name,'atob');
assert.strictEqual(btoa.length,1);assert.strictEqual(atob.length,1);
const bytes=String.fromCharCode(...Array.from({length:256},(_,i)=>i));
assert.strictEqual(atob(btoa(bytes)),bytes);
assert.strictEqual(btoa(undefined),'dW5kZWZpbmVk');assert.strictEqual(btoa(null),'bnVsbA==');
for(const value of ['\ud800','\udfff','\u0100','💩'])assert.throws(()=>btoa(value),e=>e.name==='InvalidCharacterError'&&e.code===5&&e.message==='Invalid character');
for(const value of ['-A','_A','AA=','AAAA=','A===','AAAA\v','AAAA\u00a0'])assert.throws(()=>atob(value),e=>e.name==='InvalidCharacterError'&&e.code===5&&e.message==='Invalid character');
for(const value of ['A','AAAAA',undefined])assert.throws(()=>atob(value),e=>e.message==='The string to be decoded is not correctly encoded.');
assert.strictEqual(atob(' A A == \n\t\r\f'),'\0');assert.strictEqual(atob('AB'),'\0');
let calls=0;assert.strictEqual(btoa({[Symbol.toPrimitive](hint){assert.strictEqual(hint,'string');calls++;return 'A'}}),'QQ==');assert.strictEqual(calls,1);
assert.throws(()=>btoa(Symbol('x')),TypeError);assert.throws(()=>atob(Symbol('x')),TypeError);
assert.throws(()=>btoa(),e=>e.code==='ERR_MISSING_ARGS');assert.throws(()=>atob(),e=>e.code==='ERR_MISSING_ARGS');
assert(new btoa('A') instanceof btoa);assert(new atob('QQ==') instanceof atob);
const original=DOMException;globalThis.DOMException=function(){throw Error('replacement used')};
assert.throws(()=>atob('!'),e=>e instanceof original&&e.name==='InvalidCharacterError');globalThis.DOMException=original;
console.log(JSON.stringify([btoa(bytes),atob('SGVsbG8=')]));
