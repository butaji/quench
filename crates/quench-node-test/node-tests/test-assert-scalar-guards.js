const a=require('assert');const s=require('assert/strict');const events=[];
console.log('aliases',a.strict===s,a.strictEqual===a.Assert.prototype.strictEqual,a.strictEqual===s.equal,a.notStrictEqual===s.notEqual);
for(const key of ['strictEqual','notStrictEqual']){const f=a[key],d=Object.getOwnPropertyDescriptor(f,'prototype');console.log(key,f.name,f.length,Boolean(d),d.writable,d.enumerable,d.configurable,f.prototype.constructor===f)}
const bomb={toString(){events.push('toString');throw 'message'},valueOf(){events.push('valueOf');throw 'value'}};
for(const [actual,expected]of[[undefined,undefined],[NaN,NaN],[-0,-0],[1,1],['x','x'],[bomb,bomb]]) console.log('same',a.strictEqual(actual,expected,bomb));
for(const [actual,expected]of[[-0,0],[1,2],[{},{}],[Symbol(),Symbol()]])console.log('different',a.notStrictEqual(actual,expected,bomb));
for(const key of ['strictEqual','notStrictEqual'])for(const args of [[],[undefined],[1,2],[NaN,NaN],[-0,0]])try{a[key](...args);console.log(key,'ok')}catch(e){console.log(key,e.name,e.code,e.operator,Boolean(e.actual===args[0]),Boolean(e.expected===args[1]))}
const custom=new a.Assert({strict:false,diff:'full'});console.log('instance',custom.strictEqual(1,1),custom.notStrictEqual(1,2));
console.log('construct',Object.getPrototypeOf(new a.strictEqual(1,1))===a.strictEqual.prototype);
console.log('events',events.join(','));
const effects=[];const customReceiver=new Proxy(new a.Assert({diff:'full'}),{get(target,key,receiver){if(typeof key==='symbol')effects.push('options');return Reflect.get(target,key,receiver)}});
try{a.strictEqual.call(customReceiver,1,2,()=>{effects.push('message');return 'custom mismatch'})}catch(e){console.log('custom failure',e.code,e.operator,e.message,e.actual,e.expected)}
try{a.notStrictEqual.call(customReceiver,1,1,()=>{effects.push('message');return 'custom equality'})}catch(e){console.log('custom failure',e.code,e.operator,e.message,e.actual,e.expected)}
console.log('effects',effects.join(','));
