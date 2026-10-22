import os,json,subprocess,collections
from pathlib import Path
root=Path('/workspace/quench');out=root/'work/shadow-stack-isolation';env=os.environ.copy();env.pop('QUENCH_NODE_LIBRARY_ROOT',None);env['PATH']=str(root/'work/node-v26.12.0-linux-x64/bin')+':'+env['PATH'];rows=[]
for label,bundle in [('before',root/'work/native-buffer-unsafe-bin'),('candidate',root/'target/iteration')]:
 env['QUENCH_NODE_TEST_EXECUTABLE']=str(bundle/'quench-node')
 command=[str(bundle/'run-parallel'),'--all','--parallel-dir','/workspace/work/node-stable-release/node-v26.12.0/test/parallel','--filter','test-shadow-realm-','--jobs','4','--timeout-secs','30','--results',str(out/('family-'+label+'.json'))]
 with (out/('family-'+label+'.log')).open('w')as f:code=subprocess.run(command,cwd=root,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 data=json.loads((out/('family-'+label+'.json')).read_text());rows.append({'label':label,'exit':code,'counts':dict(collections.Counter(x['status']for x in data['results'])),'command':command})
(out/'family-receipt.json').write_text(json.dumps(rows,indent=2)+'\n');print(rows)
