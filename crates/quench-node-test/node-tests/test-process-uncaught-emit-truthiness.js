'use strict';
const out=[];
const original=process.emit;
process.emit=function(name,...args) {
  if (name === 'uncaughtExceptionMonitor') { out.push([name,args[0].message,args[1]]); return Symbol('ignored'); }
  if (name === 'uncaughtException') { out.push([name,args[0].message,args[1]]); console.log(JSON.stringify(out)); return 'handled'; }
  return original.call(this,name,...args);
};
process.nextTick(() => { throw new Error('custom emit'); });
