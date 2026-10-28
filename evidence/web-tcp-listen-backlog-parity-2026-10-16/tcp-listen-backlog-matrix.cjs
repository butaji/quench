const child=require('child_process');
const probe=child.spawnSync('ss',['--version'],{encoding:'utf8'});
if(process.platform!=='linux'||probe.error)console.log('socket diagnostics unavailable');
else {
 const out=[];
 const cases=[['net-ipv4','net',{port:0,host:'127.0.0.1'}],['net-default','net',{port:0}],['net-reuse','net',{port:0,host:'127.0.0.1',reusePort:true}],['http-ipv4','http',{port:0,host:'127.0.0.1'}]];
 function next(index){if(index===cases.length){console.log(JSON.stringify(out));return}
  const [name,module,options]=cases[index],server=require(module).createServer();
  server.listen(options,()=>{const port=server.address().port;const result=child.spawnSync('ss',['-ltnH','sport = :'+port],{encoding:'utf8'});if(result.status!==0)throw Error(result.stderr);const rows=result.stdout.trim().split('\n').filter(Boolean).map(line=>Number(line.trim().split(/\s+/)[2]));out.push([name,rows]);server.close(()=>next(index+1))});
 }
 next(0);
}
