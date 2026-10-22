import hashlib,json,subprocess
from pathlib import Path
base=Path('work/operation-source-integration');bundle=Path('work/operation-source-bin')
for label in ['operation-source','operation-extras']:
    state=json.loads(Path('work/task-supervisor/'+label+'-state.json').read_text())
    assert state['status']=='complete' and state['exit']==0,state
assert '655 passed; 0 failed' in (base/'core.log').read_text()
assert '368 passed; 0 failed' in (base/'all-api.log').read_text()
assert not subprocess.check_output(['git','diff','--','crates'],text=True)
source=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert source==(bundle/'source-commit.txt').read_text().strip()
manifest=json.loads((bundle/'source-sha256.json').read_text())
assert all(hashlib.sha256(Path(n).read_bytes()).hexdigest()==h for n,h in manifest.items())
for name in ['shadow.diff','buffer-oracle.diff','buffer-concat-oracle.diff','buffer-latin1-oracle.diff','buffer-poison-oracle.diff']:
    assert not (base/name).read_bytes(),name
fixed=json.loads((base/'fixed-work.json').read_text())
assert fixed['complete'] and len(fixed['fixtures'])==8
assert all(x['valid'] and x['output_equal'] for x in fixed['fixtures'].values())
def raw(p):
    b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
paths=[p for p in sorted(base.iterdir()) if p.is_file() and p.name!='retain.py']
paths += [bundle/n for n in ['source-sha256.json','source-commit.txt','sha256.txt']]
paths += [Path('work/task-supervisor/'+label+suffix) for label in ['operation-source','operation-extras'] for suffix in ['-state.json','.log','-config.json']]
paths=[p for p in paths if p.exists()]
out={'source_commit':source,'incoming_shared':'58b4d199f','kind':'combined source-position and Node-policy qualification','oracle':'Node v26.12.0 latest stable verified 2026-10-16','checks':{'core':655,'api':368,'framework':43,'shared':7,'workspace_all_targets':True,'empty_differentials':5},'source_files_verified':len(manifest),'fixed_work':{'valid':8,'clean':sum(bool(x['clean_rounds'])for x in fixed['fixtures'].values()),'speed_claim':False},'limitations':['All-parallel goal remains open. Last completed full inventories stable1787/5015 and pinned1815/4727 are source b6738fa95. Fresh immutable c4f full stable then pinned audit remains separate and active.','Glob candidate e1598b30c is isolated and excluded from this qualification.','No selected-case sums or changed fixture inventories/deadlines/budgets. No speed claim.'],'raw':[raw(p)for p in paths]}
Path('evidence/task20-node-source-positions-core655-api368-2026-10-16.json').write_text(json.dumps(out,indent=2)+'\n')
print(source,out['checks'],out['fixed_work'])
