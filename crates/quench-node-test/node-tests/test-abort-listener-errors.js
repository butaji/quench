'use strict';
const {pull,dump}=require('stream/iter');
(async()=>{
 const first={},second={},third={},seen=[],order=[];
 let phase='dispatch';
 const reported=Promise.withResolvers();
 const handler=error=>{seen.push([error===first?'first':error===second?'second':error===third?'third':'unexpected',phase]);if(seen.length===3)reported.resolve()};
 process.on('uncaughtException',handler);
 const controller=new AbortController();
 controller.signal.addEventListener('abort',()=>{order.push('first');throw first},{once:true});
 controller.signal.addEventListener('abort',()=>{order.push('after');return {then(resolve,reject){order.push(['then',reject.name,reject.length,Object.hasOwn(reject,'prototype')]);reject(second)}}});
 controller.signal.addEventListener('abort',()=>Promise.reject(third));
 let escaped=false;try{controller.abort('original')}catch{escaped=true}
 phase='after';
 console.log('dispatch',escaped,controller.signal.reason,JSON.stringify(order));
 await reported.promise;
 process.removeListener('uncaughtException',handler);
 console.log('reported',JSON.stringify(seen));
 const sourceError=new Error('source failure'),listenerError=new Error('listener failure'),errors=[];
 const onError=error=>errors.push(error===listenerError);
 process.on('uncaughtException',onError);
 const transform={transform(source,options){options.signal.addEventListener('abort',()=>{throw listenerError});return source}};
 async function* source(){yield [new Uint8Array([65])];throw sourceError}
 try{await dump(pull(source(),transform))}catch(error){console.log('source identity',error===sourceError)}
 await new Promise(resolve=>setImmediate(resolve));process.removeListener('uncaughtException',onError);console.log('listener identity',JSON.stringify(errors));
})().catch(error=>{console.error(error);process.exitCode=1});
