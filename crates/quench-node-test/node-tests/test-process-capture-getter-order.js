'use strict';
const out=[];
process.setUncaughtExceptionCaptureCallback(function first(error) {
  out.push(['first',error.message]);
  let reads=0;
  Object.defineProperty(this,'captureFn',{configurable:true,get(){
    reads++;
    out.push(['get',reads]);
    return reads===1 ? function wrong(){out.push(['wrong']);console.log(JSON.stringify(out));} : function selected(error){out.push(['selected',error.message,this===process]);console.log(JSON.stringify(out));};
  }});
  process.nextTick(()=>{throw new Error('second');});
});
process.nextTick(()=>{throw new Error('first');});
