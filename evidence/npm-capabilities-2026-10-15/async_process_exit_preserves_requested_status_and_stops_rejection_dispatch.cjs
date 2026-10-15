
'use strict';
const {spawnSync}=require('node:child_process');
const output=[];
for(const operation of ['exit','reallyExit'])for(const code of [0,7]) {
 const source=`process.on('exit',code=>console.log('exit',code));process.on('unhandledRejection',()=>console.log('unexpected rejection'));Promise.resolve().then(async()=>{process.nextTick(()=>console.log('unexpected tick'));process.${operation}(${code});});`;
 const result=spawnSync(process.execPath,['-e',source],{encoding:'utf8'});
 output.push([operation,code,result.status,result.signal,result.stdout,result.stderr]);
}
console.log(JSON.stringify(output));
