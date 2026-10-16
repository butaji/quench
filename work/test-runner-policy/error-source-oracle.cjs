'use strict';
const b=require('internal/test/binding').internalBinding('errors');
function make(){return new Error('sample')}
for(const mode of ['fresh','formatted','custom','capture','plain','proxy']){
 let e=make();
 if(mode==='formatted')void e.stack;
 if(mode==='custom')Object.defineProperty(e,'stack',{get(){throw new Error('stack read')}});
 if(mode==='capture'){e={};Error.captureStackTrace(e)}
 if(mode==='plain')e={};
 if(mode==='proxy')e=new Proxy(e,{});
 try{console.log(mode,JSON.stringify(b.getErrorSourcePositions(e)))}catch(x){console.log(mode,x.name,x.code,x.message)}
}
