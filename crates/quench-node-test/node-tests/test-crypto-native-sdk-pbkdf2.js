'use strict';
const crypto=require('crypto');
const {AsyncLocalStorage,createHook}=require('async_hooks');
const als=new AsyncLocalStorage();
const rows=[];
const error=e=>[e.name,e.code??null,e.message,e.library??null,e.reason??null,e.opensslErrorStack??null];
for(const digest of ['sha256','sha256-192','SHA2-256/192','RSA-SHA256','dss1','sha256\0suffix','sha3-256','shake128','md4','whirlpool','NULL','unknown']){
 try{rows.push(['sync',digest,crypto.pbkdf2Sync(Buffer.from('p\0ass'),Buffer.from('s\0alt'),3,37,digest).toString('hex')]);}
 catch(e){rows.push(['sync-error',digest,error(e)]);}
}
for(const args of [['p','s',1,0,'sha256'],['p','s',0,8,'sha256'],['p','s',1,-1,'sha256'],['p','s',1,8,5]]){
 try{rows.push(['validation',crypto.pbkdf2Sync(...args).toString('hex')])}catch(e){rows.push(['validation-error',error(e)])}
}
const events=[];let id;
const hook=createHook({init(asyncId,type){if(type==='PBKDF2REQUEST'){id=asyncId;events.push(['init',als.getStore()]);}},before(asyncId){if(asyncId===id)events.push(['before',als.getStore()]);},after(asyncId){if(asyncId===id)events.push(['after',als.getStore()]);}}).enable();
let sync=true;const password=Buffer.from('password'),salt=Buffer.from('salt');
als.run({pbkdf:1},()=>crypto.pbkdf2(password,salt,2,32,'sha256-192',(e,result)=>{
 if(e)throw e;
 rows.push(['async',{deferred:!sync,store:als.getStore(),buffer:Buffer.isBuffer(result),hex:result.toString('hex')}]);
 setImmediate(()=>{hook.disable();rows.push(['hooks',events]);crypto.pbkdf2('p','s',1,8,'md4',e=>{rows.push(['async-error',error(e)]);console.log(JSON.stringify(rows));});});
}));
password.fill(0);salt.fill(0);sync=false;
