'use strict';
const {spawnSync}=require('child_process');
const out=[];
for(const phase of ['top','tick','promise']) for(const handler of ['monitor','capture','uncaught']) {
  const setup=handler==='monitor' ? "process.on('uncaughtExceptionMonitor',handler);" : handler==='capture' ? "process.setUncaughtExceptionCaptureCallback(handler);" : "process.on('uncaughtException',handler);";
  const trigger=phase==='top'?"throw new Error('initial');":phase==='tick'?"process.nextTick(()=>{throw new Error('initial');});":"Promise.reject(new Error('initial'));";
  const source="process.on('exit',code=>console.log('exit '+code));function handler(){throw new SyntaxError('handler');}"+setup+trigger;
  const child=spawnSync(process.execPath,['--unhandled-rejections=throw','-e',source],{encoding:'utf8'});
  out.push([phase,handler,child.status,child.signal,child.stdout,child.stderr.split(/\r?\n/).find(line=>/^SyntaxError:/.test(line))]);
}
for(const code of [1,7,23]) for(const handler of ['monitor','capture','uncaught']) {
  const setup=handler==='monitor' ? "process.on('uncaughtExceptionMonitor',handler);" : handler==='capture'?"process.setUncaughtExceptionCaptureCallback(handler);":"process.on('uncaughtException',handler);";
  const source="process.on('exit',code=>console.log('exit '+code));function handler(){process.exit("+code+");}"+setup+"throw new Error('initial');";
  const child=spawnSync(process.execPath,['-e',source],{encoding:'utf8'});
  out.push(['explicit',code,handler,child.status,child.signal,child.stdout,child.stderr]);
}
const auxiliary="process.on('exit',code=>console.log('exit '+code));process.setUncaughtExceptionCaptureCallback(function(){this.auxiliaryCallbacks.push(()=>{throw new SyntaxError('auxiliary');});this.captureFn=null;process.nextTick(()=>{throw new Error('next');});});throw new Error('initial');";
const child=spawnSync(process.execPath,['-e',auxiliary],{encoding:'utf8'});
out.push(['auxiliary',child.status,child.signal,child.stdout,child.stderr.split(/\r?\n/).find(line=>/^SyntaxError:/.test(line))]);
console.log(JSON.stringify(out));
