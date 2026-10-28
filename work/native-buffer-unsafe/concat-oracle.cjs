const {Buffer}=require('buffer');
for(const length of [undefined,0,1,3,6]){const a=Buffer.from([1,2]),b=new Uint8Array([3,4]);const result=length===undefined?Buffer.concat([a,b]):Buffer.concat([a,b],length);console.log('length',String(length),result.toString('hex'),result.buffer===a.buffer);}
for(const length of [null,-1,1.5,'3',NaN]){try{Buffer.concat([Buffer.from([1])],length);}catch(e){console.log('invalid',String(length),e.name,e.code,e.message);}}
console.log('empty',Buffer.concat([],Symbol()).length);
let reads=[];const items=[Buffer.from([3]),new Uint8Array([4])];const proxy=new Proxy(items,{get(t,k,r){if(k==='length'||k==='0'||k==='1')reads.push(String(k));return Reflect.get(t,k,r)}});console.log('proxy',Buffer.concat(proxy).toString('hex'),reads.join(','));
try{Buffer.concat([new Uint16Array([1])]);}catch(e){console.log('wrong',e.name,e.code,e.message);}
let nested;const source=[Buffer.from([1])];const sideEffect=new Proxy(source,{get(t,k,r){if(k==='0')nested=Buffer.from([9]);return Reflect.get(t,k,r)}});const result=Buffer.concat(sideEffect,3);console.log('reentrant',result.toString('hex'),nested.toString('hex'),result.buffer===nested.buffer,result.byteOffset===nested.byteOffset);
