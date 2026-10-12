import hashlib, importlib.util, json, subprocess
from pathlib import Path
root=Path('/workspace/quench'); out=root/'work/task61-earley-movecall-linux-2026-10-10'
spec=importlib.util.spec_from_file_location('benchrunner',root/'work/task61-direct-number-screen/runner.py'); mod=importlib.util.module_from_spec(spec); spec.loader.exec_module(mod)
data=json.loads((root/'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json').read_text()); env=next(e['environment'] for e in data['engines'] if e['name']=='quench'); node='/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node'
binaries={'baseline':out/'baseline','candidate':out/'candidate'}
records=[]
for name in ['one-argument','multiple-arguments','receiver','direct-eval','caught-object']:
 path=out/f'oracle-{name}.js'; expected=subprocess.run([node,str(path)],env=env,text=True,capture_output=True)
 outputs={}
 for variant,binary in binaries.items():
  result=subprocess.run([str(binary),str(path)],env=env,text=True,capture_output=True)
  outputs[variant]={'exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr,'matches_node':result.returncode==expected.returncode and result.stdout==expected.stdout and result.stderr==expected.stderr}
 records.append({'case':name,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'node_exit_code':expected.returncode,'node_stdout':expected.stdout,'node_stderr':expected.stderr,'baseline':outputs['baseline'],'candidate':outputs['candidate']})
report={'node_version':subprocess.run([node,'--version'],text=True,capture_output=True,check=True).stdout.strip(),'cases':records,'all_match_node':all(row['baseline']['matches_node'] and row['candidate']['matches_node'] for row in records)}
(out/'node-oracle-report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
