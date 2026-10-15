const {spawnSync}=require('node:child_process');
const result=spawnSync(process.execPath,['-e', "require('/workspace/quench/tests/npm-top300/node_modules/foreground-child').foregroundChild(process.execPath,['-e','console.log(42)']);"],{encoding:'utf8'});
console.log(JSON.stringify({status:result.status,signal:result.signal,stdout:result.stdout,stderr:result.stderr}));
