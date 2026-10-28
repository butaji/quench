const util=require('util'),out=[];
function Capture(){return util.getCallSites(2)}
function Runner(){return Capture()}
function project(sites){return sites.map(site=>[Reflect.ownKeys(site),Object.getPrototypeOf(site)===null,site.functionName,site.scriptName,site.lineNumber,site.columnNumber,site.column,/^[1-9]\d*$/.test(site.scriptId),Object.values(Object.getOwnPropertyDescriptors(site)).every(d=>d.writable&&d.enumerable&&d.configurable)])}
Object.defineProperty(Capture,'name',{value:'Changed'});let sites=Runner();out.push(project(sites),sites[0].scriptId===sites[1].scriptId);
Error.stackTraceLimit=0;Error.prepareStackTrace=()=>{throw Error('stack hook')};out.push(project(Runner()));
Object.defineProperty(Capture,'name',{value:'\ud800\0wide'});out.push(Runner().map(s=>s.functionName));
let calls=0;Object.defineProperty(Capture,'name',{get(){calls++;throw Error('name getter')}});out.push(Runner().map(s=>s.functionName),calls);
for(const args of [[0],[201],[NaN],[Infinity],['2'],[null],[1,null],[1,{sourceMap:1}],[{},{}]]){try{util.getCallSites(...args);out.push('accepted')}catch(error){out.push([error.name,error.code,error.message])}}
out.push(util.getCallSites.name,util.getCallSites.length,Reflect.ownKeys(util.getCallSites),Function.prototype.toString.call(util.getCallSites));
console.log(JSON.stringify(out));
