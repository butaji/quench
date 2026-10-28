const get=require('internal/test/binding').internalBinding('util').getConstructorName;
const factories=[['map iterator',()=>new Map().keys()],['set iterator',()=>new Set().values()],['array iterator',()=>[].values()],['string iterator',()=>''[Symbol.iterator]()],['generator',()=>(function* Named(){})()],['anonymous generator',()=>(function*(){})()],['async generator',()=>(async function* Named(){})()],['proxy',()=>new Proxy({}, {})],['callable proxy',()=>new Proxy(()=>{}, {})],['DataView',()=>new DataView(new ArrayBuffer(0))],['Promise',()=>new Promise(()=>{})]];
const out=[];for(const [name,make]of factories){const value=make();out.push([name,get(value)]);Object.setPrototypeOf(value,null);out.push(get(value));Object.create(value);out.push(get(value));}
console.log(JSON.stringify(out));
{
 Error.prepareStackTrace=(_,frames)=>frames[0].getFunctionName();let calls=0;
 function Frame(){return new Error().stack}function Base(){};delete Frame.name;delete Base.name;
 const names=[];
 for(const prototype of [{name:'Inherited'},{name:1},{get name(){calls++;throw Error('getter')}},new Proxy({name:'Proxy'},{get(){calls++;throw Error('trap')}})]){
  Object.setPrototypeOf(Frame,prototype);Object.setPrototypeOf(Base,prototype);names.push(Frame(),get(new Base),calls);
 }
 console.log(JSON.stringify(names));
}
