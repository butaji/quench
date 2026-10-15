
"use strict";
const out=[],buffer=Buffer.from('abcabc');
for(const name of ['indexOf','lastIndexOf','includes']) {
 const d=Object.getOwnPropertyDescriptor(Buffer.prototype,name);out.push([name,d.value.name,d.value.length,d.writable,d.enumerable,d.configurable,Object.hasOwn(d.value,'prototype')]);
 for(const value of ['a','bc','',Buffer.from('bc'),new Uint8Array([99]),97,NaN,-1])
  for(const offset of [undefined,-Infinity,-10,-1,-0.5,0,1,2.5,5,Infinity,NaN,null])
   for(const end of [undefined,0,2,4,100,-1,'utf8']) out.push(buffer[name](value,offset,end));
 const log=[],offset={[Symbol.toPrimitive](hint){log.push(hint);return 1;}};out.push(buffer[name]('a',offset),log);
 const marker={};try{buffer[name]('a',{valueOf(){throw marker;}});}catch(error){out.push(error===marker);}
 for(const value of [()=>{}, {}, [], null,undefined,true,Symbol('x'),1n]){try{buffer[name](value);out.push('accepted');}catch(error){out.push([error.name,error.code,error.message]);}}
 for(const call of [()=>buffer[name]({},0),()=>buffer[name]('a',0,'unknown'),()=>Buffer.prototype[name].call({},'a'),()=>buffer[name]('a',Symbol('x')),()=>buffer[name]('a',1n)]) {try{call();out.push('accepted');}catch(error){out.push([error.name,error.code||null]);}}
}
for(const name of ['indexOf','lastIndexOf','includes']) {
 const b=Buffer.from('aβaβ','utf16le');
 for(const needle of ['a','β','',Buffer.from('β','utf16le')])
  for(const offset of [-10,-1,0,1,2,3,5,20]) for(const end of [undefined,0,3,7,100]) out.push(b[name](needle,offset,end,'utf16le'));
 out.push(buffer[name](Buffer.from('a'),0,undefined,'unknown'));
 const encoding={valueOf(){return 'utf8';}};out.push(buffer[name]('a',0,undefined,encoding));
 const b2=Buffer.from('xxx');out.push(b2[name]('a',{valueOf(){b2[1]=97;return 0;}}));
}
const aligned=Buffer.from('aβaβ','utf16le');
out.push(aligned.indexOf('a',1,undefined,'utf16le'));
out.push(aligned.indexOf(Buffer.from('a','utf16le'),1,undefined,'utf16le'));
out.push(aligned.includes('a',5,undefined,'utf16le'));
console.log(JSON.stringify(out));
