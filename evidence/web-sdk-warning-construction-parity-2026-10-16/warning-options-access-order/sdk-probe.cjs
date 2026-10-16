process.emitWarning=require('internal/process/warning').emitWarning;
process.removeAllListeners('warning');
const accesses=[];
process.emitWarning('probe',new Proxy({type:'Warning'},{get(target,key,receiver){accesses.push(String(key));return Reflect.get(target,key,receiver)}}));
console.log(JSON.stringify(accesses));
process.on('warning', warning=>console.log(JSON.stringify([warning.name,warning.message,Object.keys(warning)])));
