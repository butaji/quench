'use strict';
async function scenario(mode) {
 const events=[],reason={};
 const source={
  [Symbol.asyncIterator](){return this},
  async next(){events.push('next');return {done:false,value:1}},
  async return(){events.push('close-start');await Promise.resolve();events.push('close-end');if(mode==='reject')throw reason;return {done:true}}
 };
 async function* generator(){try{for await(const value of source){yield value;if(mode==='explicit')return 'explicit'}}finally{events.push('outer-finally')}}
 if(mode==='function'){async function fn(){try{for await(const value of source)return 'function'}finally{events.push('outer-finally')}}const result=await fn();events.push('result:'+result,'observed');console.log(mode,JSON.stringify(events));return;}
 const iterator=generator();await iterator.next();
 try{const result=await(mode==='explicit'?iterator.next():iterator.return('return'));events.push('result:'+result.value)}catch(error){events.push('reason:'+(error===reason))}
 events.push('observed');console.log(mode,JSON.stringify(events));
 await Promise.resolve();await Promise.resolve();console.log(mode,'settled',JSON.stringify(events));
}
const events=[];const syncSource={[Symbol.iterator](){return this},next(){return {done:false,value:1}},return(){events.push('sync-close');return {done:true}}};function* syncGenerator(){try{for(const value of syncSource)yield value}finally{events.push('sync-finally')}}const sync=syncGenerator();sync.next();console.log('sync',JSON.stringify(sync.return('result')),JSON.stringify(events));
(async()=>{for(const mode of ['return','explicit','reject','function'])await scenario(mode)})().catch(e=>{console.error(e);process.exitCode=1});
