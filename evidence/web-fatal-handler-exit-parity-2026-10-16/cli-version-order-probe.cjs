const {spawnSync}=require('child_process');
const cases=[['--version'],['-v'],['--jitless','--version'],['-e','throw 1','--version'],['--version','-e','throw 1'],['--jitless','-v'],['-e','console.log(JSON.stringify(process.argv.slice(1)))','--','--version'],['-e','console.log(JSON.stringify(process.argv.slice(1)))','guest','--version']];
console.log(JSON.stringify(cases.map(args=>{const child=spawnSync(process.execPath,args,{encoding:'utf8'});return [args,child.status,child.signal,child.stdout,child.stderr];})));
