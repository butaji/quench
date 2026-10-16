const types=require('util').types,out=[];
const entries=[['boolean',true,Boolean.prototype.valueOf],['number',7,Number.prototype.valueOf],['string','s',String.prototype.valueOf],['symbol',Symbol('s'),Symbol.prototype.valueOf],['bigint',7n,BigInt.prototype.valueOf]];
function result(method,receiver){try{const value=method.call(receiver);return[typeof value,String(value)]}catch(error){return[error.name]}}
for(const [kind,value,method]of entries){
 const key='\0quench:'+kind+'-value',fake={[key]:value},real=Object(value);
 out.push([kind,types.isBoxedPrimitive(fake),Object.prototype.toString.call(fake),result(method,fake)]);
 for(const [,other]of entries)out.push(result(method,Object(other)));
 Object.defineProperty(real,key,{value:'forged',configurable:true});Object.setPrototypeOf(real,null);
 out.push(types.isBoxedPrimitive(real),result(method,real));
 let calls=0;const proxy=new Proxy(real,{get(){calls++;throw Error('get')},getPrototypeOf(){calls++;throw Error('prototype')}});
 out.push(types.isBoxedPrimitive(proxy),result(method,proxy),calls);
 if(kind!=='symbol')out.push(result(method,structuredClone(real)));
}
for(const value of [Boolean.prototype,Number.prototype,String.prototype,Symbol.prototype,BigInt.prototype])out.push(types.isBoxedPrimitive(value));
for(const value of [Object(true),Object(7),Object('s'),Object(7n)]){try{out.push(JSON.stringify(value))}catch(error){out.push(error.name)}}
console.log(JSON.stringify(out));
