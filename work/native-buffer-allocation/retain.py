import collections,hashlib,json,subprocess
from pathlib import Path
root=Path('.')
def raw(name):
 p=Path(name);b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
for name in ['buffer-qualify','buffer-extras']:
 s=json.loads(Path('work/task-supervisor/'+name+'-state.json').read_text());assert s['status']=='complete' and s['exit']==0,s
assert '650 passed; 0 failed' in Path('work/native-buffer-allocation/core-accounting.log').read_text()
assert '351 passed; 0 failed' in Path('work/native-buffer-allocation/all-api.log').read_text()
bundle=Path('work/native-buffer-pool-bin')
manifest=json.loads((bundle/'source-sha256.json').read_text())
assert all(hashlib.sha256(Path(n).read_bytes()).hexdigest()==h for n,h in manifest.items())
assert not subprocess.check_output(['git','diff','--','crates'],text=True),'Commit qualified source before retaining evidence'
source=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
(bundle/'source-commit.txt').write_text(source+'\n')
family=json.loads(Path('work/native-buffer-pool/accounted-family.json').read_text())
counts=dict(collections.Counter(x['status']for x in family['results']));assert counts=={'pass':52,'timeout':1},counts
old=json.loads(Path('work/node-error-stack-policy/final-family.json').read_text())
def statuses(x):return {Path(r['fixture']).name:r['status']for r in x['results']}
a,b=statuses(old),statuses(family);assert a.keys()==b.keys()
gained=[n for n in b if b[n]=='pass'and a[n]!='pass'];lost=[n for n in b if b[n]!='pass'and a[n]=='pass'];assert gained==['test-stream-iter-readable-interop.js'] and not lost,(gained,lost)
for p in ['work/native-buffer-allocation/accounted-diff.log','work/native-buffer-pool/accounted-diff.log']:assert not Path(p).read_bytes()
paths=['work/native-buffer-allocation/'+n for n in ['core-accounting.log','accounting-check.log','accounted-paired-build.log','all-api.log','framework.log','shared.log','workspace.log','accounted-oracle.log','accounted-candidate.log','accounted-diff.log','qualify.sh','run-extras.sh','core-before-accounting.log','core-disk-full.log','failed-link-reclamation.json','immutable-bundle-dedup.json','interrupted-run-orphan-cleanup.json']]
paths += ['work/native-buffer-pool/'+n for n in ['accounted-oracle.log','accounted-candidate.log','accounted-diff.log','accounted-family.json','accounted-family.log','oracle.cjs','waiter-probe.cjs','waiter-probe.log','waiter-probe-node.log','full-failure-priorities.json']]
paths += ['work/native-buffer-allocation/oracle.cjs','work/native-buffer-pool-fixed-work.json','work/native-buffer-pool-fixed-work.log',str(bundle/'source-sha256.json'),str(bundle/'sha256.txt'),str(bundle/'source-commit.txt')]
fixed=json.loads(Path('work/native-buffer-pool-fixed-work.json').read_text());assert fixed['complete'] and len(fixed['fixtures'])==8 and all(x['valid'] and x['output_equal'] for x in fixed['fixtures'].values())
paths += [str(p) for p in sorted(Path('work/node-api-surface').iterdir()) if p.is_file()]
paths += ['work/task-supervisor/buffer-extras-missing-companion.log','work/task-supervisor/buffer-extras-missing-companion-state.json']
out={'scope':'Native ArrayBuffer physical alignment and real shared Buffer.from/copyBytesFrom pools','source_commit':source,'latest_stable':'v26.12.0','checks':{'core':650,'latest_node_api':351,'paired_native_build':True,'framework':True,'shared_regressions':True,'workspace_all_targets':True,'direct_oracles_equal':True},'family':{'counts':counts,'total':len(b),'inventory_hash':family['inventory_hash'],'gained':gained,'lost':lost},'fixed_work':{'valid':8,'clean':sum(bool(x['clean_rounds'])for x in fixed['fixtures'].values()),'qualification_ready':fixed['qualification_ready'],'speed_claim':False},'design':['Core fixed observed backings preserve physical address across snapshots, copying snapshots independently; ordinary buffers retain COW.','Pinning an externally shared zero store accounts actual newly owned allocation once. Core owns storage and alignment; Node owns Buffer ABI/pool policy.','Native allocation creates a real ArrayBuffer with initialized bytes. Unsafe permits zero initialization. No synthetic pointers or zero-fill toggles.','One shared private pool state handles from(strings/arraylike) and copyBytesFrom, actual mark-untransferable, physical base alignment and reentrant cursor semantics.','Original latest Node library remains guest code over genuine Rust primitives; no Node guest execution.'],'limitations':['Goal remains open. Latest complete stable full1783/5015 is from older c4ad13141, not this source. Do not add selected gains to full totals.','Remaining selected broadcast-waiter-cleanup times out unchanged at30s; own smaller-count diagnostic completes without consumers. This is not retention proof.','allocUnsafe/concat remain legacy unpooled and latest aligned public allocation arguments are not yet implemented.','No setDetachKey export until complete real detach-key semantics exist.','Fixed-work diagnostic does not establish a speed claim; compilation/full inventory overlap precludes isolated timing attribution.','Core before latest accounting passed649; linker disk exhaustion was retained and recovered using only failed owned temporary plus byte-identical immutable binary hardlink dedup.'],'raw':[raw(p) for p in paths]}
Path('evidence/task20-node-buffer-native-alignment-pool-core650-api351-2026-10-16.json').write_text(json.dumps(out,indent=2)+'\n')
print(source,counts,gained)
