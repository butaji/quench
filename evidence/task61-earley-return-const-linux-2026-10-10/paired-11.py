import hashlib, importlib.util, json, pathlib, statistics, subprocess, sys
ROOT=pathlib.Path(__file__).resolve().parents[3]; OUT=pathlib.Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('screen_runner',OUT/'benchmark-runner.py')
mod=importlib.util.module_from_spec(spec); spec.loader.exec_module(mod)
raw=json.loads((ROOT/'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json').read_text())
env=next(e['environment'] for e in raw['engines'] if e['name']=='quench')
base=OUT/'baseline-binary'; cand=OUT/'candidate-binary'; fixture=mod.materialize('earley-boyer'); probe=OUT/'returnconst-oracle.js'; node='/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node'
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
assert sha(base)=='e46537823eb25a37cf5646af793e6873ceaeff1069fb878a96813cae3a86ab32'
assert sha(cand)=='b5e48540e41d50373cca5e2c5988f9d140ef1f6d24ceb48a286e1e894addd9e7'
assert sha(fixture)=='aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b'
expected=subprocess.run([node,str(probe)],text=True,capture_output=True,check=True).stdout.splitlines()
for name,binary in [('baseline',base),('candidate',cand)]:
 r=subprocess.run([str(binary),str(probe)],text=True,capture_output=True,check=True)
 if r.stdout.splitlines()!=expected: raise RuntimeError(f'{name} return oracle differs from Node')
rounds=int(sys.argv[1]); tag=sys.argv[2] if len(sys.argv)>2 else str(rounds); log=OUT/f'returnconst-pairs-{tag}.jsonl'
if log.exists(): raise RuntimeError(f'refusing to overwrite {log}')
pairs=[]
for pair in range(1,rounds+1):
 order=['baseline','candidate'] if pair%2 else ['candidate','baseline']; result={}
 for variant in order: result[variant]=mod.run(base if variant=='baseline' else cand,fixture,env,f'returnconst-paired-{tag}-{pair:02d}-{variant}')
 b,c=result['baseline'],result['candidate']; equal=b['semantic_output']==c['semantic_output']
 row={'pair':pair,'order':order,'valid':b['exit_code']==c['exit_code']==0 and b['score'] is not None and c['score'] is not None and equal,'output_equal':equal,'baseline_score':b['score'],'candidate_score':c['score'],'score_delta':c['score']-b['score'],'baseline_rss_bytes':b['maximum_rss_bytes'],'candidate_rss_bytes':c['maximum_rss_bytes'],'rss_delta_bytes':c['maximum_rss_bytes']-b['maximum_rss_bytes'],'baseline_wall_ns':b['wall_ns'],'candidate_wall_ns':c['wall_ns']}
 pairs.append(row)
 with log.open('a') as f: f.write(json.dumps(row,separators=(',',':'))+'\n'); f.flush()
 print(f"pair {pair}/{rounds}: score {b['score']}->{c['score']} ({row['score_delta']:+g}); rss {b['maximum_rss_bytes']}->{c['maximum_rss_bytes']} ({row['rss_delta_bytes']:+d}); equal={equal}",flush=True)
def interval(values,seed):
 import random
 rng=random.Random(seed); draws=sorted(statistics.median(rng.choices(values,k=len(values))) for _ in range(100_000))
 return [draws[2500],draws[97499]]
scores=[p['score_delta'] for p in pairs]; rss=[p['rss_delta_bytes'] for p in pairs]
report={'experiment':'EarleyBoyer LoadConst -> Return direct-return fusion','baseline_sha256':sha(base),'candidate_sha256':sha(cand),'fixture_sha256':sha(fixture),'node_oracle_sha256':sha(probe),'pairs':pairs,'rounds':rounds,'bootstrap_replicates':100_000,'baseline_score_median':statistics.median(p['baseline_score'] for p in pairs),'candidate_score_median':statistics.median(p['candidate_score'] for p in pairs),'score_delta_median':statistics.median(scores),'score_delta_95_ci':interval(scores,0x610ecb5),'baseline_maximum_rss_median_bytes':statistics.median(p['baseline_rss_bytes'] for p in pairs),'candidate_maximum_rss_median_bytes':statistics.median(p['candidate_rss_bytes'] for p in pairs),'maximum_rss_delta_median_bytes':statistics.median(rss),'maximum_rss_delta_95_ci_bytes':interval(rss,0x610ecb6),'valid':all(p['valid'] for p in pairs),'output_equal':all(p['output_equal'] for p in pairs),'qualification_ready':False}
(OUT/f'returnconst-paired-{tag}.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k!='pairs'},indent=2))
