const get=require('internal/test/binding').internalBinding('util').getConstructorName;
const factories=[['map iterator',()=>new Map().keys()],['set iterator',()=>new Set().values()],['array iterator',()=>[].values()],['string iterator',()=>''[Symbol.iterator]()],['generator',()=>(function* Named(){})()],['anonymous generator',()=>(function*(){})()],['async generator',()=>(async function* Named(){})()],['proxy',()=>new Proxy({}, {})],['callable proxy',()=>new Proxy(()=>{}, {})],['DataView',()=>new DataView(new ArrayBuffer(0))],['Promise',()=>new Promise(()=>{})]];
const out=[];for(const [name,make]of factories){const value=make();out.push([name,get(value)]);Object.setPrototypeOf(value,null);out.push(get(value));Object.create(value);out.push(get(value));}
console.log(JSON.stringify(out));
