const out=[];
process.on('uncaughtExceptionMonitor',(e,origin)=>out.push(['monitor',e.message,origin]));
process.setUncaughtExceptionCaptureCallback(e=>{out.push(['capture',e.message]);console.log(JSON.stringify(out));});
process.nextTick(()=>{throw new Error('observed');});
