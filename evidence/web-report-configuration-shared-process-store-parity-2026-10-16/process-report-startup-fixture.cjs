'use strict';
const {spawnSync}=require('child_process');
const code="const r=process.report;console.log(JSON.stringify([r.directory,r.filename,r.compact,r.excludeEnv,r.excludeNetwork,r.signal,r.reportOnFatalError,r.reportOnSignal,r.reportOnUncaughtException,process.listenerCount(r.signal)]));";
const cases=[
 {args:['--report-compact','--report-exclude-env','--report-exclude-network','--report-uncaught-exception','--report-on-fatalerror','--report-directory','directory with spaces','--report-filename=sample.json','--report-signal=SIGUSR1','-e',code]},
 {args:['--no-report-compact','--report-directory','argument directory','-e',code],env:'--report-compact --report-directory="environment directory" --report-signal=SIGUSR1'},
 {args:['--report-on-signal','-e',code]},
 {args:['--report-dir','alias directory','-e',code]},
 {args:['--report-signal=SIGUSR1','--no-report-on-signal','-e',code]},
 {args:['--no-report-on-signal','--report-signal=SIGUSR1','-e',code]}
];
console.log(JSON.stringify(cases.map(({args,env=''})=>{const child=spawnSync(process.execPath,args,{encoding:'utf8',env:{...process.env,NODE_OPTIONS:env}});return [child.status,child.signal,child.stdout,child.stderr];})));
