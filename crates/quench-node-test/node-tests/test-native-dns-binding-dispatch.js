'use strict';
const dns = require('dns');
const {internalBinding}=require('internal/test/binding');
const binding=internalBinding('cares_wrap');
let calls=0;
const original=binding.getaddrinfo;
binding.getaddrinfo=function(req,host,family,hints,order){
 calls++;
 console.log('dispatch',host,family,hints,order,req instanceof binding.GetAddrInfoReqWrap);
 return original.call(this,req,host,family,hints,order);
};
(async()=>{
 await dns.promises.lookup('localhost',{all:true,order:'ipv6first'});
 await dns.promises.lookup('127.0.0.1');
 console.log('calls',calls);
 const saved=binding.getaddrinfo;
 let sync=true;
 binding.getaddrinfo=()=>internalBinding('uv').UV_ENOMEM;
 dns.lookup('localhost',function(error){console.log('submission',sync,error.code,error.errno,error.syscall,error.hostname);binding.getaddrinfo=saved;});
 sync=false;
})().catch(e=>{console.error(e.stack);process.exitCode=1});
