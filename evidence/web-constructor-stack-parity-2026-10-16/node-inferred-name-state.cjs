const vm=require('vm'),out=[];
const actions=['','void f.name;','Object.getOwnPropertyDescriptor(f,"name");','Object.defineProperty(f,"name",{});','Object.defineProperty(f,"name",{enumerable:false});','Object.defineProperty(f,"name",{enumerable:true});','Object.defineProperty(f,"name",{writable:true});','Object.defineProperty(f,"name",{value:""});','Object.defineProperty(f,"name",{configurable:false});','delete f.name;','f.extra=1;'];
for(const action of actions){out.push([action,vm.runInNewContext('Error.prepareStackTrace=(_,f)=>f[0].getFunctionName();const f=(0,function(){return new Error().stack});'+action+'f()')])}
console.log(JSON.stringify(out));
