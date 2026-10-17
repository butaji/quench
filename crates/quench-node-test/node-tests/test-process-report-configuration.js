'use strict';
const p=require('process');
const out=[];
const descriptor=Object.getOwnPropertyDescriptor(p,'report');
out.push(['descriptor',descriptor.enumerable,descriptor.configurable,descriptor.get.name,descriptor.get.length,Object.hasOwn(descriptor.get,'prototype'),descriptor.set]);
const report=p.report;
out.push(['identity',report===p.report,report===require('internal/process/report').report]);
out.push(['keys',Object.keys(report)]);
for(const key of ['writeReport','getReport'])out.push(['method',key,report[key].name,report[key].length,Object.hasOwn(report[key],'prototype')]);
const values={directory:'report directory',filename:'sample.json',compact:true,excludeNetwork:true,signal:'SIGUSR1',reportOnFatalError:true,reportOnSignal:true,reportOnUncaughtException:true,excludeEnv:true};
for(const [key,value] of Object.entries(values)){
 const original=report[key];out.push(['default',key,original]);report[key]=value;out.push(['set',key,report[key]]);
 for(const invalid of [undefined,null,0,1,{},[],1n,Symbol('invalid')])try{report[key]=invalid;out.push(['accepted',key,typeof invalid]);}catch(error){out.push(['error',key,error.name,error.code,error.message]);}
 report[key]=original;
}
console.log(JSON.stringify(out));
