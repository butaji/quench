'use strict';
const assert=require('assert');
const {internalBinding}=require('internal/test/binding');
const sdk=require('internal/process/per_thread');
const reference=sdk.wrapProcessMethods(internalBinding('process_methods'));
sdk.wrapProcessMethods=()=>{throw new Error('guest policy replacement')};
const out=[];
for(const name of ['cpuUsage','threadCpuUsage','resourceUsage']){
 const method=process[name],value=method();
 assert.strictEqual(method.toString(),reference[name].toString());
 assert(Object.values(value).every(x=>typeof x==='number'&&Number.isFinite(x)&&x>=0));
 assert(Object.values(value).some(x=>x>0));
 out.push([name,method.name,method.length,Object.hasOwn(method,'prototype'),Object.keys(value)]);
}
for(const name of ['cpuUsage','threadCpuUsage']){
 const trace=[];
 process[name]({get user(){trace.push('user');return 0},get system(){trace.push('system');return 0}});
 assert.deepStrictEqual(trace,['user','system','user','system']);out.push([name,trace]);
 const negative=process[name]({user:Number.MAX_SAFE_INTEGER,system:Number.MAX_SAFE_INTEGER});
 assert(negative.user<0&&negative.system<0);
 for(const value of [undefined,null,false,0,'',{},[],{user:-1,system:0},{user:0,system:NaN},{user:Infinity,system:0}]){
  try{out.push([name,typeof value,'accepted',Object.keys(process[name](value))]);}
  catch(error){out.push([name,typeof value,error.name,error.code,error.message]);}
 }
 const marker={};assert.throws(()=>process[name]({get user(){throw marker}}),error=>error===marker);
}
const rss=process.memoryUsage.rss;
const fs=require('fs');fs.readFileSync=()=>{throw new Error('guest filesystem replacement')};
assert(rss()>0);
out.push(['rss',rss.name,rss.length,Object.hasOwn(rss,'prototype')]);
console.log(JSON.stringify(out));
