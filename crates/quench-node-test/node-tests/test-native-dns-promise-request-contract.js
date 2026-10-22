'use strict';
const {internalBinding}=require('internal/test/binding');
const cares=internalBinding('cares_wrap');
let returned=['some-address'];
let resolveCalls=0;
cares.getaddrinfo=(req)=>{
 console.log('request',typeof req.resolve,typeof req.reject,req.family);
 const original=req.resolve;
 req.resolve=function(value){resolveCalls++;return original(value)};
 req.oncomplete(null,returned.slice());
};
const dns=require('dns');
(async()=>{
 console.log('unknown',JSON.stringify(await dns.promises.lookup('example.invalid')),'resolveCalls',resolveCalls);
 returned=['::1','some-address'];
 console.log('forced',JSON.stringify(await dns.promises.lookup('example.invalid',{family:4,all:true})),'resolveCalls',resolveCalls);
})().catch(e=>{console.log('error',e.code,e.message);process.exitCode=1});
