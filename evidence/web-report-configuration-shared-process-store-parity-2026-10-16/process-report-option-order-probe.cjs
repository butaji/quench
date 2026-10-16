const {spawnSync}=require('child_process');
const code="console.log(JSON.stringify([process.report.directory,process.report.signal,process.report.reportOnSignal,process.listenerCount(process.report.signal)]));";
const cases=[['--report-signal=SIGUSR1','--no-report-on-signal'],['--no-report-on-signal','--report-signal=SIGUSR1'],['--report-dir','alias directory'],['--report-signal=SIGUSR2','--no-report-on-signal'],['--report-on-signal','--no-report-on-signal']];
console.log(JSON.stringify(cases.map(args=>{const r=spawnSync(process.execPath,[...args,'-e',code],{encoding:'utf8'});return[args,r.status,r.stdout,r.stderr];})));
