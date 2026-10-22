Error.prepareStackTrace=(_,frames)=>frames;
const Kinds=[Function,(async function(){}).constructor,(function*(){}).constructor,(async function*(){}).constructor];
async function invoke(fn){const value=fn();return typeof value?.next==='function'?(await value.next()).value:await value}
const position=frame=>[frame.getLineNumber(),frame.getColumnNumber()];
(async()=>{
 const out=[];
 for(const Kind of Kinds){
  for(const prefix of ['', '  ', '\t', '\n  ', '\r\n  ', '\r  ', '\u2028  ', '\u2029  ', '\ufeff  ']){
   const body=prefix+'return new Error().stack[0];\n  ';
   const fn=Kind(body);out.push([Kind.name,'body',prefix,position(await invoke(fn)),fn.toString()]);
  }
  for(const parameters of ['arg = new Error().stack[0]', 'arg =\n  new Error().stack[0]', 'arg /*🦀*/ = new Error().stack[0]', 'arg = (function parameter(){return new Error().stack[0]})()', 'arg = (()=>new Error().stack[0])()']){
   const fn=Kind(parameters,'\n return arg; \n');out.push([Kind.name,'parameters',parameters,position(await invoke(fn)),fn.name,fn.length,fn.toString()]);
  }
  const nested=Kind('\n return function nested(){return new Error().stack[0]};\n '),closure=await invoke(nested);
  out.push([Kind.name,'nested',position(closure()),closure.toString(),nested.toString()]);
  const evaluated=Kind('\n return eval("new Error().stack[0]");\n '),frame=await invoke(evaluated);
  out.push([Kind.name,'eval-origin',position(frame),frame.getEvalOrigin()]);
  out.push([Kind.name,'no-self-binding',await invoke(Kind('return typeof anonymous'))]);
  let strict;try{await invoke(Kind('"use strict";return anonymous'))}catch(error){strict=error.name}
  out.push([Kind.name,'strict-self-binding',strict]);
  out.push([Kind.name,'parameter-binding',await invoke(Kind('anonymous=17','return anonymous'))]);
 }
 console.log(JSON.stringify(out));
})();
