const get=require('internal/test/binding').internalBinding('util').getConstructorName;Error.prepareStackTrace=(_,f)=>f[0].getFunctionName();
const out=[];for(const prototype of [function Named(){},Math.max,Array,Function.prototype,get]){const f=(0,function(){return new Error().stack});delete f.name;Object.setPrototypeOf(f,prototype);out.push(f())}
console.log(JSON.stringify(out));
