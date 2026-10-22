'use strict';
const {text,from}=require('stream/iter'),{converters}=require('internal/webidl');
(async()=>{
 const signal=AbortSignal.abort();
 try{await text(from('data'),{signal});console.log('unexpected success')}catch(e){console.log('aborted',e.name,e.code,e.message)}
 const live=new AbortController().signal;
 for(const value of [live,signal,new Proxy(live,{}),Object.create(AbortSignal.prototype),{aborted:false,addEventListener(){}},null,undefined,1,()=>{}]){
  try{console.log('brand',converters.AbortSignal(value,{context:'input'})===value)}catch(e){console.log('invalid',e.name,e.message)}
 }
 Object.setPrototypeOf(live,null);console.log('changed prototype',converters.AbortSignal(live)===live);
 const original=WeakMap.prototype.get,marker={};WeakMap.prototype.get=function(){throw marker};
 try{console.log('private map',converters.AbortSignal(live)===live)}finally{WeakMap.prototype.get=original}
})().catch(e=>{console.error(e);process.exitCode=1});
