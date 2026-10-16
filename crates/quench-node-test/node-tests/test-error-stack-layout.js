{
'use strict';
const out=[],saved=Object.getOwnPropertyDescriptor(Error,'stackTraceLimit');
for(const value of [10,0,undefined,null,NaN,Symbol('limit')]){
 Error.stackTraceLimit=value;
 for(const error of [new Error('message'),new Error('message',{cause:'cause'}),new AggregateError([], 'message'),new SuppressedError(1,2,'message')])out.push([String(value),error.constructor.name,Object.getOwnPropertyNames(error)]);
}
Object.defineProperty(Error,'stackTraceLimit',saved);
console.log(JSON.stringify(out));

}
{
'use strict';
const out=[],saved=Object.getOwnPropertyDescriptor(Error,'stackTraceLimit');
for(const [initial,during]of [[7,0],[0,7],[undefined,7],[7,undefined]]){
 Error.stackTraceLimit=initial;
 let error=new Error({toString(){Error.stackTraceLimit=during;return 'message';}});
 out.push([String(initial),String(during),Object.getOwnPropertyNames(error),error.stack===undefined?'undefined':error.stack.split('\n').length>1]);
 Error.stackTraceLimit=initial;
 error=new Error('message',{get cause(){Error.stackTraceLimit=during;return 'cause';}});
 out.push([String(initial),String(during),Object.getOwnPropertyNames(error),error.stack===undefined?'undefined':error.stack.split('\n').length>1]);
}
Object.defineProperty(Error,'stackTraceLimit',saved);console.log(JSON.stringify(out));

}
{
 const {internalBinding}=require('internal/test/binding');const out=[];
 for(const name of ['internal_only_v8','quench_missing_binding','__proto__']){
  try{internalBinding(name);}catch(error){out.push(Object.getOwnPropertyNames(error));}
 }
 console.log(JSON.stringify(out));
}

{
'use strict';
const original=AggregateError,iterator=Array.prototype[Symbol.iterator];
const snapshot=error=>[error instanceof original,error.name,error.message,Object.getOwnPropertyNames(error),error.errors];
const a=Promise.any([]),b=Promise.any([Promise.reject('a'),Promise.reject('b')]);
globalThis.AggregateError=function(){throw Error('public constructor must not run');};
const done=Promise.all([a.catch(snapshot),b.catch(snapshot)]);
Array.prototype[Symbol.iterator]=function(){throw Error('internal error creation must not iterate');};
done.then(out=>{Array.prototype[Symbol.iterator]=iterator;console.log(JSON.stringify(out));});

}
