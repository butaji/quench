const calls=[];
process.emitWarning=(...args)=>calls.push(args.map(value=>value instanceof Error?{name:value.name,message:value.message,id:value.id,stack:value.stack==='Error: fixed\n    at fixed'}:value));
const error=new Error('fixed');error.stack='Error: fixed\n    at fixed';
const pending=Promise.reject(error);
setTimeout(()=>{pending.catch(()=>{});setTimeout(()=>console.log(JSON.stringify(calls)),10)},10);
