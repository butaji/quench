Error.prepareStackTrace=(_,frames)=>frames;
const Kinds=[Function,(async function(){}).constructor,(function*(){}).constructor,(async function*(){}).constructor];
async function invoke(fn){const value=fn();return typeof value?.next==='function'?(await value.next()).value:await value}
(async()=>{
 const out=[];
 for(const Kind of Kinds)for(const prefix of ['', '  ', '\n  ', '\r\n  ', '\u2028  ']){
  const body=prefix+'const frame=new Error().stack[0]; return [frame.getLineNumber(),frame.getColumnNumber()];\n  ';
  const fn=Kind(body);out.push([Kind.name,prefix,await invoke(fn),fn.toString()]);
 }
 console.log(JSON.stringify(out));
})();
