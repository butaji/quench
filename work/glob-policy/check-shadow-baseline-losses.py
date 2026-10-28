import collections,json,os,subprocess
from pathlib import Path
root=Path('/workspace/quench');out=root/'work/full-shadow-stack-modern-buffer-stable';bundle=root/'work/native-buffer-pool-bin';names=json.loads((out/'comparison.json').read_text())['lost'];rows=[]
env=os.environ.copy();env.pop('QUENCH_NODE_LIBRARY_ROOT',None);env.pop('QUENCH_NODE_TEST_EXECUTABLE',None)
for name in names:
 result=out/('baseline-control-'+name+'.json');command=[str(bundle/'run-parallel'),'--all','--parallel-dir','/workspace/work/node-stable-release/node-v26.12.0/test/parallel','--filter',name,'--jobs','1','--timeout-secs','30','--results',str(result)]
 with (out/('baseline-control-'+name+'.log')).open('w')as log:code=subprocess.run(command,cwd=root/'work/native-buffer-full-checkout',env=env,stdout=log,stderr=subprocess.STDOUT).returncode
 data=json.loads(result.read_text());rows.append({'fixture':name,'command':command,'exit':code,'counts':dict(collections.Counter(r['status']for r in data['results']))})
(out/'baseline-lost-controls.json').write_text(json.dumps(rows,indent=2)+'\n');print(rows)
