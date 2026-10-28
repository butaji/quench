const out=[];
for (const name of ['setUncaughtExceptionCaptureCallback','hasUncaughtExceptionCaptureCallback']) {
  const fn=process[name];
  out.push([name,fn.name,fn.length,Object.hasOwn(fn,'prototype')]);
  try { const value=Reflect.construct(fn,name.startsWith('set')?[function(){}]:[]); out.push(['constructed',value instanceof fn]); }
  catch(error) { out.push(['error',error.name,error.code,error.message]); }
  process.setUncaughtExceptionCaptureCallback(null);
}
console.log(JSON.stringify(out));
