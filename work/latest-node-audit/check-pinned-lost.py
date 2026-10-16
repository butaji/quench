import os,subprocess,json,datetime
from pathlib import Path
root=Path('/workspace/quench');base=root/'work/full-native-codecs-restart-pinned';env=os.environ.copy();env.pop('QUENCH_NODE_LIBRARY_ROOT',None);env.pop('QUENCH_NODE_TEST_EXECUTABLE',None);env['PATH']=str(root/'work/node-v27.0.0-nightly20260819c79868f102-linux-x64/bin')+':'+env['PATH']
rows=[]
for name in ['test-stream2-read-sync-stack.js','test-stringbytes-external.js']:
 out=base/(name+'.control.json');cmd=[str(root/'work/native-zstd-bin/run-parallel'),'--all','--parallel-dir','/workspace/work/node-fixtures/test/parallel','--filter',name,'--jobs','1','--timeout-secs','30','--results',str(out)]
 with (base/(name+'.control.log')).open('w')as f:code=subprocess.run(cmd,cwd=root/'work/native-codec-full-checkout',env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 rows.append({'fixture':name,'exit':code,'command':cmd,'results':json.loads(out.read_text())})
(base/'lost-controls.json').write_text(json.dumps(rows,indent=2)+'\n')
print([(x['fixture'],x['exit'])for x in rows])
