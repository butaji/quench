'use strict';
const out=[];
process.setUncaughtExceptionCaptureCallback(function primary(error) {
  out.push(['primary',error.message]);
  const state=this;
  const next={
    get '1.5'(){out.push(['get','1.5']);return function(error){out.push(['later',error.message,this===next]);return 1;};},
    get '0.5'(){out.push(['get','0.5']);return function(error){out.push(['earlier',error.message,this===next]);console.log(JSON.stringify(out));return true;};}
  };
  this.auxiliaryCallbacks={get length(){out.push(['length']);state.auxiliaryCallbacks=next;return {valueOf(){out.push(['number']);if(global.gc)global.gc();return 2.5;}};}};
  this.captureFn=null;
  process.nextTick(()=>{throw new Error('auxiliary');});
});
process.nextTick(()=>{throw new Error('primary');});
