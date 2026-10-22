import json,hashlib,subprocess,collections,re
from pathlib import Path
for stem in ['buffer-integration','buffer-integration-extras']:
 d=json.loads(Path('work/task-supervisor/'+stem+'-state.json').read_text());assert d['status']=='complete' and d['exit']==0,d
base=Path('work/native-buffer-integration');bundle=Path('work/native-buffer-integrated-bin')
assert '653 passed; 0 failed' in (base/'core.log').read_text()
assert '356 passed; 0 failed' in (base/'all-api.log').read_text()
subset=re.search(r'test result: ok\. (\d+) passed; 0 failed', (base/'subset.log').read_text());assert subset
manifest=json.loads((bundle/'source-sha256.json').read_text());assert all(hashlib.sha256(Path(n).read_bytes()).hexdigest()==h for n,h in manifest.items())
source=(bundle/'source-commit.txt').read_text().strip();assert source==subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
family=json.loads((base/'family.json').read_text());counts=dict(collections.Counter(x['status']for x in family['results']));assert counts.get('pass',0)>=52 and not counts.get('fail',0),counts
fixed=json.loads((base/'fixed-work.json').read_text());assert fixed['complete'] and len(fixed['fixtures'])==8 and all(x['valid'] and x['output_equal']for x in fixed['fixtures'].values())
for p in [base/'allocation.diff',base/'pool.diff']:assert not p.read_bytes()
def raw(p):
 p=Path(p);b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
paths=[p for p in sorted(base.iterdir())if p.is_file()and p.name!='retain.py']
paths += [bundle/n for n in ['source-sha256.json','source-commit.txt','sha256.txt']]
paths += [Path('work/native-buffer-allocation')/n for n in ['incoming-crates.diff','incoming-merge.log','incoming-merge-commit.log','incoming-production-paths.txt','core650-completed-cache-reclamation.json']]
paths += [Path('work/latest-node-audit')/n for n in ['native-fulls-shared-merge.log','native-fulls-shared-diff.log','native-fulls-shared-push.log','native-fulls-shared-fetch.log','native-fulls-shared-fetch-explicit.log']]
out={'scope':'Normal integration of native Buffer pools with actual Core constructor identity, private boxed primitive data, generator cleanup and shape index','source_commit':source,'incoming_commit':'f7fbfd14903cc55449658d32b0cd9625ca5578d3','latest_stable':'v26.12.0','checks':{'core':653,'api':356,'subset':int(subset.group(1)),'framework':43,'shared':7,'workspace_all_targets':True,'direct_buffer_differentials_equal':True},'family':{'counts':counts,'total':len(family['results']),'inventory_hash':family['inventory_hash']},'fixed_work':{'valid':8,'clean':sum(bool(v['clean_rounds'])for v in fixed['fixtures'].values()),'speed_claim':False},'limitations':['Goal remains open. Latest complete totals stable1783/5015 and pinned1809/4727 are from older c4ad source.','Fresh full stable→pinned audit running separately from frozen b673 source does not include this merge.','Compilation overlaps the independent full inventory, no speed claim or isolated timing attribution.','New aligned public allocUnsafe/allocUnsafeSlow and pooled concat are still only uncompiled work prototypes; not included in this source.','Single test-file append conflict kept both native Buffer tests and incoming boxed primitive test. Raw incoming evidence whitespace was preserved; production diff check clean.'],'raw':[raw(p)for p in paths]}
Path('evidence/task20-node-buffer-core-integration-core653-api356-2026-10-16.json').write_text(json.dumps(out,indent=2)+'\n');print(source,counts,'subset',subset.group(1))
