'use strict';
const dns = require('dns');
const assert = require('assert');
const {promisify} = require('util');
(async()=>{
 console.log('identity',dns.promises === require('dns/promises'));
 console.log('default',dns.getDefaultResultOrder());
 dns.setDefaultResultOrder('ipv4first');
 console.log('mutable',dns.promises.getDefaultResultOrder());
 dns.promises.setDefaultResultOrder('ipv6first');
 console.log('mutable2',dns.getDefaultResultOrder());
 const records=await dns.promises.lookup('localhost',{all:true,order:'ipv4first'});
 console.log('localhost',JSON.stringify(records));
 console.log('promisify',JSON.stringify(await promisify(dns.lookup)('127.0.0.1')));
 console.log('numeric',JSON.stringify(await dns.promises.lookup('::1',{family:4,all:true})));
 for(const value of ['bad',null,1]) {
  try { dns.setDefaultResultOrder(value); } catch(e) {console.log('error',e.name,e.code,e.message);}
 }
 console.log('done');
})().catch(e=>{console.error(e.stack);process.exitCode=1});
