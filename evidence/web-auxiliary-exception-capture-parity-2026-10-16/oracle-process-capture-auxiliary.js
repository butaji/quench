'use strict';
const out=[];
process.setUncaughtExceptionCaptureCallback(function first(error) {
  out.push(['primary',error.message]);
  const callbacks=this.auxiliaryCallbacks;
  callbacks.push(function earlier(error){out.push(['earlier',error.message,this===callbacks]);console.log(JSON.stringify(out));return true;});
  callbacks.push(function later(error){out.push(['later',error.message,this===callbacks]);return 1;});
  process.setUncaughtExceptionCaptureCallback(null);
  process.nextTick(()=>{throw new Error('auxiliary');});
});
process.nextTick(()=>{throw new Error('primary');});
