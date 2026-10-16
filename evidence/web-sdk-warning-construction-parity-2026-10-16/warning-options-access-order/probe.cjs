process.removeAllListeners('warning');
const accesses=[];
const options=new Proxy({type:'Warning'}, {get(target,key,receiver){accesses.push(String(key));return Reflect.get(target,key,receiver)}});
process.emitWarning('probe',options);
console.log(JSON.stringify(accesses));
