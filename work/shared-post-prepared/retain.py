import hashlib,json,subprocess
from pathlib import Path
base=Path('work/shared-post-prepared');state=json.loads(Path('work/task-supervisor/shared-post-prepared-state.json').read_text())
assert state['status']=='complete'and state['exit']==0,state
for name,count in [('core.log',659),('all-api.log',374),('shared.log',7)]:assert f'{count} passed; 0 failed' in(base/name).read_text(),name
assert 'pass=43 skip=0 fail=0 timeout=0 crash=0 unclassified=0 total=43'in(base/'framework.log').read_text()
source=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip();assert source.startswith('ebf7ddcdd'),source
assert not subprocess.check_output(['git','diff','--','crates'],text=True)
manifest=json.loads(Path('work/shared-post-prepared-bin/source-sha256.json').read_text())
assert all(hashlib.sha256(Path(n).read_bytes()).hexdigest()==h for n,h in manifest.items())
def raw(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
paths=[base/n for n in ['qualify.sh','check.log','core.log','paired-build.log','all-api.log','framework.log','shared.log','workspace.log','core656-cache-reclamation.json']]
paths+=[Path('work/task-supervisor/shared-post-prepared'+s)for s in ['-state.json','-config.json','.log']]
paths += [Path('work/shared-post-prepared-bin')/n for n in ['source-commit.txt','source-sha256.json','sha256.txt']]
result={'source_commit':source,'scope':'Normal shared merge preserving prepared-code ownership and all incoming eval/array/lazy-policy oracle comparisons','oracle':'Node v26.12.0 latest stable','checks':{'core':659,'api':374,'framework':43,'shared':7,'workspace_all_targets':True},'limitations':['No new full-suite totals for this merge. Frozen e9 full audits are running separately.','No performance claim. Unchanged fixture assertions and deadlines.'],'raw':[raw(p)for p in paths if p.exists()]}
Path('evidence/task20-node-shared-prepared-core659-api374-2026-10-16.json').write_text(json.dumps(result,indent=2)+'\n');print(source,result['checks'])
