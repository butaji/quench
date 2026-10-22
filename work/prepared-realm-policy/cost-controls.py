import json,os,subprocess,time
from pathlib import Path
root=Path('/workspace/quench');out=root/'work/prepared-realm-policy/cost-controls';env=os.environ.copy();env.pop('QUENCH_NODE_LIBRARY_ROOT',None);env.pop('QUENCH_NODE_TEST_EXECUTABLE',None)
out.mkdir(exist_ok=True)
workloads=[('shadow20',['--experimental-shadow-realm'],"let sum=0;for(let i=0;i<20;i++){const r=new ShadowRealm();sum+=r.evaluate('1+1')}console.log(sum)"),('buffer20000',[],"let sum=0;for(let i=0;i<20000;i++){const b=Buffer.allocUnsafe(1);sum+=b.length}console.log(sum)")];rows=[]
for name,flags,source in workloads:
 for label,engine in [('before',root/'work/native-buffer-pool-bin/quench-node'),('c4f',root/'work/shadow-stack-bin/quench-node'),('prepared',root/'work/prepared-realm-bin/quench-node'),('node26',root/'work/node-v26.12.0-linux-x64/bin/node')]:
  command=[str(engine),'--no-warnings',*flags,'-e',source];start=time.monotonic()
  try:
   result=subprocess.run(command,cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=30);code=result.returncode;stdout=result.stdout;stderr=result.stderr;timeout=False
  except subprocess.TimeoutExpired as e:code=None;stdout=e.stdout or b'';stderr=e.stderr or b'';timeout=True
  (out/(name+'-'+label+'.stdout')).write_bytes(stdout);(out/(name+'-'+label+'.stderr')).write_bytes(stderr);row={'workload':name,'label':label,'command':command,'elapsed_seconds':time.monotonic()-start,'exit':code,'timeout':timeout};rows.append(row);(out/'results.json').write_text(json.dumps({'purpose':'Directional cost diagnostics under concurrent audits; no speed claim','rows':rows},indent=2)+'\n')
