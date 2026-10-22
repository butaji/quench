const types=require('util').types,out=[];
for(const [kind,value,method]of [['boolean',true,Boolean.prototype.valueOf],['number',7,Number.prototype.valueOf],['string','s',String.prototype.valueOf],['symbol',Symbol('s'),Symbol.prototype.valueOf],['bigint',7n,BigInt.prototype.valueOf]]){
 const fake={['\0quench:'+kind+'-value']:value};
 let coerced;try{coerced=[typeof method.call(fake),String(method.call(fake))]}catch(e){coerced=[e.name]}
 out.push([kind,types.isBoxedPrimitive(fake),Object.prototype.toString.call(fake),coerced]);
}
console.log(JSON.stringify(out));
