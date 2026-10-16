import hashlib,json,subprocess
from pathlib import Path
base=Path('work/owned-script-policy');bundle=Path('work/owned-script-bin')
for label in ['owned-script','owned-script-extras']:
 state=json.loads(Path('work/task-supervisor/'+label+'-state.json').read_text());assert state['status']=='complete'and state['exit']==0,state
for name,count in [('core.log',663),('all-api.log',380),('shared.log',7)]:assert f'{count} passed; 0 failed'in(base/name).read_text(),name
assert 'compiled_host_scripts_keep_source_identity_and_own_separate_realm_execution_state ... ok'in(base/'core.log').read_text()
assert 'owned_host_script_code_reclaims_after_owners_and_escaped_functions_drop ... ok'in(base/'core.log').read_text()
assert 'pass=43 skip=0 fail=0 timeout=0 crash=0 unclassified=0 total=43'in(base/'framework.log').read_text()
source=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip();assert source==(bundle/'source-commit.txt').read_text().strip()
assert not subprocess.check_output(['git','diff','--','crates'],text=True)
manifest=json.loads((bundle/'source-sha256.json').read_text());assert all(hashlib.sha256(Path(n).read_bytes()).hexdigest()==h for n,h in manifest.items())
for n in ['shadow.diff','buffer-oracle.diff','buffer-concat-oracle.diff','buffer-latin1-oracle.diff','buffer-poison-oracle.diff']:assert not(base/n).read_bytes(),n
scenarios=json.loads((base/'framework-scenarios.json').read_text());assert len(scenarios)==3 and all(r['matches']for r in scenarios)
fixed=json.loads((base/'fixed-work.json').read_text());assert fixed['complete']and len(fixed['fixtures'])==8 and all(r['valid']and r['output_equal']for r in fixed['fixtures'].values())
family=json.loads((base/'family-receipt.json').read_text());assert not family['lost']and not family['skip_changes'],family
index=Path('work/node-release-index-2026-10-16T1033.json');assert json.loads(index.read_text())[0]['version']=='v26.12.0'
def raw(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
paths=[p for p in sorted(base.rglob('*'))if p.is_file()and p.name!='retain.py'and '__pycache__'not in p.parts]
paths += [bundle/n for n in ['source-commit.txt','source-sha256.json','sha256.txt']]
paths += [Path('work/task-supervisor/'+label+s)for label in ['owned-script','owned-script-extras']for s in ['-state.json','-config.json','.log']]
paths += [p for p in sorted(Path('work/error-source-policy').glob('*'))if p.is_file()]
paths += [p for p in sorted(Path('work/test-runner-policy').glob('*'))if p.is_file()and p.suffix in ['.log','.json','.cjs','.rs','.py']]
paths += [Path('work/task-supervisor/'+label+s)for label in ['sdk-primitives','sdk-primitives-extras','error-source','error-source-extras']for s in ['-state.json','-config.json','.log']]
paths += [p for p in sorted(Path('work/function-info-fast-guard').glob('*'))if p.is_file()and p.suffix in ['.json','.log','.py','.rs','.sh']]
paths += [Path('work/function-info-before-bin')/n for n in ['source-commit.txt','source-sha256.json','sha256.txt']]
paths += [index]
output={'source_commit':source,'scope':'Owned logical host Scripts, shared function-info oracle repairs and canonical atom guard','oracle':'Node v26.12.0 latest stable, official index rechecked 2026-10-16T10:33Z','checks':{'core':663,'api':380,'framework':43,'shared':7,'workspace_all_targets':True,'owned_script_core_tests':2,'shared_function_info_oracles':2,'framework_scenarios':3,'empty_differentials':5},'tracked_source_files':len(manifest),'family':{k:family[k]for k in ['before','candidate','total','gained','lost','skip_changes']},'fixed_work':{'valid':8,'clean':sum(bool(r['clean_rounds'])for r in fixed['fixtures'].values()),'speed_claim':False},'design':['One logical source record retains immutable compiler code; each realm has a separate execution instance and caches. Detailed captures expose the actual shared source-record identity.','Opaque owners retain no guest values or strong realm roots. Realm weak associations are pruned before heap slot reuse; escaped functions retain their source records and otherwise unused code is reclaimed after owners drop.','Foreign runtime/reset handles reject using actual atom-sequence ownership, preserving private brands, fresh objects and realm-specific template identity.','All incoming function-constructor and prototype-info assertions remain intact. Named native-info lookup now checks canonical atom identities and keeps the exact prototype transition out of ordinary-key text inspection.'],'limitations':['All-parallel goal remains open. Latest complete stable321 full1795/5015 and pinned e9 full1821/4727; newer pinned321 audit is running separately.','Functional milestone only: fixed-work8valid0clean. Linux collector records RSS/output but no PMU counters; direct user-only perf_event_open returns ENOENT. Incoming failed1percent instruction guards are retained, not claimed passed or isolated to one change.','The initial new owned-Script tests exposed bypassed program-cache initialization:661existing passed and2new failed. Corrected by the shared complete initializer; unchanged new tests now pass within663/663.','Matched15-case assertion/OS family has no losses or skip changes, with no new full-case gain in this batch.','Native ContextifyScript, VM timeout/cache/code-generation and broader sourceURL/prototype warmup behavior remain incomplete. No placeholder class or false capability was added.'],'raw':[raw(p)for p in paths if p.exists()]}
Path('evidence/task20-node-owned-script-core663-api380-2026-10-16.json').write_text(json.dumps(output,indent=2)+'\n');print(source,output['checks'],output['family'])
