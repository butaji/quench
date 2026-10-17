import collections,hashlib,json,subprocess
from pathlib import Path
base=Path('work/native-buffer-unsafe');bundle=Path('work/native-buffer-unsafe-bin')
for stem in ['buffer-unsafe-focused','buffer-unsafe-qualify','buffer-unsafe-extras']:
 s=json.loads(Path('work/task-supervisor/'+stem+'-state.json').read_text());assert s['status']=='complete' and s['exit']==0,s
assert '360 passed; 0 failed' in (base/'all-api.log').read_text()
assert '21 passed; 0 failed' in (base/'focused.log').read_text()
m=json.loads((bundle/'source-sha256.json').read_text());assert all(hashlib.sha256(Path(n).read_bytes()).hexdigest()==h for n,h in m.items())
proof=json.loads((base/'unchanged-core653.json').read_text());assert len(proof['unchanged_files'])==301 and all(hashlib.sha256(Path(n).read_bytes()).hexdigest()==h for n,h in proof['unchanged_files'].items())
assert not subprocess.check_output(['git','diff','--','crates'],text=True)
source=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip();(bundle/'source-commit.txt').write_text(source+'\n')
for name in ['oracle','concat-oracle','latin1-oracle','poison-oracle']:assert not (base/(name+'.final.diff')).read_bytes()
originals={}
for name in ['test-buffer-alloc-alignment.js','test-buffer-islatin1.js','test-buffer-concat.js']:
 r=json.loads((base/(name+'.results.json')).read_text());c=dict(collections.Counter(x['status']for x in r['results']));assert c=={'pass':1},c;originals[name]=c
family=json.loads((base/'family.json').read_text());counts=dict(collections.Counter(x['status']for x in family['results']));assert counts=={'pass':52,'timeout':1},counts
fixed=json.loads((base/'fixed-work.json').read_text());assert fixed['complete'] and len(fixed['fixtures'])==8 and all(x['valid'] and x['output_equal']for x in fixed['fixtures'].values())
def raw(p):
 p=Path(p);b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
paths=[p for p in sorted(base.iterdir())if p.is_file()and p.name not in ['apply-next.py','retain.py','latin1-prototype.rs']]
paths += [bundle/n for n in ['source-commit.txt','source-sha256.json','sha256.txt']]
for stem in ['buffer-unsafe-focused','buffer-unsafe-qualify','buffer-unsafe-extras']:
 paths += [Path('work/task-supervisor/'+stem+s)for s in ['-state.json','.log']]
paths += [Path('work/task-supervisor/'+name)for name in ['buffer-unsafe-focused.json','buffer-unsafe-qualify.json','buffer-unsafe-extras.json','buffer-unsafe-quiet.json','buffer-unsafe-first-state.json','buffer-unsafe-first.log','buffer-unsafe-qualify-first-state.json','buffer-unsafe-qualify-first.log','buffer-unsafe-qualify-second-state.json','buffer-unsafe-qualify-second.log']]
out={'scope':'Latest Node aligned unsafe Buffer allocation, shared pooled concat, Latin1 classification and protected original validation policy','source_commit':source,'latest_stable':'v26.12.0','checks':{'unchanged_qualified_core':653,'unchanged_core_dependency_config_files':301,'api':360,'focused_buffer':21,'framework':43,'shared':7,'workspace_all_targets':True,'direct_differentials_equal':4},'unchanged_originals':originals,'family':{'counts':counts,'total':53,'inventory_hash':family['inventory_hash']},'fixed_work':{'valid':8,'clean':sum(bool(x['clean_rounds'])for x in fixed['fixtures'].values()),'speed_claim':False},'design':['One real private pool is shared by from/copyBytesFrom/allocUnsafe/concat. Slow and strict-alignment allocations own actual over-allocated ArrayBuffers.','Physical alignment is measured from fixed backing storage; independent snapshots preserve observed addresses and actual contents.','Original configured Node validators and coded constructors are captured before guest execution, including before replacement of internal exports.','Concat preserves original two-pass reads, reentrant allocation, truncation and real zero filling.','Latin1 classification reads genuine Core UTF16 code units; original Node validation rejects objects without coercion.','Core source, dependency and configuration bytes are unchanged from passing Core653 qualification.'],'limitations':['Goal remains open. Latest complete full stable1787/5015 and pinned1815/4727 use older b673 source, excluding this allocation extension and9b3 Core integration.','Real ShadowRealm stack callback regression remains recorded in both full audits; realm-local original Node policy is the next fix.','Two contended full API attempts each passed359/360 with distinct child-process500ms and TCP5s deadline failures. Both unchanged isolated controls pass; a subsequent quiet complete run passed360/360186.87s. Failed attempts are retained and not replaced by control sums.','First focused run20/21 caught private isLatin1 binding name mismatch; corrected actual empty name and reran21/21.','No speed claim:8fixed-work outputs valid,0clean rounds.','Iterator broadcast-waiter-cleanup remains an unchanged30s timeout. No fixture, deadline or skip changes.'],'raw':[raw(p)for p in paths]}
Path('evidence/task20-node-modern-buffer-core653-api360-2026-10-16.json').write_text(json.dumps(out,indent=2)+'\n');print(source,originals,counts)
