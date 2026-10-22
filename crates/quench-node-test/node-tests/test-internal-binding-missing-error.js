'use strict';
const {internalBinding}=require('internal/test/binding');
const out=[];
for(const name of ['internal_only_v8','quench_missing_binding','__proto__','missing\0uv']){
 try{internalBinding(name);out.push(['returned']);}catch(error){
  const d=Object.getOwnPropertyDescriptor(error,'code');
  out.push([error.name,error.message,error.code,d.writable,d.enumerable,d.configurable,String(error),error.stack.split("\n")[0]]);
 }
}
console.log(JSON.stringify(out));
