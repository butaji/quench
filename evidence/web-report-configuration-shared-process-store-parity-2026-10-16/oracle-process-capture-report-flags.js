'use strict';
const p=require('process');
const out=[];
const actual=p.report;
actual.reportOnUncaughtException=true;
p.setUncaughtExceptionCaptureCallback(()=>{});out.push(['set',actual.reportOnUncaughtException,p.hasUncaughtExceptionCaptureCallback()]);
p.setUncaughtExceptionCaptureCallback(null);out.push(['clear',actual.reportOnUncaughtException,p.hasUncaughtExceptionCaptureCallback()]);
actual.reportOnUncaughtException=false;p.setUncaughtExceptionCaptureCallback(null);out.push(['clear-again',actual.reportOnUncaughtException]);actual.reportOnUncaughtException=false;
const saved=Object.getOwnPropertyDescriptor(p,'report');
const log=[];
let flag=true;
const fake={get reportOnUncaughtException(){log.push('read');return flag;},set reportOnUncaughtException(value){log.push(['write',value]);flag=value;}};
Object.defineProperty(p,'report',{configurable:true,get(){log.push('report');return fake;}});
p.setUncaughtExceptionCaptureCallback(function captured(){
 Object.defineProperty(this,'reportFlag',{configurable:true,get(){log.push('saved');return true;}});
 log.length=0;p.setUncaughtExceptionCaptureCallback(null);out.push(['restore-order',log.slice()]);
 Object.defineProperty(this,'reportFlag',{configurable:true,writable:true,value:false});
 Object.defineProperty(p,'report',{configurable:true,get(){throw new Error('report getter');}});
 try{p.setUncaughtExceptionCaptureCallback(()=>{});}catch(error){out.push(['set-abrupt',error.message,p.hasUncaughtExceptionCaptureCallback()]);}
 try{p.setUncaughtExceptionCaptureCallback(null);}catch(error){out.push(['clear-abrupt',error.message,p.hasUncaughtExceptionCaptureCallback()]);}
 Object.defineProperty(p,'report',saved);
 console.log(JSON.stringify(out));
});
out.push(['install-order',log.slice()]);
process.nextTick(()=>{throw new Error('capture');});
