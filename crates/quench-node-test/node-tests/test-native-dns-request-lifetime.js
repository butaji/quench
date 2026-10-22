'use strict';
const dns=require('dns');
const ah=require('async_hooks');
const als=new ah.AsyncLocalStorage();
const events=[];
let resource;
let id;
const hook=ah.createHook({init(asyncId,type,trigger,object){if(type==='GETADDRINFOREQWRAP'){id=asyncId;resource=object;events.push('init');}},before(asyncId){if(asyncId===id)events.push('before');},after(asyncId){if(asyncId===id)events.push('after');},destroy(asyncId){if(asyncId===id)events.push('destroy');}}).enable();
als.run('dns-context',()=>{
 dns.lookup('localhost',(error,address,family)=>{
  console.log('callback',error===null,typeof address==='string',family===4||family===6,als.getStore(),ah.executionAsyncId()===id,ah.executionAsyncResource()===resource);
  setTimeout(()=>{hook.disable(); console.log('events',JSON.stringify(events));},10);
 });
 if(typeof global.gc==='function')global.gc();
 const churn=[];for(let i=0;i<1000;i++)churn.push({i,s:'root-check'});
});
