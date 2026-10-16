'use strict';
const {spawnSync}=require('child_process');
const base="const out=[];out.push(['initial',process.listeners('warning').map(fn=>[fn.name,fn.length,Object.hasOwn(fn,'prototype')])]);";
const finish="process.nextTick(()=>console.log(JSON.stringify(out)));";
const controlled="process.traceProcessWarnings=true;const warning=new Error('controlled');warning.name='Warning';warning.code='SKIP';warning.stack='Warning: controlled\\n    at fixed (fixture:1:1)';";
const cases=[
 {source:base+"process.removeAllListeners('warning');process.emitWarning('removed');"+finish},
 {args:['--no-warnings'],source:base+"process.on('warning',warning=>out.push(['custom',warning.message]));process.emitWarning('custom');"+finish},
 {env:{NODE_NO_WARNINGS:'1'},source:base+"process.on('warning',warning=>out.push(['custom',warning.message]));process.emitWarning('environment');"+finish},
 {source:base+controlled+"process.on('warning',warning=>out.push(['custom',warning.code,warning.message]));process.emit('warning',warning);"+finish},
 {args:['--disable-warning=SKIP'],source:base+controlled+"process.on('warning',warning=>out.push(['custom',warning.code]));process.emitWarning(warning);"+finish},
 {args:['--disable-warning','SKIP','--disable-warning','Warning'],source:base+controlled+"process.emitWarning(warning);"+finish},
 {args:['--no-deprecation','--trace-warnings','--trace-deprecation','--throw-deprecation'],source:"console.log(JSON.stringify(['noDeprecation','traceProcessWarnings','traceDeprecation','throwDeprecation','noProcessWarnings'].map(key=>[key,process[key],Object.getOwnPropertyDescriptor(process,key)])));"},
 {source:"console.log(JSON.stringify([process.emitWarning.name,process.emitWarning.length,Object.hasOwn(process.emitWarning,'prototype'),['noDeprecation','traceProcessWarnings','traceDeprecation','throwDeprecation'].map(key=>Object.hasOwn(process,key))]));"}
];
console.log(JSON.stringify(cases.map(({source,args=[],env={}})=>{const child=spawnSync(process.execPath,[...args,'-e',source],{encoding:'utf8',env:{...process.env,NODE_OPTIONS:'',NODE_NO_WARNINGS:'',...env}});return[child.status,child.signal,child.stdout,child.stderr.replace(/\(node:\d+\)/g,'(node:PID)')];})));
