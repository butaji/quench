import collections,json,os,subprocess,time
from pathlib import Path
root=Path('/workspace/quench');out=root/'work/prepared-realm-policy';env=os.environ.copy();env.pop('QUENCH_NODE_LIBRARY_ROOT',None);rows=[]
for label,bundle in [('before',root/'work/shadow-stack-bin'),('candidate',root/'work/prepared-realm-bin')]:
 env['QUENCH_NODE_TEST_EXECUTABLE']=str(bundle/'quench-node')
 for name in ['test-shadow-realm-gc-module.js','test-stream2-read-sync-stack.js','test-stringbytes-external.js']:
  result=out/('loss-'+label+'-'+name+'.json');command=[str(bundle/'run-parallel'),'--all','--parallel-dir','/workspace/work/node-stable-release/node-v26.12.0/test/parallel','--filter',name,'--jobs','1','--timeout-secs','30','--results',str(result)];started=time.monotonic()
  with (out/('loss-'+label+'-'+name+'.log')).open('w')as log:code=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT).returncode
  data=json.loads(result.read_text());rows.append({'label':label,'fixture':name,'command':command,'exit':code,'elapsed_seconds':time.monotonic()-started,'counts':dict(collections.Counter(r['status']for r in data['results']))});(out/'loss-controls.json').write_text(json.dumps(rows,indent=2)+'\n')
