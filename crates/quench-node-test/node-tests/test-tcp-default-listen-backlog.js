const child=require('child_process');
if(process.platform!=='linux'||child.spawnSync('strace',['--version']).status!==0)console.log('syscall tracing unavailable');
else {
 const fs=require('fs'),os=require('os'),path=require('path');
 const directory=fs.mkdtempSync(path.join(os.tmpdir(),'listen-backlog-'));
 try {
  const trace=path.join(directory,'listen.trace');
  const source=`const cases=[['net',{port:0,host:'127.0.0.1'}],['net',{port:0}],['net',{port:0,host:'127.0.0.1',reusePort:true}],['http',{port:0,host:'127.0.0.1'}]];function next(index){if(index===cases.length)return;const [module,options]=cases[index];const server=require(module).createServer();server.listen(options,()=>server.close(()=>next(index+1)))}next(0);`;
  child.execFileSync('strace',['-f','-e','trace=listen','-o',trace,process.execPath,'-e',source],{encoding:'utf8'});
  const values=[...fs.readFileSync(trace,'utf8').matchAll(/listen\(\d+,\s*(-?\d+)\)/g)].map(match=>Number(match[1]));
  if(values.length!==4)throw Error('expected four traced TCP listens');
  console.log(JSON.stringify(values));
 } finally {fs.rmSync(directory,{recursive:true,force:true})}
}
