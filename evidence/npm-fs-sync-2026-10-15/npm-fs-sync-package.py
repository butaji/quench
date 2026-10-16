"""Preserve exact completed canonical integration evidence; never qualify partial runs."""
from pathlib import Path
import collections, gzip, hashlib, json, shutil
scratch=Path('/workspace/scratch')
packet=scratch/'quench-npm-file-url/evidence/npm-fs-sync-2026-10-15'
packet.mkdir(exist_ok=True)
prefix='npm-fs-sync'
assert (scratch/f'{prefix}-suite.exit').read_text().strip()=='0'
for label in ['fast-core','node-api','workspace-check','opt-core','node-lib']:
    assert (scratch/f'{prefix}-{label}.exit').read_text().strip()=='0',label
for p in sorted(scratch.glob(prefix+'-*')):
    if p.suffix not in {'.json','.log','.exit','.txt','.py','.sh'} or not p.is_file(): continue
    if p.name.endswith('-launch.log') and p.stat().st_size==0: continue
    # Retain raw bytes. Large JSON corpora/reports are compressed losslessly.
    if p.stat().st_size>65536:
        with p.open('rb') as src,gzip.GzipFile(filename=str(packet/(p.name+'.gz')),mode='wb',mtime=0) as dst:
            shutil.copyfileobj(src,dst)
    else: shutil.copy2(p,packet/p.name)
provenance=json.loads((scratch/f'{prefix}-engines.json').read_text())
assert provenance['node']['version']=='v26.12.0'
assert provenance['node']['sha256']=='caa24debe02770c92a70677e2a62ca98890150e7ac1c17bcfe31bcc98cc89943'
raw=Path('/workspace/quench/target/npm-fs-sync-instruction-guard/raw.txt')
shutil.copy2(raw,packet/f'{prefix}-instruction-guard-raw.txt')
rows={}
for line in raw.read_text().splitlines():
    side,fixture,n,ir=line.split();rows[side,fixture,int(n)]=int(ir)
plan={'crypto':1,'deltablue':10,'earley-boyer':1,'navier-stokes':1,'raytrace':3,'regexp':1,'richards':25,'splay':75}
guard=[]
for fixture,n in plan.items():
    a=(rows['base',fixture,n]-rows['base',fixture,0])/n
    b=(rows['cand',fixture,n]-rows['cand',fixture,0])/n
    guard.append({'fixture':fixture,'runs':n,'baseline_per_run':a,'candidate_per_run':b,'delta_percent':100*(b-a)/a})
completions=json.loads((scratch/f'{prefix}-guard-completions.json').read_text())
assert len(completions)==32 and all(x['passed'] for x in completions)
counts={}
for name in ['scenarios','baseline-scenarios','surface']:
    report=json.loads((scratch/f'{prefix}-{name}.json').read_text());assert len(report['records'])==300
    counts[name]=dict(collections.Counter(x['verdict'] for x in report['records']))
coverage=json.loads((scratch/f'{prefix}-coverage.json').read_text())
summary={'production_source':'b8b02f99245ee4e3392c84b75480733d7f135802',
         'baseline_source':'faa120035a332d3b5eb232eaadcad1399f306d7d','baseline_published_merge':'1f36ad64d05505dc438d381cb2d1dd9740f96140',
         'baseline_reused_observation':json.loads((scratch/f'{prefix}-baseline-scenarios-reuse.json').read_text()),
         'engines':json.loads((scratch/f'{prefix}-engines.json').read_text()),'counts':counts,
         'coverage':{'qualified':coverage['qualified'],'checked':sum(p['behavior_checked_paths'] for p in coverage['packages']),
                     'total':sum(p['discovered_callable_paths'] for p in coverage['packages'])},
         'instruction_guard':guard,'exact_guard_completions':len(completions),'node_api_provenance':provenance['node'],
         'instruction_findings_over_one_percent':[r for r in guard if r['delta_percent']>1],
         'total_node_parity':False,'package_performance_phase_started':False}
base=json.loads((scratch/f'{prefix}-baseline-scenarios.json').read_text())['records']
cand=json.loads((scratch/f'{prefix}-scenarios.json').read_text())['records']
base={r['package']['name']:r['verdict'] for r in base}
passed=lambda v:v in ['behavior-pass','declarations-pass']
summary['scenario_gains']=[r['package']['name'] for r in cand if passed(r['verdict']) and not passed(base[r['package']['name']])]
summary['scenario_regressions']=[r['package']['name'] for r in cand if not passed(r['verdict']) and passed(base[r['package']['name']])]
(packet/f'{prefix}-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))
